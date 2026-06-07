---
title: Make USK a Plug-and-Play Skill Registry
type: feat
status: active
date: 2026-06-07
---

# Make USK a Plug-and-Play Skill Registry

## Overview

USK currently works but requires significant setup: install Rust, build the CLI from source, start a registry server, configure the client, register a harness, then install. The "skills are folders" insight is honored in the spec but not in the CLI's API surface. This plan transforms USK into a plug-and-play tool: drop a skill folder in, install it, it works in your harness. No server required for local use, no Rust toolchain required for consumers, no config gymnastics.

The work is organized into 4 tiers with 12 implementation units, sequenced so each tier is independently shippable.

---

## Problem Frame

### The current layman journey (friction map)

A consumer trying to use a skill today:

1. Install Rust toolchain (1-3 min, OS-specific)
2. `cargo install --path crates/usk-cli` (2-10 min for cold compile)
3. Start a server somewhere: `cargo run --bin usk-server &`
4. Configure: `usk harness add claude-code` (step exists but with no discoverable "why")
5. Search and install: `usk search foo` then `usk install foo --harness claude-code`
6. Files land in `~/.usk/skills/claude-code/foo/` — a staging location that Claude Code does not read
7. User must manually symlink or copy into `~/.claude/skills/`

An author trying to share a skill today:

