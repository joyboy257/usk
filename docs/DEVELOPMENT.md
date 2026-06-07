# Development

This guide covers day-to-day development: environment, testing, debugging, and how to extend USK with new adapters and CLI commands.

## Dev environment

- Rust 1.75 or newer (`rustup default stable`)
- A C toolchain — `usk-server` uses `git2` which links to libgit2; on macOS this is automatic, on Linux install `build-essential` and `cmake`, on Windows install the Visual Studio Build Tools.
- `git` for the registry audit trail.

Clone and build:

```bash
git clone https://github.com/<your-username>/usk
cd usk
cargo build --workspace
```

## Running tests

```bash
# All tests, all crates
cargo test --workspace

# A single crate
cargo test -p usk-core

# A single test by name
cargo test -p usk-integration-tests publish_round_trip

# With logging
RUST_LOG=usk=debug cargo test --workspace -- --nocapture
```

The integration tests in `crates/usk-integration-tests/` are end-to-end. They build a temporary registry, start a server, and exercise the full publish/search/install pipeline.

## Running the server

```bash
# Default: registry at ./registry, listening on 0.0.0.0:8080
cargo run --bin usk-server

# Custom registry path
USK_REGISTRY_PATH=/tmp/usk-registry cargo run --bin usk-server

# Verbose tracing
RUST_LOG=usk=debug,usk_server=trace cargo run --bin usk-server
```

## Running the CLI

The CLI talks to a running `usk-server` by default. To run against a local server on port 8080, point your config at it:

```toml
# ~/.usk/config.toml
registry_url = "http://localhost:8080"
```

Then:

```bash
cargo run --bin usk-cli -- search escalation
cargo run --bin usk-cli -- install escalation-handling --harness claude-code
cargo run --bin usk-cli -- harness list
```

## Debugging tips

```bash
# Trace every USK call
RUST_LOG=usk=trace cargo run --bin usk-cli -- install foo

# Trace a specific crate only
RUST_LOG=usk_server=debug cargo run --bin usk-server

# Backtrace on panic
RUST_BACKTRACE=1 cargo test -p usk-server

# Check formatting
cargo fmt --all -- --check

# Lint
cargo clippy --workspace -- -D warnings
```

## Adding a new harness adapter

The `HarnessAdapter` trait is the contract between USK and a specific agent framework. The full plan:

1. **Create the crate.** Add `crates/usk-harness-<name>/` with a `Cargo.toml` and `src/converter.rs`. Use the existing `usk-harness-claude` or `usk-harness-codex` crates as a template.

2. **Implement the trait.** The trait is in `usk-harness-core::adapter`. At minimum:
   - `name()` returning the harness identifier (kebab-case, e.g. `claude-code`).
   - `convert(skill, dest) -> Result<()>` writing framework-specific output to `dest`.
   - `default_install_dir()` returning where `usk install --harness <name>` should place files.

3. **Register the adapter.** Add an entry to `usk-harness-core::discovery::KNOWN_HARNESSES` with a constructor closure. This is what `usk harness list` and the install pipeline use to find your adapter.

4. **Add a fixture.** Drop a representative skill under `spec/examples/<your-harness-test>/` so the integration tests have something concrete to convert.

5. **Wire the workspace.** Add the new crate to `members` in the root `Cargo.toml`. Add it as a dependency of `usk-cli`.

6. **Test.** Add a unit test in your adapter crate that converts `spec/examples/escalation-handling` and asserts on the output structure. Then add a case to `usk-integration-tests` if your harness is part of the default install path.

7. **Document.** Update `spec/SKILL_SPEC.md`'s "Harness Compatibility Matrix" table so users can find your adapter.

## Adding a new CLI subcommand

CLI subcommands live in `crates/usk-cli/src/commands/`. The `main.rs` file uses `clap` to define the top-level command tree.

1. Add a new module under `crates/usk-cli/src/commands/<name>.rs`.
2. Define a `pub struct Args` with `#[derive(Args)]` from clap.
3. Implement `pub async fn run(args: Args, ctx: &Context) -> Result<()>`.
4. Register the variant in the top-level `Command` enum in `main.rs` and dispatch to your `run`.

Keep handlers thin — they parse arguments, call into `usk-core` / `usk-harness-core`, and format output. Business logic belongs in the library crates so it can be tested without the CLI plumbing.

## Releasing

The release workflow is triggered by a `v*` tag. See [`.github/workflows/release.yml`](../.github/workflows/release.yml). A maintainer runs:

```bash
# Bump version in workspace.package
$EDITOR Cargo.toml

# Update CHANGELOG.md

git commit -am "Release 0.2.0"
git tag v0.2.0
git push origin main --tags
```

The CI builds release binaries for linux/macos/windows on x86_64 and aarch64 and attaches them to a GitHub release.
