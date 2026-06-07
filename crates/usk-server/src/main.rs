use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Weak};

use axum::{
    body::Bytes,
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use tokio::sync::{Mutex, RwLock};
use usk_core::index::RegistryIndex;

/// Per-(name, version) publish lock map. The outer `Mutex` guards the
/// `HashMap`; each entry stores a `Weak` ref so the lock is released
/// the moment the last strong holder drops it. `acquire_publish_lock`
/// cleans up dead entries on access, so the map only contains entries
/// for versions currently being published.
type LockMap = Arc<Mutex<HashMap<(String, String), Weak<Mutex<()>>>>>;

#[derive(Clone)]
struct AppState {
    index: Arc<RwLock<RegistryIndex>>,
    registry_path: PathBuf,
    publish_locks: LockMap,
}

#[derive(Deserialize)]
struct SearchParams {
    q: Option<String>,
    tags: Option<String>,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let registry_path = std::env::var("USK_REGISTRY_PATH")
        .unwrap_or_else(|_| "./registry".to_string());

    let mut index = RegistryIndex::new();
    let path = PathBuf::from(&registry_path);
    if path.exists() {
        index.load_from_dir(&path).expect("failed to load registry index");
        tracing::info!("Loaded {} skills from {:?}", index.len(), path);
    } else {
        tracing::warn!("Registry path {:?} does not exist, starting with empty index", path);
    }

    let state = AppState {
        index: Arc::new(RwLock::new(index)),
        registry_path: path,
        publish_locks: Arc::new(Mutex::new(HashMap::new())),
    };

    let app = Router::new()
        .route("/api/v1/search", get(handle_search))
        .route("/api/v1/packages/{name}", get(handle_package))
        .route("/api/v1/packages/{name}/versions", get(handle_versions))
        .route("/api/v1/packages/{name}/{version}", get(handle_package_version))
        .route("/api/v1/packages/{name}/{version}/download", get(handle_download))
        .route("/api/v1/publish", post(handle_publish))
        .with_state(state);

    let addr = "0.0.0.0:8080";
    tracing::info!("Starting usk-server on {}", addr);
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn handle_search(
    State(state): State<AppState>,
    Query(params): Query<SearchParams>,
) -> Json<serde_json::Value> {
    let index = state.index.read().await;

    let results = match (params.q.as_deref(), params.tags.as_deref()) {
        (Some(query), _) if !query.is_empty() => {
            let mut results = index.search(query);
            results.sort_by(|a, b| a.name.cmp(&b.name));
            results.into_iter().map(|m| serde_json::to_value(m).unwrap()).collect::<Vec<_>>()
        }
        (_, Some(tag)) if !tag.is_empty() => {
            let mut results = index.search_by_tag(tag);
            results.sort_by(|a, b| a.name.cmp(&b.name));
            results.into_iter().map(|m| serde_json::to_value(m).unwrap()).collect::<Vec<_>>()
        }
        _ => {
            let mut results: Vec<_> = index.all().to_vec();
            results.sort_by(|a, b| a.name.cmp(&b.name));
            results.into_iter().map(|m| serde_json::to_value(m).unwrap()).collect::<Vec<_>>()
        }
    };

    Json(serde_json::json!({
        "count": results.len(),
        "results": results
    }))
}

async fn handle_package(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> (StatusCode, Json<serde_json::Value>) {
    let index = state.index.read().await;

    match index.get(&name) {
        Some(skill) => (StatusCode::OK, Json(serde_json::to_value(skill).unwrap())),
        None => (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "not found"}))),
    }
}

async fn handle_versions(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> (StatusCode, Json<serde_json::Value>) {
    let index = state.index.read().await;

    if index.get(&name).is_some() {
        let versions: Vec<String> = index
            .all()
            .iter()
            .filter(|s| s.name == name)
            .map(|s| s.version.clone())
            .collect();
        (StatusCode::OK, Json(serde_json::json!({
            "name": name,
            "versions": versions
        })))
    } else {
        (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "not found"})))
    }
}

