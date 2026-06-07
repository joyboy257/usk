# Contributing to USK

Thanks for your interest in the Universal Skills Library. Contributions of all sizes are welcome — bug reports, new harness adapters, documentation, example skills, and tests.

## Code of Conduct

This project follows the [Contributor Covenant v2.1](CODE_OF_CONDUCT.md). By participating, you agree to uphold it.

## Prerequisites

- **Rust 1.75+** (install via [rustup](https://rustup.rs))
- A C toolchain (for `git2`'s libgit2 bindings)
- `git`

## Setup

Fork the repository on GitHub, then clone your fork:

```bash
git clone https://github.com/<your-username>/usk
cd usk
cargo build --workspace
cargo test --workspace
```

## Running the server

```bash
USK_REGISTRY_PATH=/tmp/usk-registry cargo run --bin usk-server
# Listens on 0.0.0.0:8080
```

## Running the CLI

```bash
# Build once
cargo build --bin usk-cli

# Run a command
cargo run --bin usk-cli -- search escalation
cargo run --bin usk-cli -- install escalation-handling --harness claude-code
```

## Adding a new harness adapter

USK ships with a `HarnessAdapter` trait that any framework can implement. The full walkthrough is in [`docs/DEVELOPMENT.md`](docs/DEVELOPMENT.md). Short version:

1. Create `crates/usk-harness-<name>/` with a `Cargo.toml` and `src/converter.rs`.
2. Implement `HarnessAdapter` from `usk-harness-core`.
3. Register the adapter in `usk-harness-core::discovery::KNOWN_HARNESSES`.
4. Add a fixture under `spec/examples/<your-skill>/` if relevant.
5. Add the crate to the workspace `members` in the root `Cargo.toml`.
6. Run `cargo test --workspace`.

## Code style

- Format with `cargo fmt --all` before committing.
- Lint with `cargo clippy --workspace -- -D warnings`.
- We do not enable `clippy::pedantic`. Standard `cargo clippy` warnings are blocking in CI.
- Document all public API with `///` doc comments.
- Prefer small, focused commits. Squash noisy WIP commits before review.

## Pull request process

1. Branch from `main`: `git checkout -b fix/short-description`.
2. Keep changes focused. One concern per PR.
3. Add or update tests for any behavior change. The bar is "coverage is no worse than before."
4. Update `CHANGELOG.md` under an `## [Unreleased]` section describing your change in user-facing terms.
5. Run the full suite locally: `cargo build --workspace && cargo test --workspace && cargo clippy --workspace -- -D warnings && cargo fmt --all -- --check`.
6. Open a PR using the [PR template](.github/PULL_REQUEST_TEMPLATE.md). Link any related issue with `Closes #123` or `Refs #123`.
7. CI must pass. A maintainer will review; expect discussion on design, not just style.
8. Squash-merge is the default. Your commits will be condensed into one on `main`.

## Reporting bugs

Use the [bug report template](.github/ISSUE_TEMPLATE/bug_report.md). Include:
- What you did (commands, OS, Rust version)
- What you expected
- What actually happened
- Minimal reproduction (a skill directory, a command, a curl request)

## Suggesting features

Use the [feature request template](.github/ISSUE_TEMPLATE/feature_request.md). Describe the problem you are trying to solve before proposing a solution.

## License

By contributing, you agree that your contributions will be licensed under the [MIT License](LICENSE).
