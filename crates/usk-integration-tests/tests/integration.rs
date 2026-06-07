//! End-to-end integration tests for the `usk` CLI and `usk-server`.
//!
//! These tests spawn the actual binaries against temporary state
//! directories, drive them with real HTTP and subprocess calls, and
//! assert on observable side effects (filesystem contents, JSON
//! responses, CLI output).
//!
//! Because the server is hardcoded to bind on `0.0.0.0:8080`, the
//! tests that need a live server use `#[serial]` to avoid port
//! collisions. Cargo runs `#[serial]` tests one at a time.

use std::path::PathBuf;
use std::time::Duration;

use serde_json::Value;
use serial_test::serial;
use usk_integration_tests::{build_tarball, run_cli, wait_for_path, TestEnv, TestServer};

/// Absolute path to the example escalation skill that ships with the
/// repo. Used as a fixture for end-to-end publish/install flows.
fn example_skill_path() -> PathBuf {
    // The integration test runs from the workspace root via `cargo test`,
    // so the repo-root-relative path below resolves correctly.
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .join("spec/examples/escalation-handling")
}

// ---------------------------------------------------------------------
// Test 1: end-to-end publish -> search -> install -> list
// ---------------------------------------------------------------------
#[test]
#[serial]
fn test_publish_search_install_list_endtoend() {
    let server = TestServer::spawn();
    let env = TestEnv::new(&server.base_url);

    // 1. Publish the example skill
    let skill_path = example_skill_path();
    assert!(
        skill_path.exists(),
        "example skill fixture missing at {:?}",
        skill_path
    );
    let mut publish = env.usk_cmd();
    publish.arg("publish").arg(&skill_path);
    let (out, err, code) = run_cli(&mut publish);
    assert_eq!(
        code, 0,
        "usk publish failed: stdout={}\nstderr={}",
        out, err
    );
    assert!(
        out.contains("Published"),
        "expected publish success line, got: {}",
        out
    );

    // 2. Search for it
    let mut search = env.usk_cmd();
    search.arg("search").arg("escalation");
    let (out, err, code) = run_cli(&mut search);
    assert_eq!(code, 0, "usk search failed: stderr={}", err);
    assert!(
        out.contains("escalation-handling"),
        "expected escalation-handling in search output, got: {}",
        out
    );

    // 3. Install it
    let mut install = env.usk_cmd();
    install.arg("install").arg("escalation-handling").arg("--harness").arg("claude-code");
    let (out, err, code) = run_cli(&mut install);
    assert_eq!(code, 0, "usk install failed: stdout={}\nstderr={}", out, err);

    // 4. Install dir assertions
    let skill_install_dir = env.install_dir.join("claude-code").join("escalation-handling");
    assert!(
        wait_for_path(&skill_install_dir.join("SKILL.md"), Duration::from_secs(5)),
        "install dir missing SKILL.md: {:?}",
        skill_install_dir
    );

    // 5. Config file shows it as installed
    let cfg_path = env.config_dir.path().join("config.toml");
    let cfg_text = std::fs::read_to_string(&cfg_path).expect("read config.toml");
    assert!(
        cfg_text.contains("escalation-handling"),
        "config.toml missing installed entry: {}",
        cfg_text
    );
    assert!(
        cfg_text.contains("claude-code"),
        "config.toml missing harness name: {}",
        cfg_text
    );

    // 6. `usk list` shows it
    let mut list = env.usk_cmd();
    list.arg("list");
    let (out, err, code) = run_cli(&mut list);
    assert_eq!(code, 0, "usk list failed: stderr={}", err);
    assert!(
        out.contains("escalation-handling"),
        "usk list did not contain installed skill, got: {}",
        out
    );

    drop(server);
}

// ---------------------------------------------------------------------
// Test 2: server search by tag (direct HTTP)
// ---------------------------------------------------------------------
#[test]
#[serial]
fn test_server_search_by_tag_http() {
    let server = TestServer::spawn();
    let env = TestEnv::new(&server.base_url);

    // Publish the escalation example (its skill.yaml has tag: support)
    let mut publish = env.usk_cmd();
    publish.arg("publish").arg(example_skill_path());
    let (_, _, code) = run_cli(&mut publish);
    assert_eq!(code, 0, "publish failed");

    // Give the server a moment to reload its index from disk.
    std::thread::sleep(Duration::from_millis(200));

    // Direct HTTP search by tag
    let client = server.client();
    let resp = client
        .get(format!("{}/api/v1/search?tags=support", server.base_url))
        .send()
        .expect("HTTP request");
    assert!(resp.status().is_success(), "tag search HTTP failed: {}", resp.status());

    let body: Value = resp.json().expect("parse JSON");
    let results = body["results"].as_array().expect("results array");
    assert!(
        !results.is_empty(),
        "expected at least one result for tag 'support', got: {}",
        body
    );
    let names: Vec<String> = results
        .iter()
        .filter_map(|r| r["name"].as_str().map(|s| s.to_string()))
        .collect();
    assert!(
        names.iter().any(|n| n == "escalation-handling"),
        "expected escalation-handling in results, got: {:?}",
        names
    );

    drop(server);
}

