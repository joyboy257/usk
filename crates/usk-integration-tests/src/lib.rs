//! Shared test helpers for the `usk-integration-tests` crate.
//!
//! These helpers spin up an isolated `usk-server` instance backed by a
//! temporary registry directory and run the `usk` CLI with a temporary
//! `USK_CONFIG_DIR`. Because the server binary is hardcoded to bind on
//! `0.0.0.0:8080`, all tests in this crate that need a live server use
//! `#[serial_test::serial]` to avoid port collisions.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use usk_core::config::Config;

/// Default base URL the server is hardcoded to listen on.
pub const SERVER_BASE_URL: &str = "http://localhost:8080";

/// Resolve the absolute path to a prebuilt workspace binary
/// (e.g. `usk-server`, `usk-cli`). We deliberately avoid `cargo run`
/// here because it forces a fresh compile on every test invocation,
/// blowing the 30s suite budget.
pub fn locate_binary(name: &str) -> PathBuf {
    // `CARGO_MANIFEST_DIR` = crates/usk-integration-tests
    // The workspace's `target/` lives two levels up.
    let workspace_target = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .join("target/debug")
        .join(name);

    if workspace_target.exists() {
        return workspace_target;
    }

    // Fallback: try the current target dir relative to the test binary.
    // `OUT_DIR` / `PROFILE` / `CARGO_BIN_EXE_<name>` are set by cargo for
    // integration tests too.
    if let Some(path) = option_env!("CARGO_BIN_EXE_usk-cli") {
        // Same directory as the CLI binary holds usk-server.
        if let Some(dir) = Path::new(path).parent() {
            let candidate = dir.join(name);
            if candidate.exists() {
                return candidate;
            }
        }
    }

    panic!(
        "could not find prebuilt binary {:?}; expected at {:?}",
        name, workspace_target
    );
}

/// A live `usk-server` instance, backed by a temp registry directory.
pub struct TestServer {
    pub registry_dir: tempfile::TempDir,
    pub child: Child,
    pub base_url: String,
}

impl TestServer {
    /// Spawn the server binary against a fresh temp registry directory and
    /// wait until it answers HTTP requests.
    pub fn spawn() -> Self {
        Self::spawn_with_env(Vec::new())
    }

    /// Spawn the server binary with additional env vars applied.
    pub fn spawn_with_env(extra_env: Vec<(String, String)>) -> Self {
        let registry_dir = tempfile::tempdir().expect("create temp registry dir");

        let bin = locate_binary("usk-server");
        let mut cmd = Command::new(&bin);
        cmd.env("USK_REGISTRY_PATH", registry_dir.path());
        cmd.stdout(Stdio::null());
        cmd.stderr(Stdio::piped());
        for (k, v) in &extra_env {
            cmd.env(k, v);
        }

        let child = cmd
            .spawn()
            .unwrap_or_else(|e| panic!("failed to spawn {:?}: {}", bin, e));

        let server = TestServer {
            registry_dir,
            child,
            base_url: SERVER_BASE_URL.to_string(),
        };
        server.wait_until_ready(Duration::from_secs(10));
        server
    }

