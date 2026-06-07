---
name: Bug report
about: Report broken behavior in USK
title: "[bug] "
labels: ["bug", "triage"]
assignees: []
---

## Description

A clear, concise description of what the bug is.

## Reproduction

Minimal steps to reproduce. If applicable, include:

- A sample skill directory (`skill.yaml`, `SKILL.md`)
- The exact `usk` / `usk-server` commands you ran
- The expected output

```bash
# Example
usk install my-skill --harness claude-code
```

## Expected behavior

What you expected to happen.

## Actual behavior

What actually happened. Include the full error message and, if useful, a stack trace.

```
(paste error here)
```

## Environment

- USK version: (run `usk --version` or check `Cargo.toml`)
- OS: (e.g., macOS 14.4, Ubuntu 24.04, Windows 11)
- Rust version: (run `rustc --version`)
- Harness involved: (e.g., claude-code, codex-cli, or N/A)
- Installation method: (cargo install, source build, release binary)

## Additional context

Any other information — logs (`RUST_LOG=usk=debug`), related issues, workarounds you've found.
