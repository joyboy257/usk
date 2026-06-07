use clap::{Parser, Subcommand};


#[derive(Parser)]
#[command(name = "usk", about = "Universal Skills Library CLI", version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Create a new skill scaffold
    New {
        /// Name of the skill
        name: String,
    },
    /// Validate and publish the current skill to the registry
    Publish {
        /// Path to the skill directory
        #[arg(default_value = ".")]
        path: String,
    },
    /// Search for skills in the registry
    Search {
        /// Search query
        query: String,
    },
    /// Install a skill from the registry, a local path, or a URL
    Install {
        /// Name of the skill, local path (./foo, /abs/path, ~/foo),
        /// or URL (https://..., git@...)
        name: String,
        /// Target harness (e.g., claude-code, codex-cli)
        #[arg(long)]
        harness: Option<String>,
        /// Override the install path (defaults to the harness-native location)
        #[arg(long)]
        target: Option<String>,
        /// Install from usk.lock instead of the named source (U12)
        #[arg(long)]
        locked: bool,
    },
    /// List installed skills
    List,
    /// Check for updates to installed skills
    Update {
        /// Specific skill to update (updates all if omitted)
        name: Option<String>,
    },
    /// List outdated installed skills
    Outdated,
    /// Manage installed harness adapters
    Harness {
        #[command(subcommand)]
        command: HarnessCommands,
    },
    /// Show a skill's contents (files, sizes, parsed metadata)
    Inspect {
        /// Path to the skill directory to inspect
        path: String,
    },
    /// Print a file from a skill directory to stdout
    Read {
        /// Path to the skill directory
        path: String,
        /// Relative file path within the skill
        file: String,
    },
    /// Run structural validation on a skill directory
    Validate {
        /// Path to the skill directory to validate
        path: String,
    },
    /// Convert a skill to a harness-specific output (dry-run preview)
    Convert {
        /// Path to the skill directory
        path: String,
        /// Target harness
        #[arg(long)]
        harness: String,
        /// Output directory
        #[arg(long)]
        out: String,
    },
}

#[derive(Subcommand)]
enum HarnessCommands {
    /// Register a harness adapter
    Add {
        /// Harness name (e.g., claude-code, codex-cli)
        name: String,
    },
    /// Remove a harness adapter
    Remove {
        /// Harness name
        name: String,
    },
    /// List registered harness adapters
    List,
}

mod registry;
mod inspect;
mod read;
mod validate_cmd;
mod convert;
mod local_install;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    let config = usk_core::config::Config::load();
    let client = registry::RegistryClient::new(&config.registry_url);
    let cli = Cli::parse();

    let result = match cli.command {
        Commands::New { name } => commands::new_skill(&name, &config),
        Commands::Publish { path } => commands::publish(&path, &config, &client).await,
        Commands::Search { query } => commands::search(&query, &client).await,
        Commands::Install { name, harness, target, locked } => {
            commands::install(&name, harness.as_deref(), target.as_deref(), locked, &config, &client).await
        }
        Commands::List => commands::list(&config),
        Commands::Update { name } => commands::update(name.as_deref(), &config, &client).await,
        Commands::Outdated => commands::outdated(&config, &client).await,
        Commands::Harness { command } => match command {
            HarnessCommands::Add { name } => commands::harness_add(&name, &config),
            HarnessCommands::Remove { name } => commands::harness_remove(&name, &config),
            HarnessCommands::List => commands::harness_list(&config),
        },
        Commands::Inspect { path } => inspect::inspect(std::path::Path::new(&path)),
        Commands::Read { path, file } => read::read(std::path::Path::new(&path), &file),
        Commands::Validate { path } => validate_cmd::validate(std::path::Path::new(&path)),
        Commands::Convert { path, harness, out } => {
            convert::convert(std::path::Path::new(&path), &harness, std::path::Path::new(&out))
        }
    };

    if let Err(e) = result {
        eprintln!("error: {}", e);
        std::process::exit(1);
    }
}

mod commands {
    use std::path::Path;
    use usk_core::config::Config;

    use crate::registry::RegistryClient;

