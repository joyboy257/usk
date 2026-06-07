# Changelog

All notable changes to USK will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-06-05

### Added
- Initial release of the Universal Skills Library
- `usk-core`: schema, validation, in-memory search index, config persistence
- `usk-harness-core`: `HarnessAdapter` trait
- `usk-harness-claude`: SKILL.md directory adapter
- `usk-harness-codex`: agent.yaml adapter
- `usk-cli`: 8 subcommands (new, publish, search, install, list, update, outdated, harness)
- `usk-server`: axum-based registry server with git audit trail
- `usk-integration-tests`: end-to-end pipeline tests
- Path traversal protection on publish
- Per-(name, version) concurrency lock on publish
- Harness adapter discovery with validation
- Example skills: escalation-handling, sales-call-prep

### Security
- Tarball path validation rejects absolute paths and parent traversal