async fn handle_package_version(
    State(state): State<AppState>,
    Path((name, version)): Path<(String, String)>,
) -> (StatusCode, Json<serde_json::Value>) {
    let index = state.index.read().await;

    let found = index
        .all()
        .iter()
        .find(|s| s.name == name && s.version == version);

    match found {
        Some(skill) => (StatusCode::OK, Json(serde_json::to_value(skill).unwrap())),
        None => (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "not found"}))),
    }
}

async fn handle_download(
    State(state): State<AppState>,
    Path((name, version)): Path<(String, String)>,
) -> Result<(StatusCode, [(String, String); 2], Vec<u8>), (StatusCode, Json<serde_json::Value>)> {
    let skill_dir = state.registry_path.join(&name).join(&version);
    if !skill_dir.exists() {
        return Err((StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "not found"}))));
    }

    let mut buf = Vec::new();
    {
        let encoder = flate2::write::GzEncoder::new(&mut buf, flate2::Compression::default());
        let mut archive = tar::Builder::new(encoder);
        archive
            .append_dir_all(".", &skill_dir)
            .map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({"error": format!("archive failed: {}", e)})),
                )
            })?;
        let encoder = archive.into_inner().map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": format!("archive failed: {}", e)})),
            )
        })?;
        encoder.finish().map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": format!("archive failed: {}", e)})),
            )
        })?;
    }

    Ok((
        StatusCode::OK,
        [
            ("Content-Type".to_string(), "application/gzip".to_string()),
            (
                "Content-Disposition".to_string(),
                format!("attachment; filename=\"{}-{}.tar.gz\"", name, version),
            ),
        ],
        buf,
    ))
}

async fn handle_publish(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    body: Bytes,
) -> (StatusCode, Json<serde_json::Value>) {
    let name = match headers.get("X-Skill-Name").and_then(|v| v.to_str().ok()) {
        Some(n) => n.to_string(),
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "missing X-Skill-Name header"})),
            )
        }
    };
    let version = match headers.get("X-Skill-Version").and_then(|v| v.to_str().ok()) {
        Some(v) => v.to_string(),
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "missing X-Skill-Version header"})),
            )
        }
    };

    let dest = state.registry_path.join(&name).join(&version);

    // Acquire the per-(name, version) lock BEFORE any existence check or I/O,
    // then re-check existence under the lock. This closes the TOCTOU race
    // between two concurrent publishes of the same (name, version).
    let per_version_lock = acquire_publish_lock(&state.publish_locks, &name, &version).await;
    let _guard = per_version_lock.lock().await;

    // Versions are immutable: reject re-publish with 409.
    if dest.exists() {
        return (
            StatusCode::CONFLICT,
            Json(serde_json::json!({
                "error": format!(
                    "version '{}' of skill '{}' already exists; versions are immutable",
                    version, name
                )
            })),
        );
    }

    if let Err(e) = std::fs::create_dir_all(&dest) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("failed to create directory: {}", e)})),
        );
    }

    // SECURITY: validate tarball entries for path traversal BEFORE unpacking.
    // We do two passes: one for validation, one for extraction.
    if let Err(e) = validate_tarball_safety(&body) {
        tracing::error!("rejecting tarball from publish of {}@{}: {}", name, version, e);
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": e})),
        );
    }

    let decoder = flate2::read::GzDecoder::new(&body[..]);
    let mut archive = tar::Archive::new(decoder);
    if let Err(e) = archive.unpack(&dest) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("failed to extract archive: {}", e)})),
        );
    }

    // Reload index to include the new package
    {
        let mut index = state.index.write().await;
        *index = RegistryIndex::new();
        if state.registry_path.exists() {
            if let Err(e) = index.load_from_dir(&state.registry_path) {
                tracing::error!("failed to reload index: {}", e);
            }
        }
    }

    // Best-effort git commit of registry changes. Never fails the publish.
    if let Err(e) = try_git_commit(&state.registry_path, &name, &version) {
        tracing::warn!("git commit failed for {}@{}: {}", name, version, e);
    }

    tracing::info!("Published {} v{}", name, version);
    (
        StatusCode::OK,
        Json(serde_json::json!({"status": "published", "name": name, "version": version})),
    )
}

