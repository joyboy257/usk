//! Known harness adapters in the USK ecosystem.

use std::collections::HashMap;

/// Metadata for a known harness adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HarnessInfo {
    /// The harness key used in skill.yaml and CLI (e.g. "claude-code")
    pub key: &'static str,
    /// The crate name providing the adapter
    pub crate_name: &'static str,
    /// Human-readable description
    pub description: &'static str,
}

/// The canonical list of harness adapters shipped with USK.
///
/// In v1, this is a hardcoded list. Future versions may support
/// dynamic discovery via plugin manifests.
pub const KNOWN_HARNESSES: &[HarnessInfo] = &[
    HarnessInfo {
        key: "claude-code",
        crate_name: "usk-harness-claude",
        description: "Anthropic Claude Code (SKILL.md directory format)",
    },
    HarnessInfo {
        key: "codex-cli",
        crate_name: "usk-harness-codex",
        description: "OpenAI Codex CLI (agent.yaml format)",
    },
];

/// Look up a harness by its key.
pub fn find(key: &str) -> Option<&'static HarnessInfo> {
    KNOWN_HARNESSES.iter().find(|h| h.key == key)
}

/// Return a map of all known harnesses, keyed by harness key.
pub fn all() -> HashMap<&'static str, &'static HarnessInfo> {
    KNOWN_HARNESSES.iter().map(|h| (h.key, h)).collect()
}

/// Check if a harness key is recognized.
pub fn is_known(key: &str) -> bool {
    find(key).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_known_harness() {
        let info = find("claude-code").unwrap();
        assert_eq!(info.crate_name, "usk-harness-claude");
    }

    #[test]
    fn test_find_unknown_harness() {
        assert!(find("nonexistent").is_none());
    }

    #[test]
    fn test_is_known() {
        assert!(is_known("claude-code"));
        assert!(is_known("codex-cli"));
        assert!(!is_known("bogus"));
    }

    #[test]
    fn test_all_returns_both() {
        let all = all();
        assert_eq!(all.len(), 2);
        assert!(all.contains_key("claude-code"));
        assert!(all.contains_key("codex-cli"));
    }
}