1. `usk new my-skill` makes a barebones scaffold
2. Edit `SKILL.md` (no preview of how it'll render in any harness)
3. `usk publish` requires the server from step 3 above
4. Validation only runs at publish time; no standalone `validate` command

### Why this matters

The architectural mismatch is that USK treats the **server** as the only source of truth, when in reality a skill is a **self-describing folder** that can be installed directly. The server is one valid distribution channel; it is not the *only* one. A proper plug-and-play model — like `npm install ./local-package` or `pip install ./local-wheel` — works with or without a registry.

---

## Requirements Trace

- **R1.** Consumer can install a skill from a local directory path with no registry running.
- **R2.** Consumer can install a skill from a URL (git clone, archive download) with no registry running.
- **R3.** Existing registry install flow (`usk install <name>`) continues to work unchanged.
- **R4.** `usk install` places skill files in the harness-specific location the harness actually reads (e.g. `~/.claude/skills/<name>/` for Claude Code), with the option to override.
- **R5.** A non-Rust consumer can install the `usk` CLI via a curl-able shell script.
- **R6.** First-time `usk` invocation detects missing setup and offers a guided `usk setup` wizard.
- **R7.** Author can run `usk inspect <path-or-name>` to see a skill's contents (files, sizes, parsed metadata) without unpacking or installing.
- **R8.** Author can run `usk validate <path>` to surface all validation errors without publishing.
- **R9.** Author can run `usk convert <path> --harness <h> --out <dir>` to preview the harness adapter output locally.
- **R10.** The default `registry_url` in `Config::default()` points to a public registry, not `http://localhost:8080`. Self-hosting remains supported via config override.
- **R11.** The server serves a minimal HTML UI at `GET /` that lists skills, with `GET /skills/{name}` rendering the skill's `SKILL.md` as markdown.
- **R12.** A skill that declares `requires: [other-skill]` causes the required skills to be installed transitively (with cycle detection).
- **R13.** A directory containing multiple skill folders can be installed as a collection in one command.
- **R14.** A `usk.lock` file (TOML) records exact installed versions per harness and is used for reproducible installs.

---

## Scope Boundaries

### In scope

- Local-first install (R1, R2, R3)
- Harness path auto-detection (R4)
- Install script (R5)
- First-run setup wizard (R6)
- Authoring commands: `inspect`, `validate`, `convert`, `read` (R7, R8, R9)
- Public default registry URL (R10)
- Minimal server web UI (R11)
- `requires` field honored (R12)
- Skill collections (R13)
- `usk.lock` for reproducibility (R14)

### Out of scope

- Skill marketplace, ratings, reviews
- Payments, paid skills, licensing enforcement
- Cryptographic signing of skills (future work; the immutable-version 409 already provides some protection)
- Editor integrations (VS Code, JetBrains)
- Telemetry or analytics
- Multi-user collaborative skill editing
- Web UI for skill authoring (separate from browse/preview)

### Deferred to follow-up work

- **Skill templates**: `usk new --template=checklist` would let authors start from a known-good scaffold. Worth doing once the basic `new` flow is validated, but not blocking the plug-and-play story.
- **Web UI for authoring**: a browser-based SKILL.md editor. Separate from R11.
- **Harness config validation**: `usk doctor` to verify Claude Code can actually find installed skills.
- **Cross-platform path resolution**: use the `dirs` crate to handle Windows `%APPDATA%` correctly. Currently assumes Unix-style `~/.usk/`. Low priority since most AI agent development happens on macOS/Linux.

---

## Context & Research

### Relevant code and patterns

- `crates/usk-cli/src/main.rs`: the `commands::install` function (around line 173) is the main target for U1 and U2; it currently calls `client.get_package` + `client.download` and writes to `config.install_dir/<harness>/<name>/`. Will be modified to accept a local path or URL and write to the harness-native location.
- `crates/usk-harness-core/src/adapter.rs`: the `HarnessAdapter` trait has a `convert(skill, source_dir, output_dir)` method that is already the right primitive for U7 (`usk convert`). No change needed to the trait.
- `crates/usk-harness-core/src/discovery.rs`: the `KNOWN_HARNESSES` constant pattern. U1's harness-path map follows the same shape (a `const` in the harness-core crate).
- `crates/usk-core/src/validation.rs`: `validate(skill, base_dir)` already does name/semver/entry checks. U6 (`usk validate`) reuses this; deeper checks (instruction file references, example file existence) are added incrementally.
- `crates/usk-server/src/main.rs`: the axum server. U9 (web UI) adds new routes and a `pulldown-cmark` dependency.
- `crates/usk-core/src/config.rs`: `Config::default()` sets `registry_url` to `http://localhost:8080`. U8 changes this.
- `crates/usk-core/src/index.rs`: `RegistryIndex` has `get_latest`, `get_version`, `versions` for version-aware lookups. Used by U10's dependency resolution.

### Patterns to follow

- All existing commands are dispatched from `Cli::parse()` → `tokio::main` → `match cli.command`. New commands follow the same pattern.
- All async commands take `&RegistryClient` (when needed) and `&Config`. Local-only commands (U6, U7, U9) skip the client.
- The integration test crate at `crates/usk-integration-tests/` already spawns the server as a subprocess; U9's web UI tests can use the same pattern.
- Existing tests in `crates/usk-cli/src/main.rs` use `#[serial]` from `serial_test` for env-var-dependent tests. U1's harness-path detection tests follow the same pattern.

### External references

- `cargo` and `pip` are the most relevant installation UX analogs. Both support `install <path>`, `install <url>`, and `install <name>` (from registry) as three distinct install modes. USK's U1/U2 follow this tri-modal pattern.
- `npm install --save` writes to `package.json`; `cargo add` writes to `Cargo.toml`. U12 (`usk.lock`) is the equivalent for USK.
- The `dirs` crate (https://docs.rs/dirs/) is the standard cross-platform home/config path resolver on crates.io. Worth adopting in U4 if cross-platform becomes a real need, but deferred for now.

---

## Key Technical Decisions

### D1. Harness install paths are a hardcoded map, not a config field

**Decision:** The mapping from harness key (`claude-code`, `codex-cli`) to its install path (`~/.claude/skills/<name>/`, `~/.codex/agents/`) lives in `usk-harness-core` as a `const`, alongside `KNOWN_HARNESSES`.

**Rationale:** Keeping it in code means it's versioned with the harness adapter crate. If Claude Code's install location changes, the next release of `usk-harness-claude` updates the path. Putting it in `Config` would mean the user has to know to update their config when a harness changes — exactly the friction this plan removes.

**Override mechanism:** `usk install --target <path>` lets power users override per-install. The default is the harness-known path.

### D2. Local-first install reuses the existing `HarnessAdapter::convert`

**Decision:** `usk install <local-path>` parses `skill.yaml` from the path, validates it, then calls the same `HarnessAdapter::convert` that registry install uses. No new code path in the adapter layer.

**Rationale:** Avoids two code paths drifting apart. The local install is just a different *source* for the same conversion.

### D3. `usk.lock` lives in the project root, not `~/.usk/`

**Decision:** Lockfile lives at `<cwd>/usk.lock` for project-local skill collections. Global lockfile (for user-level installs) lives at `~/.usk/installed.lock`.

**Rationale:** A user with multiple projects wants per-project lockfiles, not a global one. Cargo's per-project `Cargo.lock` is the model.

### D4. The `requires` field uses simple name-based resolution, not semver ranges (v1)

**Decision:** `requires: [other-skill]` installs the latest version of `other-skill` from the configured registry. No semver range support in v1.

**Rationale:** Adds dependency resolution complexity that the current spec doesn't require. Simple name-based resolution is the npm v1 model and is enough for the "skill references a helper skill" use case.

### D5. The web UI is server-side rendered HTML, not a SPA

**Decision:** U9 uses `askama` or `maud` templates (TBD) for server-rendered HTML, with a small amount of vanilla JS for filtering. No React/Vue/SPA.

**Rationale:** Keeps the server binary small and the deploy story simple. A static HTML page that renders the index is enough for browse/discover. Power features (search-as-you-type, skill previews) can be added later with sprinkles of JS without a build step.

---

## Output Structure

New files and where they land:

```
crates/
  usk-harness-core/
    src/
      paths.rs                  # NEW: harness_key -> install_path map (D1)
  usk-core/
    src/
      lockfile.rs               # NEW: usk.lock parse/write (R14)
      resolver.rs               # NEW: requires resolution (R12)
  usk-cli/
    src/
      local_install.rs          # NEW: install from path/URL (R1, R2)
      setup.rs                  # NEW: usk setup wizard (R6)
      inspect.rs                # NEW: usk inspect (R7)
      validate_cmd.rs           # NEW: usk validate (R8) — note: name avoids clashing with usk_core::validation
      convert.rs                # NEW: usk convert (R9)
  usk-server/
    src/
      web.rs                    # NEW: HTML routes, markdown rendering (R11)
      templates/
        index.html              # NEW
        skill.html              # NEW
scripts/
  install.sh                    # NEW: curl-able install (R5)
docs/
  plans/
    2026-06-07-001-feat-plug-and-play-usk-plan.md   # NEW: this file
```

Modified files:

- `Cargo.toml`: add `pulldown-cmark` to workspace deps, add `maud` (or `askama`) if we go that route for U9
- `crates/usk-core/src/lib.rs`: re-export new modules
- `crates/usk-cli/src/main.rs`: new subcommands in the `Commands` enum, install flow split into local-vs-registry
- `crates/usk-server/src/main.rs`: wire `web` routes
- `crates/usk-harness-claude/src/converter.rs`: the `convert` method already does the right thing; U1's path map will reference `~/.claude/skills/<name>/` which is the same place Claude Code reads from
- `README.md`: quickstart rewritten to use the install script and the local-first flow
- `docs/DEVELOPMENT.md`: document the new commands

---

## Implementation Units

Units are grouped by tier. Each tier should land as one PR for reviewability.

### Tier 1 — Plug and play foundations

#### U1. Harness install path map

**Goal:** USK knows where each harness expects skills, so `usk install` writes to the right place without user configuration.

**Requirements:** R4

**Dependencies:** None (first unit)

**Files:**
- Create: `crates/usk-harness-core/src/paths.rs`
- Modify: `crates/usk-harness-core/src/lib.rs`
- Test: `crates/usk-harness-core/src/paths.rs` (unit tests inline)

**Approach:**
- Add a `pub fn default_install_path(harness_key: &str, skill_name: &str) -> Option<PathBuf>` to `usk-harness-core`.
- Hardcoded map: `claude-code` → `~/.claude/skills/<name>`, `codex-cli` → `~/.codex/agents/<name>.yaml` (verify Codex's actual path before landing; treat as a discovery task during implementation).
- `~/.` paths are resolved against the home directory at call time, not stored as literal strings.
- Returning `None` for unknown harnesses lets the caller fall back to the existing `config.install_dir` behavior.

**Patterns to follow:** The `discovery::KNOWN_HARNESSES` constant in the same crate.

**Test scenarios:**
- Happy path: `default_install_path("claude-code", "foo")` returns `<HOME>/.claude/skills/foo`.
- Edge case: when `HOME` is unset, returns a path with the literal `~` segment (caller can detect this and error).
- Edge case: unknown harness key returns `None`.
- Error path: skill name containing path traversal characters (`../foo`) is rejected or sanitized.

**Verification:** `cargo test -p usk-harness-core` passes; the new function is used by U2's install flow.

#### U2. Local-first install from path or URL

**Goal:** `usk install <local-path>` and `usk install <url>` work without a registry server.

**Requirements:** R1, R2, R3 (preserve existing registry flow), R4 (use U1's path map)

**Dependencies:** U1

**Files:**
- Create: `crates/usk-cli/src/local_install.rs`
- Modify: `crates/usk-cli/src/main.rs` (install command dispatch + flow split)

**Approach:**
- Detect the install source by argument shape: starts with `./` or `/` or `~` → local path; starts with `http://`, `https://`, or `git@` → URL; otherwise → registry name (current behavior).
- For local path: read `skill.yaml` directly, validate, then call the adapter's `convert` into the harness's install path.
- For URL: download to a tempdir, extract, then run the same flow as local path. Use `reqwest::get` for HTTP, `git2` (already a workspace dep) for git URLs, or shell out to `git clone` for git URLs (simpler).
- The `convert_to` step in the existing flow (in `usk-cli/src/main.rs`) takes a `skill_name` and `version`; add a sibling function that takes a `Skill` (already parsed) and a `source_dir`. This is the right place to centralize the "convert + record" step.
- Continue persisting to `config.installed` after success, with `install_path` set to the harness-native path (not the staging location).

**Patterns to follow:** The existing `commands::install` function shape; the download+tarpit logic in `RegistryClient::download` (`crates/usk-cli/src/registry.rs`) for URL handling.

**Test scenarios:**
- Happy path: `usk install ./spec/examples/escalation-handling --harness claude-code` creates `<HOME>/.claude/skills/escalation-handling/SKILL.md`.
- Happy path: same command writes to `config.installed` with the harness-native path.
- Edge case: local path with no `skill.yaml` errors with a clear message.
- Error path: URL that 404s is reported with the HTTP status.
- Error path: git URL that fails to clone is reported, tempdir is cleaned up.
- Integration: install locally, verify files exist at the harness-native path, verify `usk list` shows it.

**Verification:** `cargo test -p usk-cli` passes; the integration test crate gains a test for local install.

#### U3. Curl-able install script

**Goal:** A non-Rust user can run `curl -fsSL get.usk.dev | sh` and end up with `usk` on their PATH.

**Requirements:** R5

**Dependencies:** U2 (the install script installs the binary that includes the local-first flow)

**Files:**
- Create: `scripts/install.sh`
- Modify: `README.md` (quickstart uses the script)
- Modify: `.github/workflows/release.yml` (ensure release artifacts have stable names the script expects)

**Approach:**
- Standard pattern (mirrors `rustup`, `deno`, `uv`): detect OS (linux/macos) and arch (x86_64/aarch64), construct the GitHub release URL, download to a temp file, `chmod +x`, move to `~/.local/bin/usk` (or `$CARGO_HOME/bin/usk` if `~/.cargo/bin` is on PATH).
- Verify checksum against a `.sha256` file in the release artifacts.
- Print next-step instructions: "Run `usk setup` to finish configuration."
- Script is POSIX-sh compatible (no bashisms) so it works in default `sh`.

**Patterns to follow:** The `rustup` install script (https://sh.rustup.rs) is the gold standard; model the structure on it.

**Test scenarios:**
- Test expectation: none (shell scripts tested manually in CI via the release workflow, not unit-tested). The release workflow itself is the regression test — if a release artifact's name doesn't match what the script expects, the release fails.

**Verification:** Script is referenced from README; the release workflow produces artifacts with predictable names.

### Tier 2 — Authoring experience

#### U4. `usk inspect`

**Goal:** Show what's in a skill without unpacking it.

**Requirements:** R7

**Dependencies:** None (independent of install flow)

**Files:**
- Create: `crates/usk-cli/src/inspect.rs`
- Modify: `crates/usk-cli/src/main.rs` (new subcommand)

**Approach:**
- Accept a local path or an installed skill name (resolved via `config.installed`).
- Walk the skill directory, list files with sizes, render the parsed `skill.yaml` as a summary.
- For installed skills, walk `<install_path>/` (the harness-native path from U1).
- Output: a `tree`-like rendering: name, version, description, then a list of files with sizes.

**Test scenarios:**
- Happy path: inspect a local skill folder; output contains the skill name, version, and a list of files.
- Happy path: inspect an installed skill; output reflects the on-disk state.
- Edge case: skill with no `examples/` folder shows only the files present.
- Error path: path that doesn't exist errors clearly.

**Verification:** `cargo test -p usk-cli` passes for the new command.

#### U5. `usk read`

**Goal:** Read a file from a skill without manually `cd`-ing into the install dir.

**Requirements:** R7 (sister feature to inspect)

**Dependencies:** U4 (shares path resolution logic)

**Files:**
- Create: `crates/usk-cli/src/read.rs` (or fold into `inspect.rs`)
- Modify: `crates/usk-cli/src/main.rs` (new subcommand)

**Approach:**
- `usk read <name-or-path> <relative-file>` prints the file contents.
- Path traversal protection: reject `<relative-file>` containing `..` segments.

**Test scenarios:**
- Happy path: `usk read escalation-handling SKILL.md` prints the file.
- Error path: `usk read escalation-handling ../../etc/passwd` is rejected.

**Verification:** `cargo test -p usk-cli` passes.

#### U6. `usk validate`

**Goal:** Standalone validation that surfaces all errors at once.

**Requirements:** R8

**Dependencies:** None

**Files:**
- Create: `crates/usk-cli/src/validate_cmd.rs` (filename chosen to avoid clashing with `usk_core::validation`)
- Modify: `crates/usk-cli/src/main.rs` (new subcommand)

**Approach:**
- Reuse `usk_core::validation::validate` for the basic checks.
- Add deeper checks:
  - For each `NN-foo.md` in `instructions/`, verify the file exists.
  - For each path referenced in `SKILL.md` (e.g. `see templates/foo.md`), warn if missing.
  - For each file in `scripts/`, warn if it doesn't have a shebang or executable bit.
- Output: list of errors (blocking) and warnings (non-blocking), with file:line where possible.

**Test scenarios:**
- Happy path: validate a well-formed skill → 0 errors, 0 warnings.
- Edge case: missing entry file → 1 error referencing the path.
- Edge case: instructions directory present but empty → warning.
- Edge case: scripts with no execute bit → warning.

**Verification:** `cargo test -p usk-cli` passes; validate on `spec/examples/escalation-handling` succeeds with 0 errors.

#### U7. `usk convert`

**Goal:** Preview the harness adapter output for a local skill.

**Requirements:** R9

**Dependencies:** None (uses `HarnessAdapter` directly, no install flow)

**Files:**
- Create: `crates/usk-cli/src/convert.rs`
- Modify: `crates/usk-cli/src/main.rs` (new subcommand)

**Approach:**
- Parse the skill from `<path>/skill.yaml`, instantiate the named adapter, call `convert(skill, source_dir, --out)`.
- Print a summary of what was written: "Converted to claude-code: SKILL.md, examples/ (2 files), instructions/ (6 files)".
- No persistence to `config.installed`. This is a dry-run for authors.

**Test scenarios:**
- Happy path: convert to claude-code, `--out /tmp/preview`, files appear at the expected paths.
- Happy path: convert to codex-cli, `agent.yaml` is generated.
- Edge case: `--out` directory doesn't exist → create it.
- Error path: unknown harness key → clear error from `usk_harness_core::discovery::is_known`.

**Verification:** `cargo test -p usk-cli` passes; manual verification that the converted output matches what `usk install` would produce.

### Tier 3 — Distribution and discovery

#### U8. Public default registry URL

**Goal:** First-run users don't have to start a local server.

**Requirements:** R10

**Dependencies:** None (cosmetic config change)

**Files:**
- Modify: `crates/usk-core/src/config.rs` (the `Config::default()` function)

**Approach:**
- Change `registry_url` default from `http://localhost:8080` to a public URL.
- The user did not specify a real public registry hostname. The implementation should use a clearly-placeholder URL like `https://registry.usk.dev` with a comment in the code that says "Update this to the public registry when one is available; for now, the project is local-first and most workflows don't need a registry."
- Alternative: leave the default as `localhost:8080` but have `usk setup` (U-not-yet-defined) prompt for a registry URL on first run, defaulting to the local one.

**Decision recommendation:** Go with the placeholder URL approach. Self-hosting remains possible via `usk config set registry_url ...` or editing `~/.usk/config.toml`. Document this in the README.

**Test scenarios:**
- Happy path: fresh `Config::load()` on a machine with no `~/.usk/config.toml` produces a config with the new default URL.
- Existing test: `usk install <name>` against the default URL fails clearly (no public registry yet) but the failure message tells the user how to configure a local one.

**Verification:** `cargo test -p usk-core` passes; `usk config show` (or equivalent) reflects the new default.

#### U9. Server web UI

**Goal:** A human can browse the registry in a browser.

**Requirements:** R11

**Dependencies:** None (additive to the server)

**Files:**
- Create: `crates/usk-server/src/web.rs`
- Create: `crates/usk-server/src/templates/index.html` (or inline strings in `web.rs`)
- Create: `crates/usk-server/src/templates/skill.html`
- Modify: `crates/usk-server/src/main.rs` (route registration)
- Modify: `crates/usk-server/Cargo.toml` (add `pulldown-cmark` and a templating crate like `maud` or `askama`)

**Approach:**
- New axum routes:
  - `GET /` → list of skills (name, version, description, tags), sorted alphabetically.
  - `GET /skills/{name}` → skill details: metadata + rendered `SKILL.md` as HTML.
- `SKILL.md` is read from `registry_path/{name}/{latest_version}/SKILL.md` (using `RegistryIndex::get_latest`).
- Render markdown with `pulldown-cmark` and sanitize (use `pulldown-cmark`'s default — no raw HTML, which is safe).
- Use `maud` (compile-time HTML templates in Rust) for the page chrome. Lighter than `askama`, no external template files needed.
- Keep the existing JSON API routes unchanged.

**Patterns to follow:** The existing axum handler shape in `usk-server/src/main.rs`.

**Test scenarios:**
- Happy path: `GET /` returns 200, HTML contains at least one skill name from the registry.
- Happy path: `GET /skills/{name}` returns 200, HTML contains the skill description and rendered markdown.
- Edge case: skill name that doesn't exist → 404.
- Edge case: skill with no `SKILL.md` → page renders metadata, markdown section is empty/missing.

**Verification:** `cargo test -p usk-server` passes; manual verification that the pages render in a browser.

### Tier 4 — Composability

#### U10. Honor `requires` field

**Goal:** Skills can declare dependencies that install transitively.

**Requirements:** R12

**Dependencies:** U2 (uses the install flow)

**Files:**
- Create: `crates/usk-core/src/resolver.rs`
- Modify: `crates/usk-cli/src/main.rs` (install flow calls resolver)

**Approach:**
- New `Resolver` struct in `usk-core` that takes a starting skill name and a `&RegistryClient`.
- BFS through `requires`, fetching metadata for each, building a list of `(name, version, harness)` tuples.
- Detect cycles by tracking the current resolution path; if a cycle is found, return an error naming the cycle.
- Return the resolved list in topological order (dependencies before dependents).
- The install flow walks this list, installing each skill. Failures short-circuit with a clear message.

**Patterns to follow:** The `RegistryIndex` API in `crates/usk-core/src/index.rs` for version selection.

**Test scenarios:**
- Happy path: skill with one `requires` entry → both are installed.
- Happy path: transitive chain (A requires B requires C) → all three installed, in correct order.
- Edge case: a required skill is already installed → skip, don't re-install.
- Error path: cycle detected (A requires B requires A) → clear error.
- Error path: required skill doesn't exist in registry → clear error.

**Verification:** `cargo test -p usk-core` passes for the resolver; `cargo test -p usk-cli` passes for the install integration.

#### U11. Skill collections

**Goal:** Install all skills in a directory in one command.

**Requirements:** R13

**Dependencies:** U2 (local install), U10 (requires resolution)

**Files:**
- Modify: `crates/usk-cli/src/main.rs` (install dispatch detects "directory of skills" vs "single skill")

**Approach:**
- If the install path is a directory and contains a `skill.yaml` at its root → treat as single skill (current behavior).
- If the install path is a directory containing multiple subdirectories, each with its own `skill.yaml` → treat as a collection. Install each.
- Print a summary: "Installed 5 skills from ./skills/: foo, bar, baz, qux, quux."
- Failures on one skill don't abort the others; collect all errors and report at the end.

**Test scenarios:**
- Happy path: directory with 3 skill subdirs → 3 installs succeed.
- Edge case: directory with a mix of valid skills and directories without `skill.yaml` → valid skills install, invalid ones are reported.
- Edge case: directory with no skill subdirs at all → error.

**Verification:** `cargo test -p usk-cli` passes; the integration test crate gains a test for collections.

#### U12. `usk.lock`

**Goal:** Reproducible installs via a lockfile.

**Requirements:** R14

**Dependencies:** U10 (resolver), U11 (collections)

**Files:**
- Create: `crates/usk-core/src/lockfile.rs`
- Modify: `crates/usk-cli/src/main.rs` (write lock after install, read lock on `--locked` install)

**Approach:**
- Lockfile format (TOML):
  ```toml
  version = 1
  
  [[skill]]
  name = "escalation-handling"
  version = "1.0.0"
  harness = "claude-code"
  install_path = "/Users/you/.claude/skills/escalation-handling"
  
  [[skill]]
  name = "tone-analysis"
  version = "0.3.1"
  harness = "claude-code"
  install_path = "/Users/you/.claude/skills/tone-analysis"
  ```
- After a successful install (single or collection), write/update `usk.lock` in the project root (D3).
- `usk install --locked` reads the lockfile and installs each entry at the pinned version, ignoring the registry's "latest".
- `usk install <path>` (single skill, no `--locked`) still works and updates the lockfile with the new entry.
- A versioned `version` field in the lockfile (e.g. `version = 1`) lets us evolve the format later.

**Patterns to follow:** `Cargo.lock`'s structure; the existing `Config` persistence pattern in `usk-core/src/config.rs`.

**Test scenarios:**
- Happy path: install creates `usk.lock` with the entry.
- Happy path: `usk install --locked` reads the lockfile and installs at the pinned version even if the registry has a newer one.
- Edge case: lockfile with a removed skill is pruned after install.
- Error path: lockfile with a malformed entry errors with a clear message.
- Error path: lockfile references a harness that no longer exists → error or skip with warning.

**Verification:** `cargo test -p usk-core` passes for the lockfile module; `cargo test -p usk-cli` passes for the install integration.

---

## Phased Delivery

This work is naturally tiered. Each tier should land as one PR for reviewability:

### PR 1 — Tier 1: Foundations
- U1, U2, U3
- Biggest user-facing win: local install + correct paths
- Risk: medium (the install flow is core)
- Estimated size: ~600 lines of new code + tests

### PR 2 — Tier 2: Authoring
- U4, U5, U6, U7
- Lower risk: all additive commands, no install flow changes
- Estimated size: ~400 lines of new code + tests

### PR 3 — Tier 3: Distribution
- U8, U9
- Low risk: cosmetic config change + additive server routes
- Estimated size: ~300 lines + a new dependency (`pulldown-cmark`, `maud`)

### PR 4 — Tier 4: Composability
- U10, U11, U12
- Medium risk: dependency resolution and lockfile are subtle
- Estimated size: ~500 lines + tests

Each PR must:
- Pass all 47 existing tests
- Add tests for new behavior (target: +20 tests per PR)
- Update the relevant docs (`README.md`, `docs/DEVELOPMENT.md`)
- Include a CHANGELOG.md entry

---

## System-Wide Impact

- **Interaction graph:** `usk install` becomes a tri-modal dispatcher (path / URL / registry name). The existing `HarnessInstaller` trait wrapper in `usk-cli/src/main.rs` is folded into the new install flow; the local-first path doesn't need it.
- **Error propagation:** Local install errors should mention the local path. Registry install errors should mention the registry URL. The two paths must not silently swallow each other's errors.
- **State lifecycle risks:** U12's lockfile is the only piece of mutable state with new lifecycle concerns. Partial writes (crash mid-write) are handled by writing to `usk.lock.tmp` then renaming.
- **API surface parity:** Existing `usk install <name>` (registry) is preserved. New `usk install <path>` and `usk install <url>` are additive. No deprecations.
- **Integration coverage:** U2 + U11 + U12 form a chain (install a collection with locked versions). The integration test crate should have one end-to-end test that exercises this chain.
- **Unchanged invariants:** The `HarnessAdapter` trait signature is unchanged. The `RegistryIndex` API is unchanged. The `Config` schema is unchanged. The server's existing JSON API routes are unchanged. The 47 existing tests continue to pass.

---

## Risks & Dependencies

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| U1's harness path assumptions are wrong (e.g. Claude Code actually reads from a different directory) | Medium | High | U1 ships with a `--target` override so users can correct it. The README documents the assumed paths. The path map is in `usk-harness-core` so it updates with the adapter crate. |
| U2's URL handling for git URLs is complex (auth, submodules, large repos) | Medium | Medium | U2's git URL support is best-effort: use `git clone --depth 1` to a tempdir, then run the local install flow. Document the limitations. |
| U6's deeper validation produces too many warnings, becoming noise | Low | Medium | Warnings are opt-out via `--strict` (default: warnings, no errors). Tune the threshold based on what the example skills trigger. |
| U8's public registry URL is a placeholder and the project has no real one yet | High | Low | The placeholder URL is clearly marked as such; the local-first flow doesn't need it; the README explains how to set up a local registry. |
| U9's web UI adds dependencies (`pulldown-cmark`, `maud`) and the release binary grows | Low | Low | The dependencies are small (~500KB total). The web UI is optional in spirit — power users can keep using the JSON API. |
| U10's dependency resolution has subtle bugs (cycles, version conflicts) | Medium | High | U10 starts with name-only resolution (no semver ranges). Cycle detection is a hard requirement before merge. Version conflict resolution is a follow-up if needed. |
| U12's lockfile format is hard to evolve later | Low | Medium | Include a `version = 1` field in the lockfile. Future format changes bump the version and provide a migration. |

---

## Documentation Plan

- `README.md` quickstart: rewrite to lead with `curl -fsSL get.usk.dev | sh` → `usk install ./skill --harness claude-code` (no server). Existing quickstart (with the server) moves to a "Self-hosting" section.
- `README.md` commands table: add `inspect`, `validate`, `convert`, `read`, `setup` to the existing CLI reference.
- `docs/DEVELOPMENT.md`: document the new commands and the lockfile format.
- `docs/ARCHITECTURE.md`: update the "Data flow" section to show local-first install alongside registry install.
- `CHANGELOG.md`: one entry per tier landing, with the PR link.

---

## Open Questions

### Resolved during planning

- **Where do lockfiles live?** Project root, with global fallback at `~/.usk/installed.lock`. (D3)
- **Does the web UI need a build step?** No, server-side rendered with `maud`. (D5)
- **Does `requires` need semver ranges?** Not in v1; simple name resolution. (D4)

### Deferred to implementation

- **What is Codex CLI's actual skill install path?** Needs verification during U1. If unknown, the code accepts the user providing it via `--target` for that harness.
- **What is the public registry URL?** The user has not specified. U8 will use a clearly-placeholder URL (`https://registry.usk.dev` or similar) and document that it must be updated.
- **Should the install script support Windows?** `sh` works in WSL and Git Bash. Native Windows support is a follow-up.
- **Should `usk install` accept multiple positional args?** E.g. `usk install ./a ./b ./c`. TBD during U2 implementation; would naturally support the collections use case.

---

## Sources & References

- Origin discussion: previous conversation (analysis of layman journey + 12 suggested improvements)
- Related code:
  - `crates/usk-cli/src/main.rs` (`commands::install` is the main refactor target)
  - `crates/usk-harness-core/src/adapter.rs` (`HarnessAdapter` trait is the conversion primitive)
  - `crates/usk-core/src/config.rs` (`Config::default()` for U8)
  - `crates/usk-server/src/main.rs` (axum server, U9's target)
- External references:
  - `rustup` install script (https://sh.rustup.rs) — model for U3
  - `cargo install --path` / `pip install ./wheel` — UX model for U2
  - `Cargo.lock` structure — model for U12