/// Get or create the per-(name, version) lock used to serialize concurrent
/// publishes. Reuses an existing strong ref if one is still alive; otherwise
/// installs a fresh `Arc` and stores a `Weak` ref so the entry is reclaimed
/// the moment the last publish drops it.
async fn acquire_publish_lock(
    locks: &LockMap,
    name: &str,
    version: &str,
) -> Arc<Mutex<()>> {
    let mut map = locks.lock().await;
    let key = (name.to_string(), version.to_string());
    if let Some(weak) = map.get(&key) {
        if let Some(arc) = weak.upgrade() {
            return arc;
        }
    }
    map.remove(&key);
    let arc = Arc::new(Mutex::new(()));
    map.insert(key, Arc::downgrade(&arc));
    arc
}

/// Validate a gzipped tarball for unsafe paths.
/// Rejects absolute paths and parent-directory traversal.
fn validate_tarball_safety(body: &[u8]) -> Result<(), String> {
    let cursor = std::io::Cursor::new(body);
    let decoder = flate2::read::GzDecoder::new(cursor);
    let mut archive = tar::Archive::new(decoder);
    let entries = archive
        .entries()
        .map_err(|e| format!("failed to read archive: {}", e))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("invalid entry: {}", e))?;
        let path = entry
            .path()
            .map_err(|e| format!("invalid entry path: {}", e))?;
        if path.is_absolute() {
            return Err("absolute paths not allowed".to_string());
        }
        if path
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            return Err("parent traversal not allowed".to_string());
        }
    }
    Ok(())
}

