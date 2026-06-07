# usk — Universal Skills Library

> A harness-agnostic registry for reusable AI agent skills.

[![CI](https://img.shields.io/github/actions/workflow/status/YOUR-USERNAME/usk/ci.yml?branch=main&style=flat-square)](https://github.com/YOUR-USERNAME/usk/actions)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg?style=flat-square)](LICENSE)
[![Crates.io](https://img.shields.io/crates/v/usk-core.svg?style=flat-square)](https://crates.io/crates/usk-core)

## What is USK?

USK is a registry, schema, and toolchain for **Universal Skills** — reusable packages of instructions, examples, templates, scripts, and reference material that teach an AI agent a specific method of working. Skills are authored once in a harness-agnostic format and converted to any supported agent framework via pluggable adapters.

## Why?

AI agent frameworks are multiplying. Without a shared skills layer, every team rewrites the same escalation handling, sales prep, or code review checklist for Claude Code, Codex CLI, and whatever comes next. USK fixes that:

- **One authoring format** — write `SKILL.md` once.
- **Many harnesses** — adapters convert the same skill to Claude Code, Codex CLI, etc.
- **A registry** — publish, search, version, and install skills with one tool.
- **Repeatable methods** — capture the way your team works, not just what they know.

## Supported Harnesses

| Harness | Adapter crate | Output format |
|---------|---------------|---------------|
| Claude Code | [`usk-harness-claude`](crates/usk-harness-claude) | `SKILL.md` directory |
| Codex CLI | [`usk-harness-codex`](crates/usk-harness-codex) | `agent.yaml` |

## Quick Start

```bash
# Install the CLI
cargo install --path crates/usk-cli

# Create a new skill from a template
usk new my-escalation-skill

# Search the local index
usk search escalation

# Install a skill for Claude Code
usk install escalation-handling --harness claude-code
```

## Anatomy of a Skill

A skill is a directory with a manifest and an instruction body:

```
my-skill/
├── skill.yaml             # Metadata + manifest (REQUIRED)
├── SKILL.md               # Main instruction document (REQUIRED)
├── instructions/          # Sub-step instructions (optional)
├── examples/              # Example inputs/outputs (optional)
├── templates/             # Output templates (optional)
├── scripts/               # Executable scripts (optional)
└── references/            # Reference documentation (optional)
```

See [`spec/SKILL_SPEC.md`](spec/SKILL_SPEC.md) for the full schema and the [example skills](spec/examples/).

## CLI Reference

| Command | Description |
|---------|-------------|
| `usk new <name>` | Scaffold a new skill from a template |
| `usk publish [path]` | Publish a skill to the local registry |
| `usk search <query>` | Search the registry by name, tag, or description |
| `usk install <name> [--harness <name>]` | Install a skill for a specific harness |
| `usk list` | List installed skills |
| `usk update [name]` | Update installed skills (defaults to all) |
| `usk outdated` | List skills with available updates |
| `usk harness add\|remove\|list` | Manage registered harnesses |

## Server

The `usk-server` is an axum-based registry server with a git audit trail. Run it locally:

```bash
cargo run --bin usk-server
# Server listening on 0.0.0.0:8080
```

Configuration:

| Env var | Default | Description |
|---------|---------|-------------|
| `USK_REGISTRY_PATH` | `./registry` | Filesystem path for the on-disk registry |
| `RUST_LOG` | `info` | Tracing filter directive |

The CLI reads its registry URL from `~/.usk/config.toml` (override via `USK_CONFIG_DIR`). See [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) for the API surface and data flow.

## Project Layout

```
usk/
├── Cargo.toml             # Workspace manifest
├── README.md
├── LICENSE
├── spec/                  # Universal skill format spec
│   ├── SKILL_SPEC.md
│   └── examples/          # escalation-handling, sales-call-prep
├── crates/
│   ├── usk-core/          # Schema, validation, parser, search index
│   ├── usk-harness-core/  # HarnessAdapter trait + discovery
│   ├── usk-harness-claude/
│   ├── usk-harness-codex/
│   ├── usk-cli/           # `usk` binary
│   ├── usk-server/        # `usk-server` binary
│   └── usk-integration-tests/
└── docs/
    ├── ARCHITECTURE.md
    └── DEVELOPMENT.md
```

## Contributing

We welcome contributions — new harness adapters, bug fixes, docs, and example skills. See [`CONTRIBUTING.md`](CONTRIBUTING.md) for setup, code style, and the PR process.

## Security

Report vulnerabilities per [`SECURITY.md`](SECURITY.md). Please do not file public issues for security bugs.

## License

MIT — see [`LICENSE`](LICENSE).
