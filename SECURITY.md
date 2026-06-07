# Security Policy

## Supported Versions

| Version | Supported          |
|---------|--------------------|
| 0.1.x   | :white_check_mark:  |
| < 0.1   | :x:                |

Only the latest minor release receives security fixes. Please upgrade before reporting issues against older versions.

## Reporting a Vulnerability

**Please do not file public GitHub issues for security vulnerabilities.**

Report privately via one of the following channels:

- **Email:** security@usk.dev
- **GitHub Security Advisories:** use the "Report a vulnerability" button on the [security tab](../../security/advisories/new)

A maintainer will acknowledge receipt within 2 business days.

## What to Include

To help us triage quickly, please include:

- A description of the vulnerability and its impact
- Reproduction steps or a proof-of-concept
- The affected version(s)
- Your assessment of severity (e.g., RCE, path traversal, info disclosure)
- Any known mitigations

## Response Timeline

| Stage | Target |
|-------|--------|
| Acknowledgement | 2 business days |
| Triage and severity assessment | 7 days |
| Patch for critical issues | 14 days |
| Patch for high severity | 30 days |
| Public disclosure | After a fix is released, or 90 days after report, whichever is sooner |

We follow [coordinated disclosure](https://en.wikipedia.org/wiki/Coordinated_vulnerability_disclosure). We will credit reporters in the release notes unless anonymity is requested.

## Scope

In scope:

- Path traversal, archive handling, or file overwrite bugs in `usk-cli` or `usk-server`
- Authentication / authorization flaws in `usk-server`
- Supply chain issues (dependency confusion, malicious crates)
- Code execution via published skill content
- Crashes or data loss in core data structures (parser, index, validation)

Out of scope:

- Vulnerabilities in upstream dependencies (report upstream)
- Issues requiring physical access to a developer's machine
- Theoretical issues without a working proof of concept

## Recognition

We maintain a thank-you list in release notes for reporters who consent to being credited.
