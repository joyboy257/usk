## Description

<!-- What does this PR do? Why? Link any design notes, prior discussion, or related issues. -->

## Related issue

<!-- Link with `Closes #123` or `Refs #123`. If there is no issue, explain why one isn't needed. -->

## Type of change

<!-- Check all that apply. -->

- [ ] Bug fix (non-breaking change that fixes an issue)
- [ ] New feature (non-breaking change that adds functionality)
- [ ] Breaking change (fix or feature that would cause existing functionality to change)
- [ ] Documentation only
- [ ] Refactor (no behavior change)

## How tested

<!-- How did you verify the change? Be specific. -->

- [ ] `cargo build --workspace` passes
- [ ] `cargo test --workspace` passes
- [ ] `cargo clippy --workspace -- -D warnings` passes
- [ ] `cargo fmt --all -- --check` passes
- [ ] New unit tests added (if behavior changed)
- [ ] New integration test added (if end-to-end behavior changed)
- [ ] Manually exercised the affected CLI command(s)

## Checklist

- [ ] My code follows the project's style (`cargo fmt`, no `clippy::pedantic`)
- [ ] I have added `///` doc comments to new public API
- [ ] I have updated `CHANGELOG.md` under `[Unreleased]`
- [ ] I have updated relevant docs in `README.md` / `docs/`
- [ ] I have not introduced new compiler warnings
- [ ] I have read [`CONTRIBUTING.md`](../CONTRIBUTING.md) and the [`CODE_OF_CONDUCT.md`](../CODE_OF_CONDUCT.md)
