# Architecture

This document describes how the USK workspace fits together at the system level. For day-to-day development, see [`DEVELOPMENT.md`](DEVELOPMENT.md).

## High-level system

```
                          +-----------------+
                          |   usk CLI       |
                          |  (usk binary)   |
                          +--------+--------+
                                   |
                                   v
                          +-----------------+
                          |  usk-server     |     +-----------------+
                          |  (axum, 8080)   |<--->|  On-disk        |
                          +--------+--------+     |  registry + git |
                                   |              +-----------------+
                                   v
                          +-----------------+
                          |  usk-core       |
                          |  schema, parse, |
                          |  index, config  |
                          +--------+--------
                                   |
                +------------------+------------------+
                v                  v                  v
        +---------------+   +---------------+   +---------------+
        | harness-core  |   | harness-      |   | harness-      |
        |  trait +      |   |  claude       |   |  codex        |
        |  discovery    |   |               |   |               |
        +---------------+   +---------------+   +---------------+
```

The CLI is the user-facing tool. The server is the registry backend. Both share `usk-core` for schema, validation, and search. Adapters are pluggable and registered through `usk-harness-core::discovery`.

## Crate dependency graph

```
usk-cli             usk-server
    |                   |
    +---+---------+-----+
        |         |
        v         v
  usk-harness-core  usk-core
        |
        +-------+-------+
                |       |
                v       v
   usk-harness-claude   usk-harness-codex
                |       |
                +---+---+
                    v
                usk-core

usk-integration-tests -> usk-core, usk-server, usk-cli
```

- `usk-core` is the leaf crate with no internal deps.
- `usk-harness-{claude,codex}` depend on `usk-core` and `usk-harness-core`.
- `usk-cli` and `usk-server` are top-level binaries that compose the rest.
- `usk-integration-tests` is a test-only crate and is `publish = false`.

## Skill file format

The canonical schema is in [`../spec/SKILL_SPEC.md`](../spec/SKILL_SPEC.md). In summary:

- Every skill is a directory with a `skill.yaml` manifest and a `SKILL.md` body.
- Optional subdirectories carry examples, templates, scripts, and references.
- The manifest declares harness compatibility with semver ranges.

## Adapter contract

`usk-harness-core` defines the `HarnessAdapter` trait that every framework integration must implement. Adapters are stateless and synchronous; they receive a parsed `Skill` plus a destination path, and produce framework-specific output.

Discovery is centralized in `usk-harness-core::discovery::KNOWN_HARNESSES` — adding a new harness means registering it there with a name and a constructor.

## Server architecture

`usk-server` is a single-process axum application. The main components:

- **Routes** — versioned HTTP handlers for `publish`, `search`, `install`, `list`, `update`, `outdated`.
- **Per-(name, version) lock** — an in-process mutex map serializes concurrent publishes to the same skill version, preventing torn tarballs and git corruption.
- **Git audit trail** — every accepted publish creates a git commit in the on-disk registry, providing an immutable history.
- **Search index** — backed by `usk-core`'s in-memory index; rebuilt from the registry on startup and updated on each publish.

## Data flow

```
author                  registry                    consumer
  |                        |                            |
  |  usk publish ./my-skill |                            |
  |----------------------->|                            |
  |                        | validate, lock, extract,   |
  |                        | git commit, update index    |
  |<-----------------------|                            |
  |                        |                            |
  |                        |  usk search escalation      |
  |                        |<---------------------------|
  |                        |----------------------->    |
  |                        |                            |
  |                        |  usk install esc --harness  |
  |                        |  claude-code                |
  |                        |<---------------------------|
  |                        |  fetch tarball, run         |
  |                        |  harness adapter to target  |
  |                        |  install directory          |
  |                        |----------------------->    |
```

## Storage layout

The server stores each skill under its registry root (default `./registry`):

```
registry/
├── index.json
├── git history (.git/)
├── escalation-handling/
│   ├── 1.0.0/
│   │   ├── skill.yaml
│   │   ├── SKILL.md
│   │   ├── instructions/
│   │   ├── examples/
│   │   └── ...
│   └── 1.1.0/
│       └── ...
└── sales-call-prep/
    └── 0.2.0/
        └── ...
```

Versions are immutable once published. A new version always creates a sibling directory.