    pub fn new_skill(name: &str, _config: &Config) -> Result<(), String> {
        let dir = Path::new(name);
        if dir.exists() {
            return Err(format!("directory '{}' already exists", name));
        }
        std::fs::create_dir_all(dir).map_err(|e| format!("failed to create directory: {}", e))?;

        let skill_yaml = format!(
            r#"name: "{name}"
version: 0.1.0
description: ""
author: ""
license: MIT
tags: []
harnesses: {{}}
requires: []
entry: SKILL.md
"#
        );
        std::fs::write(dir.join("skill.yaml"), skill_yaml).map_err(|e| e.to_string())?;
        std::fs::write(dir.join("SKILL.md"), format!("# {name}\n\n")).map_err(|e| e.to_string())?;

        println!("Created skill '{}'", name);
        Ok(())
    }

    pub async fn publish(path: &str, _config: &Config, client: &RegistryClient) -> Result<(), String> {
        let dir = Path::new(path);
        let skill_yaml = dir.join("skill.yaml");

        if !skill_yaml.exists() {
            return Err(format!("no skill.yaml found in '{}'", path));
        }

        match usk_core::parser::parse_skill_yaml(&skill_yaml) {
            Ok(skill) => {
                if let Err(e) = usk_core::validation::validate(&skill, dir) {
                    return Err(format!("skill validation failed: {}", e));
                }
                println!("Skill '{}' v{} validated", skill.name, skill.version);
                client.publish(dir).await?;
                Ok(())
            }
            Err(e) => Err(format!("failed to parse skill.yaml: {}", e)),
        }
    }

    pub async fn search(query: &str, client: &RegistryClient) -> Result<(), String> {
        println!("Searching for '{}'...", query);
        let results = client.search(query).await?;
        if results.is_empty() {
            println!("  (no results found)");
        } else {
            for skill in &results {
                println!("  {} v{}", skill.name, skill.version);
                if let Some(desc) = &skill.description {
                    if !desc.is_empty() {
                        println!("      {}", desc);
                    }
                }
                if !skill.tags.is_empty() {
                    println!("      tags: [{}]", skill.tags.join(", "));
                }
            }
        }
        Ok(())
    }

