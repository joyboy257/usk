//! `requires` resolution for skills (R12).
//!
//! A skill may declare a list of other skills it depends on via the
//! `requires` field in `skill.yaml`. This module walks that graph and
//! returns the skills in install order (dependencies first, the
//! requested skill last).
//!
//! The resolver is intentionally decoupled from any I/O: it takes two
//! closures for skill lookup and dependency lookup. This makes it
//! trivial to unit-test with synthetic graphs and lets callers plug in
//! either an in-memory index, a registry client, or anything else.

use crate::error::Result;
use crate::schema::SkillMeta;
use std::collections::HashSet;

/// A skill that has been resolved to a specific name+version pair.
///
/// `ResolvedSkill` only carries the bits a caller needs to install:
/// the skill name and the version selected for it. The caller is
/// responsible for any harness-specific install logic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedSkill {
    pub name: String,
    pub version: String,
}

/// Walks a `requires` graph and returns an install-ordered list.
///
/// One `Resolver` instance is a single resolution run; create a new
/// one per call if you want isolation. The internal `visiting` set
/// exists so a single cycle is reported once per run rather than
/// detected by accident on different traversals.
pub struct Resolver {
    /// Names currently on the DFS path. Used for cycle detection:
    /// if a `requires` entry is already in this set, we've circled
    /// back and must report an error.
    visiting: HashSet<String>,
    /// Skills already added to the output, by name. Prevents double-
    /// counting the same skill in a diamond dependency.
    seen: HashSet<String>,
    /// The final install order, in topological order (deps first).
    order: Vec<ResolvedSkill>,
}

impl Resolver {
    pub fn new() -> Self {
        Resolver {
            visiting: HashSet::new(),
            seen: HashSet::new(),
            order: Vec::new(),
        }
    }

    /// Resolve `start` and its `requires` chain.
    ///
    /// - `lookup(name)` returns the `SkillMeta` for a skill name, or
    ///   `None` if it cannot be found.
    /// - `requires(name, version)` returns the list of required skill
    ///   names for the skill at that (name, version) pair.
    ///
    /// The returned `Vec` is in install order: dependencies before
    /// dependents. A skill that is referenced more than once
    /// (diamond dependency) is only emitted once, at its first
    /// topological position.
    pub fn resolve<L, R>(
        &mut self,
        start: &str,
        lookup: L,
        requires: R,
    ) -> Result<Vec<ResolvedSkill>>
    where
        L: Fn(&str) -> Option<SkillMeta>,
        R: Fn(&str, &str) -> Vec<String>,
    {
        self.order.clear();
        self.seen.clear();
        self.visiting.clear();

        self.visit(start, &lookup, &requires)?;
        Ok(std::mem::take(&mut self.order))
    }

    /// DFS visit of `name` and its transitive `requires`. Pushes
    /// `name` onto `order` AFTER visiting its dependencies, so the
    /// resulting list is deps-first.
    fn visit<L, R>(
        &mut self,
        name: &str,
        lookup: &L,
        requires: &R,
    ) -> Result<()>
    where
        L: Fn(&str) -> Option<SkillMeta>,
        R: Fn(&str, &str) -> Vec<String>,
    {
        if self.seen.contains(name) {
            // Already emitted (diamond); nothing more to do.
            return Ok(());
        }
        if !self.visiting.insert(name.to_string()) {
            return Err(crate::error::SkillError::ValidationError(format!(
                "cycle detected in `requires` graph: '{}' is already being resolved",
                name
            )));
        }

        let meta = lookup(name).ok_or_else(|| {
            crate::error::SkillError::NotFound(format!(
                "required skill '{}' not found",
                name
            ))
        })?;

        for dep in requires(&meta.name, &meta.version) {
            self.visit(&dep, lookup, requires)?;
        }

        // Pop from the visiting set BEFORE recording the output so
        // that later (non-cyclic) references to this same name are
        // seen as "already resolved" and short-circuit.
        self.visiting.remove(name);
        self.seen.insert(name.to_string());
        self.order.push(ResolvedSkill {
            name: meta.name.clone(),
            version: meta.version.clone(),
        });
        Ok(())
    }
}

