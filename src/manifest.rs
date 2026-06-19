use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::config::config_dir;
use crate::error::GhrError;

/// Declarative, portable list of tools ghr should manage. Lives at
/// `~/.config/ghr/manifest.toml` alongside `config.toml`. Unlike `state.toml` (a local
/// runtime cache of install paths / sha256 / etags), the manifest holds only the portable
/// identity of each tool — its `repo` and an optional pinned `tag` — so it can be committed
/// to dotfiles and replayed on another machine with `ghr sync`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Manifest {
    #[serde(default)]
    pub tools: Vec<ManifestEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestEntry {
    pub repo: String,
    /// When set, the tool is pinned/locked to this exact release tag: `sync` installs it and
    /// `update` skips it. Absent means "track latest".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
    /// When set, the binary is installed (and tracked) under this name instead of the
    /// repo-derived default, so `sync` reproduces the custom name. Absent means "use the
    /// repo name".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alias: Option<String>,
}

impl Manifest {
    pub fn manifest_path() -> PathBuf {
        config_dir().join("manifest.toml")
    }

    pub fn load() -> Result<Self> {
        let path = Self::manifest_path();

        if !path.exists() {
            return Ok(Manifest::default());
        }

        let raw = std::fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;

        toml::from_str(&raw).map_err(|e| GhrError::StateCorrupted(e.to_string()).into())
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::manifest_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }

        let raw = toml::to_string_pretty(self).context("failed to serialize manifest")?;

        let tmp = path.with_extension("toml.tmp");
        std::fs::write(&tmp, raw).with_context(|| format!("failed to write {}", tmp.display()))?;
        std::fs::rename(&tmp, &path)
            .with_context(|| "failed to rename manifest file".to_string())?;

        Ok(())
    }

    pub fn get(&self, repo: &str) -> Option<&ManifestEntry> {
        self.tools.iter().find(|e| e.repo == repo)
    }

    /// Add or replace the row for `repo`, touching only its `tag` (`None` clears any pin) and
    /// leaving any existing `alias` intact. Used by the `update --force` pin toggle, which
    /// must not clobber a custom install name.
    pub fn upsert(&mut self, repo: &str, tag: Option<String>) {
        if let Some(existing) = self.tools.iter_mut().find(|e| e.repo == repo) {
            existing.tag = tag;
        } else {
            self.tools.push(ManifestEntry {
                repo: repo.to_string(),
                tag,
                alias: None,
            });
        }
    }

    /// Add or replace the row for `repo`, setting both the pin `tag` and the install `alias`.
    /// Used by `install`/`sync` where both are known up front.
    pub fn record(&mut self, repo: &str, tag: Option<String>, alias: Option<String>) {
        if let Some(existing) = self.tools.iter_mut().find(|e| e.repo == repo) {
            existing.tag = tag;
            existing.alias = alias;
        } else {
            self.tools.push(ManifestEntry {
                repo: repo.to_string(),
                tag,
                alias,
            });
        }
    }

    /// Drop the row for `repo`. Returns whether a row was removed.
    pub fn remove_repo(&mut self, repo: &str) -> bool {
        let before = self.tools.len();
        self.tools.retain(|e| e.repo != repo);
        self.tools.len() != before
    }

    /// The pinned tag for `repo`, if it is pinned.
    pub fn is_pinned(&self, repo: &str) -> Option<&str> {
        self.get(repo).and_then(|e| e.tag.as_deref())
    }

    pub fn iter(&self) -> impl Iterator<Item = &ManifestEntry> {
        self.tools.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_array_of_tables() {
        let mut m = Manifest::default();
        m.upsert("BurntSushi/ripgrep", None);
        m.upsert("sharkdp/bat", Some("v0.24.0".to_string()));

        let toml = toml::to_string_pretty(&m).unwrap();
        let back: Manifest = toml::from_str(&toml).unwrap();

        assert_eq!(back.tools, m.tools);
        // unpinned entry omits the tag key entirely
        assert!(!toml.contains("ripgrep\"\ntag"));
    }

    #[test]
    fn record_round_trips_alias() {
        let mut m = Manifest::default();
        m.record("BurntSushi/ripgrep", None, Some("rg".to_string()));

        let toml = toml::to_string_pretty(&m).unwrap();
        assert!(toml.contains("alias = \"rg\""));

        let back: Manifest = toml::from_str(&toml).unwrap();
        assert_eq!(back.tools, m.tools);
        assert_eq!(
            back.get("BurntSushi/ripgrep").unwrap().alias.as_deref(),
            Some("rg")
        );

        // An unaliased record omits the alias key entirely.
        let mut m2 = Manifest::default();
        m2.record("sharkdp/bat", None, None);
        let toml2 = toml::to_string_pretty(&m2).unwrap();
        assert!(!toml2.contains("alias"));
    }

    #[test]
    fn upsert_preserves_existing_alias() {
        let mut m = Manifest::default();
        m.record("a/b", None, Some("bee".to_string()));

        // The pin toggle (upsert) must not clobber a custom install name.
        m.upsert("a/b", Some("v1".to_string()));
        let e = m.get("a/b").unwrap();
        assert_eq!(e.tag.as_deref(), Some("v1"));
        assert_eq!(e.alias.as_deref(), Some("bee"));
    }

    #[test]
    fn upsert_adds_updates_and_clears_pin() {
        let mut m = Manifest::default();

        m.upsert("a/b", None);
        assert_eq!(m.tools.len(), 1);
        assert_eq!(m.is_pinned("a/b"), None);

        // update existing row's tag in place (no duplicate)
        m.upsert("a/b", Some("v1".to_string()));
        assert_eq!(m.tools.len(), 1);
        assert_eq!(m.is_pinned("a/b"), Some("v1"));

        // clearing the pin keeps the row but drops the tag
        m.upsert("a/b", None);
        assert_eq!(m.tools.len(), 1);
        assert_eq!(m.is_pinned("a/b"), None);
    }

    #[test]
    fn remove_repo_reports_whether_present() {
        let mut m = Manifest::default();
        m.upsert("a/b", None);

        assert!(m.remove_repo("a/b"));
        assert!(m.tools.is_empty());
        assert!(!m.remove_repo("a/b"));
    }

    #[test]
    fn is_pinned_only_when_tag_set() {
        let mut m = Manifest::default();
        m.upsert("a/b", None);
        m.upsert("c/d", Some("v2".to_string()));

        assert_eq!(m.is_pinned("a/b"), None);
        assert_eq!(m.is_pinned("c/d"), Some("v2"));
        assert_eq!(m.is_pinned("missing/repo"), None);
    }

    #[test]
    fn default_manifest_serializes_cleanly() {
        let m = Manifest::default();
        let toml = toml::to_string_pretty(&m).unwrap();
        let back: Manifest = toml::from_str(&toml).unwrap();
        assert!(back.tools.is_empty());
    }
}