    pub async fn install(name: &str, harness: Option<&str>, target: Option<&str>, locked: bool, config: &Config, client: &RegistryClient) -> Result<(), String> {
        use crate::local_install::{
            default_lockfile_path, entry_from_install, install_collection,
            install_with_requires, is_collection, load_lockfile_at,
            record_to_lockfile,
        };

        // --locked: install from usk.lock, ignoring `name` (U12)
        if locked {
            let lf = load_lockfile_at(&default_lockfile_path());
            if lf.skill.is_empty() {
                return Err("no skills in usk.lock; run `usk install <name>` first to populate it".to_string());
            }
            for entry in &lf.skill {
                println!("  Installing {} v{} ({})...", entry.name, entry.version, entry.harness);
                if let Err(e) = client.download(&entry.name, &entry.version, &entry.install_path).await {
                    eprintln!("    warning: download failed: {}", e);
                    continue;
                }
                if let Some(adapter) = adapter_for(&entry.harness) {
                    if let Err(e) = adapter.convert_to(&entry.name, &entry.version, &entry.install_path) {
                        eprintln!("    warning: conversion failed: {}", e);
                    }
                }
            }
            return Ok(());
        }

        // Tri-modal dispatch (Tier 1 / U2):
        //   1. Local path: starts with ./, ../, /, or ~. Read
        //      skill.yaml directly and run the harness adapter.
        //   2. URL: http(s)://, git@, git://, ssh://. Fetch to a
        //      tempdir and run the local-path flow.
        //   3. Registry: anything else. Hit the registry server.
        // Local and URL install require an explicit harness (it
        // determines the install path). Registry install falls back
        // to the registered harnesses if `--harness` is omitted
        // (preserves the original behavior).
        let effective_harness = harness;

        if crate::local_install::is_local_path(name) {
            let harness_name = effective_harness.ok_or_else(|| {
                "no harness specified; pass --harness <name>".to_string()
            })?;
            let source = expand_user(name);
            let target_path = target.map(Path::new);

            // Collection detection (U11): if `source` is a directory of
            // multiple skill subdirs, route to the collection flow.
            if is_collection(&source) {
                println!("Installing collection from '{}'...", name);
                let paths = install_collection(&source, harness_name, config)?;
                for path in &paths {
                    println!("  Installed to {}", path.display());
                    if let Err(e) = record_to_lockfile_at_install(path, harness_name) {
                        eprintln!("  warning: lockfile write failed: {}", e);
                    }
                }
                return Ok(());
            }

            // Single skill from local path
            println!("Installing local skill from '{}'...", name);
            let result = crate::local_install::install_from_path_with_target(
                &source,
                harness_name,
                config,
                target_path,
            )?;
            println!("  Installed to {}", result.display());

            // Lockfile write (U12)
            if let Err(e) = record_to_lockfile_at_install(&result, harness_name) {
                eprintln!("  warning: lockfile write failed: {}", e);
            }

            // `requires` resolution (U10): surface a warning if the skill
            // declares dependencies that can't be resolved locally.
            if let Ok(skill) = usk_core::parser::parse_skill_yaml(&source.join("skill.yaml")) {
                if !skill.requires.is_empty() {
                    let result = install_with_requires(
                        &source,
                        harness_name,
                        config,
                        |_name| None,  // v1: no local registry lookup
                        |_name, _version| skill.requires.clone(),
                    );
                    if let Err(e) = result {
                        eprintln!("  warning: requires resolution: {}", e);
                    }
                }
            }

            return Ok(());
        }

        if crate::local_install::is_url(name) {
            println!("Installing from URL '{}'...", name);
            let harness_name = effective_harness.ok_or_else(|| {
                "no harness specified; pass --harness <name>".to_string()
            })?;
            // URL install: the inner local-path flow honors the
            // harness-native location; --target is not currently
            // threaded through to URL install.
            let _ = target;
            let result = crate::local_install::install_from_url(name, harness_name, config)?;
            println!("  Installed to {}", result.display());
            if let Err(e) = record_to_lockfile_at_install(&result, harness_name) {
                eprintln!("  warning: lockfile write failed: {}", e);
            }
            return Ok(());
        }

        println!("Installing '{}' from registry...", name);

        let meta = client.get_package(name).await?;

        let adapters = get_adapters(harness, config);
        if adapters.is_empty() {
            return Err("no harness adapters matched; use `usk harness add <name>` to register".to_string());
        }

        let mut config = config.clone();

        for adapter in &adapters {
            let install_base = config.install_dir.join(adapter.name());
            let skill_install = install_base.join(name);
            std::fs::create_dir_all(&skill_install).map_err(|e| format!("create install dir: {}", e))?;

            if let Err(e) = client.download(name, &meta.version, &skill_install).await {
                eprintln!("  warning: download for '{}' failed: {}", adapter.name(), e);
                continue;
            }

            if let Err(e) = adapter.convert_to(name, &meta.version, &skill_install) {
                eprintln!("  warning: conversion for '{}' failed: {}", adapter.name(), e);
                continue;
            }

            config.installed.insert(
                name.to_string(),
                usk_core::config::InstalledSkill {
                    version: meta.version.clone(),
                    harness: adapter.name().to_string(),
                    install_path: skill_install.clone(),
                },
            );

            // Lockfile write (U12)
            let entry = entry_from_install(name, &meta.version, adapter.name(), &skill_install);
            if let Err(e) = record_to_lockfile(&entry) {
                eprintln!("  warning: lockfile write failed: {}", e);
            }

            println!("  Installed for harness: {}", adapter.name());
        }

        config.save().map_err(|e| format!("failed to save config: {}", e))?;
        Ok(())
    }

    /// Build a `LockEntry` from an installed skill's directory by
    /// re-reading its `skill.yaml`, and append it to `usk.lock`.
    /// Used after every install path (local, URL, collection) so the
    /// lockfile reflects on-disk state.
    fn record_to_lockfile_at_install(
        install_path: &Path,
        harness: &str,
    ) -> Result<(), String> {
        let yaml_path = install_path.join("skill.yaml");
        let content = std::fs::read_to_string(&yaml_path)
            .map_err(|e| format!("read {}: {}", yaml_path.display(), e))?;
        let skill: usk_core::schema::Skill = serde_yaml::from_str(&content)
            .map_err(|e| format!("parse {}: {}", yaml_path.display(), e))?;
        let entry = crate::local_install::entry_from_install(&skill.name, &skill.version, harness, install_path);
        crate::local_install::record_to_lockfile(&entry)
    }