    /// Poll the server's health endpoint until it answers or timeout.
    fn wait_until_ready(&self, timeout: Duration) {
        let client = reqwest::blocking::Client::new();
        let start = Instant::now();
        let url = format!("{}/api/v1/search?q=ready", self.base_url);
        while start.elapsed() < timeout {
            if let Ok(resp) = client.get(&url).send() {
                let status = resp.status();
                if status.is_success() {
                    return;
                }
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        panic!(
            "usk-server did not become ready within {:?} at {}",
            timeout, self.base_url
        );
    }

    /// Build a `reqwest::blocking::Client` for direct HTTP assertions.
    pub fn client(&self) -> reqwest::blocking::Client {
        reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .expect("build reqwest client")
    }

    /// Kill the server process. Idempotent.
    pub fn shutdown(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// An isolated CLI test environment: a temp `USK_CONFIG_DIR` with a
/// pre-populated `config.toml` pointing at the test server.
pub struct TestEnv {
    pub config_dir: tempfile::TempDir,
    pub install_dir: PathBuf,
}

impl TestEnv {
    /// Create the tempdirs and write a fresh `config.toml` that points at
    /// the given registry URL.
    pub fn new(registry_url: &str) -> Self {
        let config_dir = tempfile::tempdir().expect("create temp config dir");
        let install_dir = config_dir.path().join("skills");
        std::fs::create_dir_all(&install_dir).expect("create install dir");

        // Use the actual `usk_core::config::Config` struct so the
        // shape (HashMap vs Vec, field names, types) matches what
        // `Config::load()` deserializes.
        let mut harnesses = std::collections::HashMap::new();
        harnesses.insert("claude-code".to_string(), "usk-harness-claude".to_string());
        harnesses.insert("codex-cli".to_string(), "usk-harness-codex".to_string());

        let cfg = Config {
            registry_url: registry_url.to_string(),
            install_dir: install_dir.clone(),
            harnesses,
            installed: std::collections::HashMap::new(),
        };
        let toml_str = toml::to_string_pretty(&cfg).expect("serialize config");
        std::fs::write(config_dir.path().join("config.toml"), toml_str)
            .expect("write config.toml");

        TestEnv {
            config_dir,
            install_dir,
        }
    }

    /// Build a `Command` for the `usk` CLI with `USK_CONFIG_DIR` set to
    /// this env's tempdir.
    pub fn usk_cmd(&self) -> Command {
        let bin = locate_binary("usk-cli");
        let mut cmd = Command::new(&bin);
        cmd.env("USK_CONFIG_DIR", self.config_dir.path());
        // Make sure we never inherit a stray USK_CONFIG_DIR from a parent
        // process that might have been set during development.
        cmd.env_remove("USK_REGISTRY_PATH");
        cmd
    }
}

/// Run the CLI and return `(stdout, stderr, exit_code)`.
pub fn run_cli(cmd: &mut Command) -> (String, String, i32) {
    let out = cmd.output().expect("run usk CLI");
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    let code = out.status.code().unwrap_or(-1);
    (stdout, stderr, code)
}

/// Wait for the file at `path` to exist. Used for eventually-consistent
/// filesystem state after process spawn/teardown.
pub fn wait_for_path(path: &Path, timeout: Duration) -> bool {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if path.exists() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    false
}

/// Build a gzipped tarball in memory from `(path, content)` entries.
///
/// `paths` may contain `..` segments (e.g. `"../malicious"`) — this
/// helper builds the tar header bytes directly, bypassing the `tar`
/// crate's safety check on `set_path`. We use this to construct
/// malicious archives for security tests.
pub fn build_tarball(entries: &[(&str, &[u8])]) -> Vec<u8> {
    use std::io::Write;

    let mut tar_bytes = Vec::new();
    for (path, content) in entries {
        // Build a raw USTAR header so we can put `..` segments into
        // the path. The `tar` crate's `set_path` rejects `..` but the
        // server's `validate_tarball_safety` is the component we want
        // to exercise here, so we have to bypass that safety net.
        let mut header = [0u8; 512];
        // name: 100 bytes, null-terminated
        let name_bytes = path.as_bytes();
        let copy_len = name_bytes.len().min(100);
        header[..copy_len].copy_from_slice(&name_bytes[..copy_len]);
        // mode: "0000644\0" (regular file, rw-r--r--)
        let mode = b"0000644\0";
        header[100..108].copy_from_slice(mode);
        // uid: "0000000\0"
        header[108..116].copy_from_slice(b"0000000\0");
        // gid: "0000000\0"
        header[116..124].copy_from_slice(b"0000000\0");
        // size: 11 octal digits + null
        let size_str = format!("{:011o}\0", content.len());
        header[124..136].copy_from_slice(size_str.as_bytes());
        // mtime: "00000000000\0" (12 octal digits)
        header[136..148].copy_from_slice(b"00000000000\0");
        // typeflag: '0' (regular file)
        header[148] = b'0';
        // magic: "ustar\0" + "00"
        header[257..263].copy_from_slice(b"ustar\0");
        header[263..265].copy_from_slice(b"00");

        // chksum: sum of all bytes treating chksum field as spaces
        let mut chksum: u32 = 0;
        for (i, b) in header.iter().enumerate() {
            if (148..156).contains(&i) {
                chksum += b' ' as u32;
            } else {
                chksum += *b as u32;
            }
        }
        let chksum_str = format!("{:06o}\0 ", chksum);
        header[148..156].copy_from_slice(chksum_str.as_bytes());

        tar_bytes.write_all(&header).expect("write header");
        tar_bytes.write_all(content).expect("write content");
        // Pad to 512-byte boundary
        let pad = (512 - (content.len() % 512)) % 512;
        tar_bytes.resize(tar_bytes.len() + pad, 0);
    }
    // End-of-archive: two zero blocks
    tar_bytes.resize(tar_bytes.len() + 1024, 0);

    let mut gz_bytes = Vec::new();
    {
        let mut encoder =
            flate2::write::GzEncoder::new(&mut gz_bytes, flate2::Compression::default());
        encoder.write_all(&tar_bytes).expect("write to gzip");
        encoder.finish().expect("finish gzip");
    }
    gz_bytes
}
