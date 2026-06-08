# usk — Universal Skills Library

> A harness-agnostic registry for reusable AI agent skills.

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

Install `usk` with a one-liner, then drop a skill folder in and install it — no registry server required.

```bash
# 1. Install the CLI (no Rust toolchain needed for consumers)
curl -fsSL https://usk.dev/install.sh | sh

# 2. Install a skill from a local directory for Claude Code
usk install ./spec/examples/escalation-handling --harness claude-code

# Files land at ~/.claude/skills/escalation-handling/ — exactly where
# Claude Code reads from. No staging dir, no manual copy.
```

The same flow works for URLs (HTTP archive or git clone) and for a
self-hosted registry when you have one. `usk install <name>` (registry
mode) requires `registry_url` to be configured in `~/.usk/config.toml`
— see [Self-hosted registry (optional)](#self-hosted-registry-optional).
See [CLI Reference](#cli-reference) for the full surface.

### Authoring

```bash
# Create a new skill from the default scaffold
usk new my-skill

# Inspect / validate / preview conversion before publishing
usk inspect ./my-skill
usk validate ./my-skill
usk convert ./my-skill --harness claude-code --out /tmp/preview
```

### Self-hosting the registry (optional)

`usk` works without a registry — the local-first install path in
[Quick Start](#quick-start) is the default and does not need a server.
A registry is only useful if you want versioned, shared skills across
a team. `usk install <name>` (registry mode) requires `registry_url`
to be configured in `~/.usk/config.toml`.

```bash
cargo run --bin usk-server
# Server listening on 0.0.0.0:8080

# Configure the CLI to point at the local registry
usk config set registry_url http://localhost:8080
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
| `usk install <name-or-path-or-url> [--harness <name>] [--target <path>]` | Install a skill from the registry, a local directory, or a URL |
| `usk list` | List installed skills |
| `usk update [name]` | Update installed skills (defaults to all) |
| `usk outdated` | List skills with available updates |
| `usk harness add\|remove\|list` | Manage registered harnesses |

## Self-hosted registry (optional)

If you have a private registry or want to share skills within a team, USK
includes a self-hostable server. **Most users do not need this** — the
local-first install path in [Quick Start](#quick-start) covers the
central use case without running any server.

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

The CLI reads its registry URL from `~/.usk/config.toml` (override via
`USK_CONFIG_DIR`). The default config has an empty `registry_url`; the
`usk install <name>` (registry mode) flow errors with a clear message
if you have not set one. See [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md)
for the API surface and data flow.

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