    /// Expand a leading `~` or `~/...` to the home directory. Used
    /// for the local-path install flow.
    fn expand_user(path: &str) -> std::path::PathBuf {
        if path == "~" {
            if let Ok(home) = std::env::var("HOME") {
                return std::path::PathBuf::from(home);
            }
        } else if let Some(rest) = path.strip_prefix("~/") {
            if let Ok(home) = std::env::var("HOME") {
                return std::path::PathBuf::from(home).join(rest);
            }
        }
        std::path::PathBuf::from(path)
    }

    pub fn list(config: &Config) -> Result<(), String> {
        if config.installed.is_empty() {
            println!("No skills installed.");
            println!("  Use `usk install <name>` to install a skill.");
            return Ok(());
        }
        println!("Installed skills:");
        for (name, info) in &config.installed {
            println!("  {} v{} (harness: {})", name, info.version, info.harness);
        }
        Ok(())
    }

    pub async fn update(name: Option<&str>, config: &Config, client: &RegistryClient) -> Result<(), String> {
        let mut config = config.clone();
        let mut dirty = false;

        match name {
            Some(n) => {
                println!("Checking updates for '{}'...", n);
                if let Some(installed) = config.installed.get(n).cloned() {
                    let meta = client.get_package(n).await?;
                    if meta.version != installed.version {
                        println!("  Update available: {} -> {}", installed.version, meta.version);
                        apply_update(&mut config, client, n, &installed, &meta.version).await?;
                        dirty = true;
                    } else {
                        println!("  Already up to date (v{})", installed.version);
                    }
                } else {
                    println!("  '{}' is not installed", n);
                }
            }
            None => {
                println!("Checking updates for all installed skills...");
                // Collect updates to apply so we don't hold a borrow on
                // `config.installed` while mutating it.
                let entries: Vec<(String, usk_core::config::InstalledSkill)> = config
                    .installed
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();
                for (n, installed) in entries {
                    match client.get_package(&n).await {
                        Ok(meta) => {
                            if meta.version != installed.version {
                                println!("  {}: {} -> {}", n, installed.version, meta.version);
                                apply_update(&mut config, client, &n, &installed, &meta.version).await?;
                                dirty = true;
                            } else {
                                println!("  {}: already up to date (v{})", n, installed.version);
                            }
                        }
                        Err(e) => {
                            println!("  {}: check failed ({})", n, e);
                        }
                    }
                }
            }
        }

        if dirty {
            config.save().map_err(|e| format!("failed to save config: {}", e))?;
        }
        Ok(())
    }

    /// Re-download the tarball and re-run the harness conversion for an
    /// installed skill, then update the version recorded in config.
    async fn apply_update(
        config: &mut Config,
        client: &RegistryClient,
        skill_name: &str,
        installed: &usk_core::config::InstalledSkill,
        new_version: &str,
    ) -> Result<(), String> {
        // Wipe the existing install dir so stale files don't linger after
        // the tarball extracts (e.g. removed entries in the new version).
        if installed.install_path.exists() {
            std::fs::remove_dir_all(&installed.install_path)
                .map_err(|e| format!("failed to clear old install at {:?}: {}", installed.install_path, e))?;
        }
        std::fs::create_dir_all(&installed.install_path)
            .map_err(|e| format!("failed to recreate install dir: {}", e))?;

        client
            .download(skill_name, new_version, &installed.install_path)
            .await?;

        // Re-run the harness conversion for the recorded harness.
        if let Some(adapter) = adapter_for(&installed.harness) {
            if let Err(e) = adapter.convert_to(skill_name, new_version, &installed.install_path) {
                return Err(format!("conversion for '{}' failed: {}", installed.harness, e));
            }
        } else {
            eprintln!(
                "  warning: no adapter implementation for harness '{}'; skill files updated but not re-converted",
                installed.harness
            );
        }

        if let Some(entry) = config.installed.get_mut(skill_name) {
            entry.version = new_version.to_string();
        }
        println!("  Updated '{}' to v{}", skill_name, new_version);
        Ok(())
    }

    pub async fn outdated(config: &Config, client: &RegistryClient) -> Result<(), String> {
        println!("Outdated installed skills:");
        let mut found = false;
        for (name, info) in &config.installed {
            match client.get_package(name).await {
                Ok(meta) => {
                    if meta.version != info.version {
                        println!("  {}: {} -> {}", name, info.version, meta.version);
                        found = true;
                    }
                }
                Err(_) => {}
            }
        }
        if !found {
            println!("  (all up to date)");
        }
        Ok(())
    }