/// Best-effort git commit on the registry after a publish.
/// Returns Err on git failure but callers should not propagate it.
fn try_git_commit(
    registry_path: &std::path::Path,
    name: &str,
    version: &str,
) -> Result<(), git2::Error> {
    // Discover or init the repo
    let repo = match git2::Repository::discover(registry_path) {
        Ok(r) => r,
        Err(_) => {
            let repo = git2::Repository::init(registry_path)?;
            // Create an initial empty commit on main if HEAD is unborn
            if repo.head().is_err() {
                let sig = git2::Signature::now("usk-server", "usk-server@localhost")?;
                let tree_oid = repo.index()?.write_tree()?;
                let tree = repo.find_tree(tree_oid)?;
                let _ = repo.commit(
                    Some("refs/heads/main"),
                    &sig,
                    &sig,
                    "initial commit",
                    &tree,
                    &[],
                )?;
                // Point HEAD at main explicitly
                let main_ref = repo.find_reference("refs/heads/main")?;
                repo.set_head(main_ref.name().unwrap_or("refs/heads/main"))?;
            }
            repo
        }
    };

    // Stage all changes
    let mut index = repo.index()?;
    index.add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)?;
    index.write()?;

    // Skip if no changes - compare HEAD tree to the index tree
    if let Ok(head) = repo.head() {
        if let Ok(head_commit) = head.peel_to_commit() {
            let head_tree = head_commit.tree()?;
            let index_tree_oid = index.write_tree()?;
            let index_tree = repo.find_tree(index_tree_oid)?;
            if head_tree.id() == index_tree.id() {
                return Ok(());
            }
        }
    }

    let tree_oid = index.write_tree()?;
    let tree = repo.find_tree(tree_oid)?;
    let sig = git2::Signature::now("usk-server", "usk-server@localhost")?;

    // Get parent commit (HEAD may be unborn in edge cases)
    let parent = repo.head().ok().and_then(|h| h.peel_to_commit().ok());

    let parents: Vec<&git2::Commit> = match &parent {
        Some(p) => vec![p],
        None => vec![],
    };

    let msg = format!("publish: {}@{}", name, version);
    let _oid = repo.commit(
        Some("refs/heads/main"),
        &sig,
        &sig,
        &msg,
        &tree,
        &parents,
    )?;

    tracing::info!("git commit: {}", msg);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// Build a gzipped tarball in memory from a list of (path, content) entries.
    /// Paths are written verbatim (bypassing tar::Builder's safety checks) so
    /// we can construct adversarial inputs for security tests.
    fn build_gzipped_tarball(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut tar_bytes = Vec::new();
        for (path, content) in entries {
            // Build a minimal POSIX ustar header (512 bytes) by hand.
            let mut header = [0u8; 512];
            let path_bytes = path.as_bytes();
            assert!(path_bytes.len() < 100, "test path too long");
            header[..path_bytes.len()].copy_from_slice(path_bytes);

            // mode (octal, ASCII): "0000644\0"
            let mode = b"0000644\0";
            header[100..108].copy_from_slice(mode);
            // uid, gid
            header[108..116].copy_from_slice(b"0000000\0");
            header[116..124].copy_from_slice(b"0000000\0");
            // size (octal, ASCII, 11 bytes + NUL)
            let size_str = format!("{:011o}\0", content.len());
            header[124..136].copy_from_slice(size_str.as_bytes());
            // mtime
            header[136..148].copy_from_slice(b"00000000000\0");
            // typeflag = '0' (regular file)
            header[148] = b'0';
            // magic = "ustar\0" (POSIX)
            header[257..263].copy_from_slice(b"ustar\0");
            // version = "00"
            header[263..265].copy_from_slice(b"00");
            // uname/gname empty
            // compute checksum: sum of all bytes treating checksum field as spaces
            let mut checksum: u32 = 0;
            for (i, b) in header.iter().enumerate() {
                if (148..156).contains(&i) {
                    checksum += b' ' as u32;
                } else {
                    checksum += *b as u32;
                }
            }
            let chk_str = format!("{:06o}\0 ", checksum);
            header[148..156].copy_from_slice(chk_str.as_bytes());

            tar_bytes.extend_from_slice(&header);
            // pad content to 512-byte boundary
            tar_bytes.extend_from_slice(content);
            let pad = (512 - (content.len() % 512)) % 512;
            tar_bytes.extend(std::iter::repeat(0u8).take(pad));
        }
        // Two 512-byte blocks of EOF
        tar_bytes.extend(std::iter::repeat(0u8).take(1024));

        let mut gz_bytes = Vec::new();
        {
            let mut encoder =
                flate2::write::GzEncoder::new(&mut gz_bytes, flate2::Compression::default());
            encoder.write_all(&tar_bytes).unwrap();
            encoder.finish().unwrap();
        }
        gz_bytes
    }

    #[test]
    fn test_path_traversal_rejected() {
        // Build a tarball with a malicious `../malicious` entry
        let body = build_gzipped_tarball(&[("../malicious", b"pwned")]);

        let result = validate_tarball_safety(&body);
        assert!(result.is_err(), "expected path traversal to be rejected");
        let err = result.unwrap_err();
        assert!(
            err.contains("parent traversal"),
            "expected parent traversal error, got: {}",
            err
        );
    }

    #[test]
    fn test_path_traversal_rejected_absolute() {
        // Build a tarball with an absolute path entry
        let body = build_gzipped_tarball(&[("/etc/passwd", b"pwned")]);

        let result = validate_tarball_safety(&body);
        assert!(result.is_err(), "expected absolute path to be rejected");
        let err = result.unwrap_err();
        assert!(
            err.contains("absolute paths"),
            "expected absolute path error, got: {}",
            err
        );
    }

    #[test]
    fn test_safe_tarball_accepted() {
        // Normal tarball with safe entries
        let body = build_gzipped_tarball(&[
            ("normal.txt", b"hello"),
            ("subdir/inner.txt", b"world"),
        ]);

        let result = validate_tarball_safety(&body);
        assert!(result.is_ok(), "expected safe tarball to be accepted, got: {:?}", result);
    }

    #[test]
    fn test_path_traversal_does_not_escape_dest() {
        // End-to-end: confirm the validation function, not just the unpacker,
        // would prevent a `../../../tmp/usk_pwned` escape.
        let body = build_gzipped_tarball(&[("../../../tmp/usk_pwned", b"pwn")]);
        let result = validate_tarball_safety(&body);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("parent traversal"));
    }

    #[test]
    fn test_git_commit_helper() {
        // Use a tempdir as a fake registry. The helper should not panic,
        // and should produce a commit or return Ok even on a fresh dir.
        let tmp = tempfile::tempdir().unwrap();
        let result = try_git_commit(tmp.path(), "test-skill", "0.1.0");
        // Best-effort: should not panic. May succeed or fail depending on env.
        // We only assert it does not crash the test process.
        let _ = result;
    }

    #[tokio::test]
    async fn test_acquire_publish_lock_returns_same_arc_for_same_key() {
        let locks: LockMap = Arc::new(Mutex::new(HashMap::new()));
        let lock1 = acquire_publish_lock(&locks, "a", "1.0.0").await;
        let lock2 = acquire_publish_lock(&locks, "a", "1.0.0").await;
        assert!(
            Arc::ptr_eq(&lock1, &lock2),
            "expected same Arc allocation for same (name, version) key"
        );
    }

    #[tokio::test]
    async fn test_acquire_publish_lock_distinguishes_keys() {
        let locks: LockMap = Arc::new(Mutex::new(HashMap::new()));
        let lock_a1 = acquire_publish_lock(&locks, "a", "1.0.0").await;
        let lock_a2 = acquire_publish_lock(&locks, "a", "2.0.0").await;
        let lock_b1 = acquire_publish_lock(&locks, "b", "1.0.0").await;
        assert!(
            !Arc::ptr_eq(&lock_a1, &lock_a2),
            "different versions of same skill should have different locks"
        );
        assert!(
            !Arc::ptr_eq(&lock_a1, &lock_b1),
            "same version of different skills should have different locks"
        );
        // And re-acquiring the same key still returns the same Arc
        let lock_a1_again = acquire_publish_lock(&locks, "a", "1.0.0").await;
        assert!(Arc::ptr_eq(&lock_a1, &lock_a1_again));
    }

    #[tokio::test]
    async fn test_concurrent_publish_serializes() {
        use std::sync::atomic::{AtomicU32, Ordering};

        let locks: LockMap = Arc::new(Mutex::new(HashMap::new()));
        let per_version_lock = acquire_publish_lock(&locks, "a", "1.0.0").await;

        // Two tasks race for the same per-version lock. We track the
        // ordering in which they enter and exit the critical section.
        // The expected sequence is: t1_enter -> t1_exit -> t2_enter -> t2_exit.
        // Any other sequence means the lock failed to serialize them.
        let order: Arc<AtomicU32> = Arc::new(AtomicU32::new(0));
        // Steps: 0=nothing, 1=t1_entered, 2=t1_exited, 3=t2_entered, 4=t2_exited
        let order1 = order.clone();
        let order2 = order.clone();

        let lock_for_task1 = per_version_lock.clone();
        let task1 = tokio::spawn(async move {
            let _guard = lock_for_task1.lock().await;
            order1.store(1, Ordering::SeqCst);
            tokio::time::sleep(std::time::Duration::from_millis(80)).await;
            order1.store(2, Ordering::SeqCst);
        });

        let lock_for_task2 = per_version_lock.clone();
        let task2 = tokio::spawn(async move {
            let _guard = lock_for_task2.lock().await;
            order2.store(3, Ordering::SeqCst);
            order2.store(4, Ordering::SeqCst);
        });

        let res = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            async {
                let _ = task1.await;
                let _ = task2.await;
            },
        )
        .await;
        assert!(res.is_ok(), "tasks did not complete within timeout");

        // The final step should be 4 (both finished in order).
        // Most importantly, step 3 (task 2 entered) must come AFTER step 2 (task 1 exited).
        let final_step = order.load(Ordering::SeqCst);
        assert_eq!(
            final_step, 4,
            "unexpected ordering: final step was {} (expected 4, meaning t1 enter -> t1 exit -> t2 enter -> t2 exit)",
            final_step
        );
    }
}
