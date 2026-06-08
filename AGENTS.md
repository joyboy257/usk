# AGENTS.md — Working on USK

> **Read this first.** If you're an LLM agent (or a human + LLM) starting work on this repo, the strategic frame is below. The full plan is in `docs/plans/2026-06-08-001-feat-usk-v0.2-lean-cli-plan.md`. The handoff brief is in `docs/handoffs/2026-06-08-usk-v0.2-handoff.md`.

## The one-line summary

USK is a portable skill format and a small CLI for installing skills into the directory the agent harness reads from. The 60-second test is the only success metric that matters: a non-developer runs `usk install ./skill --harness claude-code` and Claude Code sees the skill.

## The bottleneck

The install path. If `usk install` does not write to the path the harness actually reads from, USK has no product. Verify this before doing anything else. The `HarnessAdapter` trait and `usk-harness-core::paths` are the contracts that hold this together.

## Strategic decisions — settled, do not relitigate

1. **No public hosted registry.** `usk-server` is buildable for self-hosting; it is not the product. The default `registry_url` is empty.
2. **No macOS app.** Build it after the CLI passes the 60-second test and there's signal from non-CLI users.
3. **No authoring commands in v0.2** (`inspect`, `validate`, `convert`, `read`). Parked.
4. **No transitive `requires` resolution in v0.2.** The `Resolver` module exists; do not wire it into the CLI yet.
5. **No LLM in the install loop.** Install path is ground truth in the harness's docs; use `Read`, not `prompt`.
6. **One harness-conversion trait.** `HarnessAdapter`. The CLI's `HarnessInstaller` wrapper is a refactor target, not a feature.
7. **Store + projection for enable/disable.** Skills live at `~/.usk/store/<harness>/<name>/`; the harness's path is a symlink created by `enable()`.

## What this means in practice

- **Do not** add LLM calls to install.
- **Do not** add a hosted registry, even if asked.
- **Do not** add the Mac app without a poll that says "yes" from non-CLI users.
- **Do not** introduce a third harness-conversion trait.
- **Do not** add authoring commands "while you're in there."
- **Do** read the plan's Implementation Status table before starting a unit.
- **Do** surface discrepancies between the plan and the code rather than silently fixing them.
- **Do** run `cargo test --workspace` as the first action.

## The plan's U-IDs are stable

U1–U8. Don't renumber, don't split without good reason, don't add new units without updating the plan. The plan is the contract; if it's wrong, the contract changes.

## Out of scope for v0.2 (parked, not killed)

- Authoring commands (`inspect`, `validate`, `convert`, `read`)
- Public hosted registry
- Server-rendered web UI
- Skill collections beyond what `local_install.rs` already has
- Transitive `requires` resolution beyond what `Resolver` already does
- Semver ranges in `requires`
- Cryptographic signing
- Editor / IDE integrations
- Telemetry, analytics, ratings, reviews, payments
- macOS app
- Multi-user collaborative editing
- Windows-native install script

Each of these has a "when to revisit" trigger; see the plan's Scope Boundaries section.

## Working style

- Small commits, small PRs. Each U-ID is roughly one commit.
- Tests are part of the unit. The plan's Test scenarios are the contract.
- `serial_test` is heavily used. Tests that touch `HOME` need `#[serial]`.
- When the plan says "verify," do verification. Don't re-implement.
- When you discover a discrepancy, surface it. The plan is the contract; if it's wrong, the contract changes.
- Ask when uncertain. Especially for UX decisions the plan defers (see Open Questions in the plan).