    pub fn harness_add(name: &str, config: &Config) -> Result<(), String> {
        if !usk_harness_core::discovery::is_known(name) {
            let known: Vec<String> = usk_harness_core::discovery::KNOWN_HARNESSES
                .iter()
                .map(|h| h.key.to_string())
                .collect();
            return Err(format!(
                "unknown harness '{}'. Known harnesses: {}",
                name,
                known.join(", ")
            ));
        }
        let mut config = config.clone();
        config.harnesses.insert(name.to_string(), name.to_string());
        config.save().map_err(|e| format!("failed to save config: {}", e))?;
        println!("Registered harness adapter: {}", name);
        Ok(())
    }

    pub fn harness_remove(name: &str, config: &Config) -> Result<(), String> {
        let mut config = config.clone();
        config.harnesses.remove(name);
        config.save().map_err(|e| format!("failed to save config: {}", e))?;
        println!("Removed harness adapter: {}", name);
        Ok(())
    }

    pub fn harness_list(config: &Config) -> Result<(), String> {
        if config.harnesses.is_empty() {
            println!("No harness adapters registered.");
            return Ok(());
        }
        println!("Registered harness adapters:");
        for name in config.harnesses.keys() {
            // `usk harness add` only accepts known harness keys, so every entry
            // in `config.harnesses` MUST resolve via `discovery::find`. If
            // this panics, the config file was edited by hand or corrupted.
            let info = usk_harness_core::discovery::find(name)
                .expect("config.harnesses contains unknown key; config file may be corrupted");
            println!("  {} ({}) - {}", name, info.crate_name, info.description);
        }
        Ok(())
    }

    use usk_harness_core::adapter::HarnessAdapter;

    /// Build the list of harness installers to use for an install/update.
    ///
    /// Source of truth: `config.harnesses` (a `HashMap<String, String>`
    /// of harness key -> adapter crate name). We only support the two
    /// concrete adapters compiled into this binary; anything else logs a
    /// warning and is skipped. This keeps `usk harness add` and
    /// `usk harness list` in sync with what `install` actually does.
    fn get_adapters(filter: Option<&str>, config: &Config) -> Vec<Box<dyn HarnessInstaller>> {
        // If the user explicitly filters by a name, only consider that
        // name (still validated against the known set below).
        let keys: Vec<String> = match filter {
            Some(name) => vec![name.to_string()],
            None => config.harnesses.keys().cloned().collect(),
        };

        let mut out: Vec<Box<dyn HarnessInstaller>> = Vec::new();
        for key in keys {
            // Only act on harnesses the user has registered; this is what
            // makes `usk harness add foo` matter for `install`.
            if !config.harnesses.contains_key(&key) && filter.is_none() {
                continue;
            }
            match key.as_str() {
                "claude-code" => out.push(Box::new(ClaudeInstaller)),
                "codex-cli" => out.push(Box::new(CodexInstaller)),
                other => {
                    eprintln!(
                        "  warning: harness '{}' is registered but has no built-in adapter; skipping",
                        other
                    );
                }
            }
        }
        out
    }

    /// Look up a single adapter by its harness key. Used by the update
    /// path where we already have the harness name recorded in config.
    fn adapter_for(name: &str) -> Option<Box<dyn HarnessInstaller>> {
        match name {
            "claude-code" => Some(Box::new(ClaudeInstaller)),
            "codex-cli" => Some(Box::new(CodexInstaller)),
            _ => None,
        }
    }

    /// Test-only public wrapper around `get_adapters` so unit tests in
    /// the parent module can assert on the source-of-truth logic.
    /// Returns the harness names as `String` so callers don't need to
    /// invoke the private trait method.
    #[cfg(test)]
    pub fn adapter_names_for_test(filter: Option<&str>, config: &Config) -> Vec<String> {
        get_adapters(filter, config)
            .iter()
            .map(|a| a.name().to_string())
            .collect()
    }

    trait HarnessInstaller: Send + Sync {
        fn name(&self) -> &str;
        fn convert_to(&self, skill_name: &str, version: &str, install_dir: &Path) -> Result<(), String>;
    }