impl Default for Resolver {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// Build a graph from `(name, version, requires_list)` triples.
    /// The lookup closure returns metadata; the requires closure
    /// returns the deps list.
    fn make_graph(graph: &[(&str, &str, Vec<&str>)]) -> (
        impl Fn(&str) -> Option<SkillMeta>,
        impl Fn(&str, &str) -> Vec<String>,
    ) {
        let mut metas: HashMap<String, SkillMeta> = HashMap::new();
        let mut reqs: HashMap<(String, String), Vec<String>> = HashMap::new();
        for (name, version, requires) in graph {
            metas.insert(
                name.to_string(),
                SkillMeta {
                    name: name.to_string(),
                    version: version.to_string(),
                    description: None,
                    author: None,
                    tags: vec![],
                    harnesses: vec![],
                },
            );
            reqs.insert(
                (name.to_string(), version.to_string()),
                requires.iter().map(|s| s.to_string()).collect(),
            );
        }
        let lookup = move |name: &str| metas.get(name).cloned();
        let requires = move |name: &str, version: &str| {
            reqs.get(&(name.to_string(), version.to_string()))
                .cloned()
                .unwrap_or_default()
        };
        (lookup, requires)
    }

    #[test]
    fn test_resolve_single_skill() {
        let (lookup, requires) = make_graph(&[("foo", "1.0.0", vec![])]);
        let mut r = Resolver::new();
        let result = r.resolve("foo", lookup, requires).expect("resolve");
        assert_eq!(
            result,
            vec![ResolvedSkill {
                name: "foo".into(),
                version: "1.0.0".into()
            }]
        );
    }

    #[test]
    fn test_resolve_with_dependency() {
        let (lookup, requires) = make_graph(&[
            ("foo", "1.0.0", vec!["bar"]),
            ("bar", "2.0.0", vec![]),
        ]);
        let mut r = Resolver::new();
        let result = r.resolve("foo", lookup, requires).expect("resolve");
        assert_eq!(
            result,
            vec![
                ResolvedSkill {
                    name: "bar".into(),
                    version: "2.0.0".into()
                },
                ResolvedSkill {
                    name: "foo".into(),
                    version: "1.0.0".into()
                },
            ]
        );
    }

    #[test]
    fn test_resolve_transitive() {
        let (lookup, requires) = make_graph(&[
            ("a", "1.0.0", vec!["b"]),
            ("b", "1.0.0", vec!["c"]),
            ("c", "1.0.0", vec![]),
        ]);
        let mut r = Resolver::new();
        let result = r.resolve("a", lookup, requires).expect("resolve");
        // c first (deepest), then b, then a.
        assert_eq!(
            result,
            vec![
                ResolvedSkill {
                    name: "c".into(),
                    version: "1.0.0".into()
                },
                ResolvedSkill {
                    name: "b".into(),
                    version: "1.0.0".into()
                },
                ResolvedSkill {
                    name: "a".into(),
                    version: "1.0.0".into()
                },
            ]
        );
    }

    #[test]
    fn test_resolve_detects_cycle() {
        let (lookup, requires) = make_graph(&[
            ("a", "1.0.0", vec!["b"]),
            ("b", "1.0.0", vec!["a"]),
        ]);
        let mut r = Resolver::new();
        let result = r.resolve("a", lookup, requires);
        assert!(result.is_err(), "expected cycle error, got {:?}", result);
        let err = format!("{}", result.unwrap_err());
        assert!(
            err.contains("cycle") || err.contains("already being resolved"),
            "error should mention cycle, got: {}",
            err
        );
    }

    #[test]
    fn test_resolve_missing_dep() {
        let (lookup, requires) = make_graph(&[("a", "1.0.0", vec!["missing"])]);
        let mut r = Resolver::new();
        let result = r.resolve("a", lookup, requires);
        assert!(result.is_err(), "expected not-found error, got {:?}", result);
        let err = format!("{}", result.unwrap_err());
        assert!(
            err.contains("missing") && err.contains("not found"),
            "error should mention the missing dep name, got: {}",
            err
        );
    }

    /// Diamond dep (a requires both b and c, both require d) should
    /// emit d exactly once.
    #[test]
    fn test_resolve_diamond_emits_dep_once() {
        let (lookup, requires) = make_graph(&[
            ("a", "1.0.0", vec!["b", "c"]),
            ("b", "1.0.0", vec!["d"]),
            ("c", "1.0.0", vec!["d"]),
            ("d", "1.0.0", vec![]),
        ]);
        let mut r = Resolver::new();
        let result = r.resolve("a", lookup, requires).expect("resolve");
        // d should appear exactly once and be the first entry.
        let d_count = result.iter().filter(|s| s.name == "d").count();
        assert_eq!(d_count, 1, "diamond dep should be emitted once");
        assert_eq!(result.last().unwrap().name, "a");
    }
}