// ---------------------------------------------------------------------
// Test 3: server version listing (multiple versions of the same skill)
// ---------------------------------------------------------------------
#[test]
#[serial]
fn test_server_versions_listing() {
    let server = TestServer::spawn();
    let env = TestEnv::new(&server.base_url);

    // Use a temp copy of the example skill so we can republish under a
    // different version without mutating the repo fixture.
    let scratch = tempfile::tempdir().expect("scratch dir");
    let versioned = scratch.path().join("versioned-skill");
    copy_dir_recursive(&example_skill_path(), &versioned).expect("copy fixture");
    // Rewrite the `name` field to match the directory name (and the
    // URL we'll query). The example fixture's skill.yaml names itself
    // `escalation-handling`; we want `versioned-skill` for this test.
    rewrite_name(&versioned, "versioned-skill");

    // First publish: v1.0.0
    write_version(&versioned, "1.0.0");
    let mut publish = env.usk_cmd();
    publish.arg("publish").arg(&versioned);
    let (_, err, code) = run_cli(&mut publish);
    assert_eq!(code, 0, "publish v1.0.0 failed: {}", err);

    // Bump version and publish again
    write_version(&versioned, "1.1.0");
    let mut publish2 = env.usk_cmd();
    publish2.arg("publish").arg(&versioned);
    let (_, err, code) = run_cli(&mut publish2);
    assert_eq!(code, 0, "publish v1.1.0 failed: {}", err);

    // Give the server a moment to reload its index from disk.
    std::thread::sleep(Duration::from_millis(200));

    // Direct HTTP: list versions
    let client = server.client();
    let resp = client
        .get(format!(
            "{}/api/v1/packages/versioned-skill/versions",
            server.base_url
        ))
        .send()
        .expect("HTTP request");
    assert!(
        resp.status().is_success(),
        "versions HTTP failed: {}",
        resp.status()
    );
    let body: Value = resp.json().expect("parse JSON");
    let versions = body["versions"].as_array().expect("versions array");
    let vs: Vec<String> = versions
        .iter()
        .filter_map(|v| v.as_str().map(|s| s.to_string()))
        .collect();
    assert!(
        vs.iter().any(|v| v == "1.0.0"),
        "expected v1.0.0 in versions, got: {:?}",
        vs
    );
    assert!(
        vs.iter().any(|v| v == "1.1.0"),
        "expected v1.1.0 in versions, got: {:?}",
        vs
    );

    drop(server);
}

// ---------------------------------------------------------------------
// Test 4: path traversal rejection on the publish endpoint
// ---------------------------------------------------------------------
#[test]
#[serial]
fn test_server_rejects_path_traversal_on_publish() {
    let server = TestServer::spawn();

    // Build a tarball with a `../malicious` entry.
    let payload = build_tarball(&[("../malicious", b"pwned")]);

    let client = server.client();
    let resp = client
        .post(format!("{}/api/v1/publish", server.base_url))
        .header("Content-Type", "application/gzip")
        .header("X-Skill-Name", "evil-skill")
        .header("X-Skill-Version", "1.0.0")
        .body(payload)
        .send()
        .expect("HTTP publish");

    assert_eq!(
        resp.status().as_u16(),
        400,
        "expected 400 BAD_REQUEST for path traversal, got {}",
        resp.status()
    );

    // The malicious entry must NOT have been written into the
    // registry. The server may have created the skill version dir
    // before validation, but the traversal payload itself must not
    // have been extracted.
    let registry = server.registry_dir.path();
    let malicious_in_dest = registry.join("evil-skill").join("1.0.0").join("malicious");
    assert!(
        !malicious_in_dest.exists(),
        "path traversal wrote the malicious file: {:?}",
        malicious_in_dest
    );
    // And critically: nothing should have escaped the registry path.
    let outside = registry.parent().unwrap().join("malicious");
    assert!(
        !outside.exists(),
        "path traversal escaped the registry path: {:?}",
        outside
    );

    drop(server);
}

// ------------------- helpers ----------------------------------------

fn write_version(skill_dir: &std::path::Path, version: &str) {
    let yaml = std::fs::read_to_string(skill_dir.join("skill.yaml")).expect("read skill.yaml");
    // Naive but sufficient: the example fixture has `version: 1.0.0` on
    // its own line. Replace the first occurrence.
    let mut replaced = false;
    let mut updated: String = yaml
        .lines()
        .map(|line| {
            if !replaced && line.trim_start().starts_with("version:") {
                replaced = true;
                format!("version: {}", version)
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    // Preserve trailing newline if present in original
    if yaml.ends_with('\n') {
        updated.push('\n');
    }
    std::fs::write(skill_dir.join("skill.yaml"), updated).expect("write skill.yaml");
}

fn rewrite_name(skill_dir: &std::path::Path, new_name: &str) {
    let yaml = std::fs::read_to_string(skill_dir.join("skill.yaml")).expect("read skill.yaml");
    let mut replaced = false;
    let mut updated: String = yaml
        .lines()
        .map(|line| {
            if !replaced && line.trim_start().starts_with("name:") {
                replaced = true;
                format!("name: {}", new_name)
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    if yaml.ends_with('\n') {
        updated.push('\n');
    }
    std::fs::write(skill_dir.join("skill.yaml"), updated).expect("write skill.yaml");
}

fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if file_type.is_dir() {
            copy_dir_recursive(&from, &to)?;
        } else if file_type.is_file() {
            std::fs::copy(&from, &to)?;
        }
    }
    Ok(())
}