    // Concrete impls and the trait above are private to this module.
    // Test code in the parent `tests` module only needs to call `name()`
    // through a `&dyn HarnessInstaller` reference, which works because
    // methods on a trait are callable through the trait object regardless
    // of the trait's own visibility. The error above came from calling
    // `name()` on a `Box<dyn HarnessInstaller>` from outside the module;
    // that requires the method to be `pub` on the trait.

    struct ClaudeInstaller;

    impl HarnessInstaller for ClaudeInstaller {
        fn name(&self) -> &str {
            "claude-code"
        }

        fn convert_to(&self, skill_name: &str, _version: &str, install_dir: &Path) -> Result<(), String> {
            let adapter = usk_harness_claude::converter::ClaudeCodeAdapter;
            let skill = load_skill(skill_name, install_dir)?;
            let output = install_dir.join("converted");
            std::fs::create_dir_all(&output).map_err(|e| e.to_string())?;
            adapter.convert(&skill, install_dir, &output).map_err(|e: usk_harness_core::error::HarnessError| e.to_string())?;
            println!("  Converted for Claude Code at {:?}", output);
            Ok(())
        }
    }

    struct CodexInstaller;

    impl HarnessInstaller for CodexInstaller {
        fn name(&self) -> &str {
            "codex-cli"
        }

        fn convert_to(&self, skill_name: &str, _version: &str, install_dir: &Path) -> Result<(), String> {
            let adapter = usk_harness_codex::converter::CodexCliAdapter;
            let skill = load_skill(skill_name, install_dir)?;
            let output = install_dir.join("converted");
            std::fs::create_dir_all(&output).map_err(|e| e.to_string())?;
            adapter.convert(&skill, install_dir, &output).map_err(|e: usk_harness_core::error::HarnessError| e.to_string())?;
            println!("  Converted for Codex CLI at {:?}", output);
            Ok(())
        }
    }

