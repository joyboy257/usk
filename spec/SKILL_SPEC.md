# Universal Skill Format Specification v0.1

## Overview

A Universal Skill is a reusable package of instructions, examples, templates, scripts, and reference material that teaches an AI agent how to perform a specific method of working. Skills are harness-agnostic — they can be converted to any AI agent framework format via adapters.

## Directory Layout

```
my-skill/
├── skill.yaml             # Metadata + manifest (REQUIRED)
├── SKILL.md               # Main instruction document (REQUIRED)
├── instructions/          # Sub-step instructions (optional)
├── examples/              # Example inputs/outputs (optional)
├── templates/             # Output templates (optional)
├── scripts/               # Executable scripts used during skill execution (optional)
└── references/            # Reference documentation (optional)
```

## skill.yaml Schema

### Required Fields

| Field | Type | Description |
|-------|------|-------------|
| `name` | string | Unique skill name (kebab-case) |
| `version` | string | Semantic version string (semver) |

### Optional Fields

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `description` | string | `""` | Human-readable description |
| `author` | string | `""` | Author or organization name |
| `license` | string | `""` | SPDX license identifier |
| `tags` | array[string] | `[]` | Categorization tags |
| `harnesses` | map[string]string | `{}` | Supported harnesses with version constraints |
| `entry` | string | `"SKILL.md"` | Main instruction entry point |
| `config` | object | `null` | Runtime configuration |

### Harness Compatibility Matrix

The `harnesses` field declares which AI agent frameworks a skill supports, with semver version requirements per harness:

| Harness Key | Framework | Adapter | Adapter Crate |
|-------------|-----------|---------|---------------|
| `claude-code` | Anthropic Claude Code | SKILL.md directory | `usk-harness-claude` |
| `codex-cli` | OpenAI Codex CLI | agent.yaml | `usk-harness-codex` |

#### Field & Directory Consumption

Each adapter consumes a subset of the universal skill fields and directories:

| Field / Directory | Claude Code | Codex CLI |
|-------------------|-------------|-----------|
| `name` | Used as directory name | Written to agent.yaml |
| `version` | Ignored | Written to agent.yaml metadata |
| `description` | Ignored | Written as agent description |
| `tags` | Ignored | Written as agent tags |
| `author` | Ignored | Ignored |
| `license` | Ignored | Ignored |
| `harnesses` | Version-constrained install | Version-constrained install |
| `entry` | Entry point filename | Primary instruction reference |
| `config.timeout` | Ignored | Written to agent config |
| `config.temperature` | Ignored | Written to agent config |
| `requires` | Ignored | Ignored (v1) |
| `SKILL.md` | Main instruction body | Main instruction body |
| `instructions/` | PRESERVED as sub-step files | PRESERVED as sub-step files |
| `examples/` | Copied as reference files | Embedded as few-shot examples |
| `templates/` | Copied as reference files | Written as output templates |
| `scripts/` | Copied as supporting files | Exposed as tool definitions |
| `references/` | Copied as reference files | Copied as reference files |

#### Adapter Behaviors

**Claude Code Adapter (`usk-harness-claude`):**
- Copies `SKILL.md` and all supporting directories to the output directory
- Preserves directory structure — Claude Code can reference sub-files via tool calls
- No harness-specific preamble is added; instructions are used as-is
- Validates that the `entry` file exists before conversion

**Codex CLI Adapter (`usk-harness-codex`):**
- Generates a single `agent.yaml` file in Codex CLI format
- `name`, `description`, `tags` map directly to agent.yaml top-level fields
- `config.timeout` and `config.temperature` become agent runtime settings
- `examples/` files are embedded as in-context few-shot examples in the agent definition
- `scripts/` files are registered as tool definitions the agent can invoke
- `templates/` and `references/` are copied to the output directory alongside agent.yaml
- The original `SKILL.md` content is preserved as the primary instruction body inside agent.yaml

### Config Object

| Field | Type | Description |
|-------|------|-------------|
| `timeout` | integer | Max execution time in seconds |
| `temperature` | float | LLM temperature (0.0 - 1.0) |

## Directory Details

### skill.yaml (REQUIRED)

The manifest file. Contains all metadata. Must be valid YAML.

### SKILL.md (REQUIRED)

The main instruction document. Written in harness-agnostic markdown. Adapters may add harness-specific preamble or postscript during conversion, but the core instructions should be portable.

### instructions/ (optional)

Sub-step instruction files. Named with numeric prefixes for ordering (e.g., `01-assess-severity.md`, `02-craft-response.md`). Each file covers one phase of the skill's workflow.

Convention: each numbered step in `SKILL.md`'s Process section SHOULD have a corresponding file in `instructions/`. The main `SKILL.md` provides the overview and output format; the individual instruction files contain the detailed guidance for each step.

### examples/ (optional)

Example inputs and expected outputs. Each file demonstrates one scenario. Adapters that support few-shot learning (e.g., Codex CLI) may embed these as in-context examples. Other adapters (e.g., Claude Code) include them as reference files.

### templates/ (optional)

Output templates or scaffolding files. Used by the skill's instructions to produce structured output.

### scripts/ (optional)

Executable scripts invoked during skill execution. Adapters may expose these as tool definitions.

### references/ (optional)

Reference documentation that the skill draws on — style guides, matrices, lookup tables, etc.

## Conversion Contract

Each harness adapter MUST:
1. Preserve the content of `SKILL.md` as the primary instruction body
2. Include all supporting files that the target harness can meaningfully consume
3. Place output in the adapter's documented install directory structure
4. Fail with a clear error if required fields are missing or invalid

Each harness adapter SHOULD:
- Add harness-specific preamble or postscript to the instructions when it improves the agent's ability to apply the skill
- Document which fields and directories it consumes and which it ignores
- Validate the harness version constraint before installation