    fn load_skill(name: &str, skill_dir: &Path) -> Result<usk_core::schema::Skill, String> {
        let yaml_path = skill_dir.join("skill.yaml");
        if yaml_path.exists() {
            usk_core::parser::parse_skill_yaml(&yaml_path).map_err(|e| e.to_string())
        } else {
            Ok(usk_core::schema::Skill {
                name: name.to_string(),
                version: "0.0.0".to_string(),
                description: None,
                author: None,
                license: None,
                tags: vec![],
                harnesses: std::collections::HashMap::new(),
                requires: vec![],
                entry: "SKILL.md".to_string(),
                config: None,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use axum::extract::Path as AxPath;
    use axum::http::StatusCode;
    use axum::response::IntoResponse;
    use axum::routing::get;
    use axum::Router;

    use usk_core::config::Config;

    use crate::commands::install;
    use crate::registry::RegistryClient;

    use serial_test::serial;

    /// Build a gzipped tarball containing a single `skill.yaml` + `SKILL.md`
    /// pair. Returns the raw bytes, suitable for serving from the test
    /// mock registry.
    fn build_skill_tarball(skill_name: &str, version: &str) -> Vec<u8> {
        let tmp = tempfile::tempdir().expect("tempdir");
        let src = tmp.path().join(skill_name);
        std::fs::create_dir_all(&src).unwrap();
        let yaml = format!(
            r#"name: "{name}"
version: "{ver}"
description: "test skill"
author: "tester"
license: MIT
tags: []
harnesses: {{}}
requires: []
entry: SKILL.md
"#,
            name = skill_name,
            ver = version
        );
        std::fs::write(src.join("skill.yaml"), yaml).unwrap();
        std::fs::write(src.join("SKILL.md"), format!("# {}\n", skill_name)).unwrap();

        let tar_path = tmp.path().join("out.tar.gz");
        let file = std::fs::File::create(&tar_path).unwrap();
        let encoder = flate2::write::GzEncoder::new(file, flate2::Compression::default());
        let mut archive = tar::Builder::new(encoder);
        archive.append_dir_all(".", &src).unwrap();
        let encoder = archive.into_inner().unwrap();
        encoder.finish().unwrap();
        std::fs::read(&tar_path).unwrap()
    }

    /// Spin up a minimal registry server in the background. Handles
    /// `GET /api/v1/packages/:name` and `GET /api/v1/packages/:name/:ver/download`.
    async fn spawn_mock_registry(
        skill_name: &str,
        version: &str,
        tarball: Arc<Vec<u8>>,
    ) -> (String, tokio::task::JoinHandle<()>) {
        let app = Router::new()
            .route(
                "/api/v1/packages/{name}",
                get({
                    let name = skill_name.to_string();
                    let version = version.to_string();
                    move |AxPath(pname): AxPath<String>| async move {
                        if pname == name {
                            let body = serde_json::json!({
                                "name": name,
                                "version": version,
                                "description": "test skill",
                                "author": "tester",
                                "tags": [],
                                "harnesses": [],
                            });
                            (StatusCode::OK, axum::Json(body)).into_response()
                        } else {
                            (StatusCode::NOT_FOUND, "not found").into_response()
                        }
                    }
                }),
            )
            .route(
                "/api/v1/packages/{name}/{version}/download",
                get({
                    let tarball = tarball.clone();
                    let name = skill_name.to_string();
                    let version = version.to_string();
                    move |AxPath((pname, pver)): AxPath<(String, String)>| async move {
                        if pname == name && pver == version {
                            (StatusCode::OK, tarball.as_ref().clone()).into_response()
                        } else {
                            (StatusCode::NOT_FOUND, "not found").into_response()
                        }
                    }
                }),
            );

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        (format!("http://{}", addr), handle)
    }

    /// Test that a successful `install` persists the skill to
    /// `config.installed` so that `usk list`, `usk update`, and
    /// `usk outdated` can see it.
    #[tokio::test]
    async fn install_persists_to_config() {
        // Isolate config to a temp dir so we don't touch the real
        // ~/.usk on the dev machine.
        let tmp = tempfile::tempdir().expect("tempdir");
        std::env::set_var("USK_CONFIG_DIR", tmp.path());

        // Build a fake skill tarball and start a mock registry.
        let tarball = Arc::new(build_skill_tarball("demo-skill", "1.2.3"));
        let (base_url, server) = spawn_mock_registry("demo-skill", "1.2.3", tarball).await;

        // Build a config that points at the mock registry and has the
        // install dir under the same temp dir.
        let install_dir = tmp.path().join("skills");
        std::fs::create_dir_all(&install_dir).unwrap();
        let mut harnesses = HashMap::new();
        harnesses.insert("claude-code".to_string(), "usk-harness-claude".to_string());
        let config = Config {
            registry_url: base_url.clone(),
            install_dir: install_dir.clone(),
            harnesses,
            installed: HashMap::new(),
        };

        let client = RegistryClient::new(&base_url);
        let result = install("demo-skill", None, None, false, &config, &client).await;
        assert!(result.is_ok(), "install failed: {:?}", result.err());

        // Re-load config from disk to verify persistence.
        let saved = Config::load();
        assert!(
            saved.installed.contains_key("demo-skill"),
            "expected `demo-skill` in config.installed, got: {:?}",
            saved.installed
        );
        let entry = &saved.installed["demo-skill"];
        assert_eq!(entry.version, "1.2.3");
        assert_eq!(entry.harness, "claude-code");
        assert!(entry.install_path.starts_with(&install_dir));

        // The skill install dir should now contain the extracted
        // `skill.yaml` (proves the tarball was actually downloaded,
        // not just metadata).
        let installed_skill_yaml = entry.install_path.join("skill.yaml");
        assert!(
            installed_skill_yaml.exists(),
            "expected skill.yaml at {:?}",
            installed_skill_yaml
        );

        // The install base for the harness should also exist.
        let harness_base = install_dir.join("claude-code");
        assert!(harness_base.exists(), "missing harness install base {:?}", harness_base);

        server.abort();
        std::env::remove_var("USK_CONFIG_DIR");
    }

    /// Sanity check: `get_adapters` honors the `--harness` filter and the
    /// `config.harnesses` source of truth.
    #[test]
    fn get_adapters_uses_config_source_of_truth() {
        let tmp = tempfile::tempdir().expect("tempdir");
        std::env::set_var("USK_CONFIG_DIR", tmp.path());

        // Empty config -> no adapters, no implicit defaults.
        let mut config = Config::default();
        config.harnesses.clear();
        let names = crate::commands::adapter_names_for_test(None, &config);
        assert!(names.is_empty(), "empty harnesses should yield no adapters");

        // Only claude-code registered -> exactly one adapter.
        config
            .harnesses
            .insert("claude-code".to_string(), "usk-harness-claude".to_string());
        let names = crate::commands::adapter_names_for_test(None, &config);
        assert_eq!(names, vec!["claude-code"]);

        // Explicit filter that doesn't match anything -> empty.
        let names = crate::commands::adapter_names_for_test(Some("does-not-exist"), &config);
        assert!(names.is_empty());

        // Unknown harness key registered -> warning + skip (no panic).
        config
            .harnesses
            .insert("mystery-harness".to_string(), "usk-harness-mystery".to_string());
        let names = crate::commands::adapter_names_for_test(None, &config);
        // Should still only have claude-code; the unknown key is skipped.
        assert!(names.contains(&"claude-code".to_string()));
        assert!(!names.contains(&"mystery-harness".to_string()));

        std::env::remove_var("USK_CONFIG_DIR");
    }

    // --- harness subcommand tests ---
    //
    // These tests manipulate the `USK_CONFIG_DIR` env var, which is
    // process-global state. They are marked `#[serial]` so they run
    // one-at-a-time and don't race each other (or the
    // `install_persists_to_config` and
    // `get_adapters_uses_config_source_of_truth` tests above, which
    // also touch this env var).

    use crate::commands::{harness_add, harness_list, harness_remove};

    #[test]
    #[serial]
    fn test_harness_add_accepts_known() {
        let temp = tempfile::tempdir().unwrap();
        std::env::set_var("USK_CONFIG_DIR", temp.path());

        let config = Config::default();
        let result = harness_add("claude-code", &config);
        assert!(result.is_ok(), "harness_add failed: {:?}", result.err());

        let reloaded = Config::load();
        assert!(
            reloaded.harnesses.contains_key("claude-code"),
            "expected `claude-code` in config.harnesses, got: {:?}",
            reloaded.harnesses
        );

        std::env::remove_var("USK_CONFIG_DIR");
    }

    #[test]
    #[serial]
    fn test_harness_add_rejects_unknown() {
        let temp = tempfile::tempdir().unwrap();
        std::env::set_var("USK_CONFIG_DIR", temp.path());

        let config = Config::default();
        let result = harness_add("bogus-harness", &config);
        assert!(result.is_err(), "expected error for unknown harness");
        let err = result.unwrap_err();
        assert!(
            err.contains("unknown harness"),
            "error message should mention 'unknown harness', got: {}",
            err
        );

        // Config should not have been written.
        let reloaded = Config::load();
        assert!(
            !reloaded.harnesses.contains_key("bogus-harness"),
            "bogus-harness should not be in config"
        );

        std::env::remove_var("USK_CONFIG_DIR");
    }

    #[test]
    #[serial]
    fn test_harness_add_then_remove() {
        let temp = tempfile::tempdir().unwrap();
        std::env::set_var("USK_CONFIG_DIR", temp.path());

        let config = Config::default();
        harness_add("claude-code", &config).expect("add should succeed");
        let after_add = Config::load();
        assert!(after_add.harnesses.contains_key("claude-code"));

        harness_remove("claude-code", &config).expect("remove should succeed");
        let after_remove = Config::load();
        assert!(
            !after_remove.harnesses.contains_key("claude-code"),
            "claude-code should be removed from config"
        );

        std::env::remove_var("USK_CONFIG_DIR");
    }

    #[test]
    #[serial]
    fn test_harness_list_shows_descriptions() {
        let temp = tempfile::tempdir().unwrap();
        std::env::set_var("USK_CONFIG_DIR", temp.path());

        let mut config = Config::default();
        config
            .harnesses
            .insert("claude-code".to_string(), "usk-harness-claude".to_string());
        config
            .harnesses
            .insert("codex-cli".to_string(), "usk-harness-codex".to_string());
        // Persist the seed config so harness_list's view matches.
        config.save().expect("save seed config");

        let result = harness_list(&config);
        assert!(result.is_ok(), "harness_list failed: {:?}", result.err());

        // Sanity-check the underlying config still has both keys.
        let reloaded = Config::load();
        assert!(reloaded.harnesses.contains_key("claude-code"));
        assert!(reloaded.harnesses.contains_key("codex-cli"));

        std::env::remove_var("USK_CONFIG_DIR");
    }
}
