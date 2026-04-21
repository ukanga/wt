use anyhow::Result;
use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ValueEnum, Default)]
#[serde(rename_all = "lowercase")]
pub enum SessionMode {
    #[default]
    Panes,
    Windows,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub session: SessionConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionConfig {
    #[serde(default)]
    pub mode: SessionMode,
    #[serde(default = "default_panes")]
    pub panes: u8,
    #[serde(default = "default_session_prefix")]
    pub session_prefix: String,
    #[serde(default = "default_agent_cmd")]
    pub agent_cmd: String,
    #[serde(default = "default_editor_cmd")]
    pub editor_cmd: String,
}

fn default_panes() -> u8 {
    2
}

fn default_session_prefix() -> String {
    "wt-".to_string()
}

fn default_agent_cmd() -> String {
    "claude".to_string()
}

fn default_editor_cmd() -> String {
    "nvim".to_string()
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            mode: SessionMode::default(),
            panes: default_panes(),
            session_prefix: default_session_prefix(),
            agent_cmd: default_agent_cmd(),
            editor_cmd: default_editor_cmd(),
        }
    }
}

impl SessionConfig {
    /// Compute the tmux session name for a worktree in windows mode by
    /// prepending `session_prefix`. An empty prefix returns the worktree
    /// name unchanged (opt-in by the user).
    pub fn session_name_for(&self, worktree: &str) -> String {
        format!("{}{}", self.session_prefix, worktree)
    }
}

impl Config {
    /// Load config with precedence: .wt.toml > ~/.wt/config.toml > defaults
    pub fn load() -> Self {
        let global = dirs::home_dir().map(|h| h.join(".wt").join("config.toml"));
        Self::load_layered(global.as_deref(), Some(Path::new(".wt.toml")))
    }

    /// Load config for a specific repo path
    pub fn load_for_repo(repo_path: &Path) -> Self {
        let global = dirs::home_dir().map(|h| h.join(".wt").join("config.toml"));
        let local = repo_path.join(".wt.toml");
        Self::load_layered(global.as_deref(), Some(&local))
    }

    /// Merge the two TOML files field-by-field (local wins) and then
    /// deserialize into `Config`. This preserves fields set in the global
    /// file when the local file only sets a subset of keys in the same
    /// section.
    fn load_layered(global: Option<&Path>, local: Option<&Path>) -> Self {
        let mut merged = toml::Table::new();
        if let Some(path) = global {
            if let Ok(contents) = std::fs::read_to_string(path) {
                if let Ok(table) = toml::from_str::<toml::Table>(&contents) {
                    deep_merge_tables(&mut merged, table);
                }
            }
        }
        if let Some(path) = local {
            if let Ok(contents) = std::fs::read_to_string(path) {
                if let Ok(table) = toml::from_str::<toml::Table>(&contents) {
                    deep_merge_tables(&mut merged, table);
                }
            }
        }
        toml::Value::Table(merged)
            .try_into::<Config>()
            .unwrap_or_default()
    }

    /// Get effective pane count (flag override if provided)
    pub fn effective_panes(&self, flag_override: Option<u8>) -> u8 {
        flag_override.unwrap_or(self.session.panes).clamp(2, 3)
    }

    /// Ensure ~/.wt directory exists
    pub fn ensure_wt_dir() -> Result<std::path::PathBuf> {
        let home =
            dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Could not find home directory"))?;
        let wt_dir = home.join(".wt");
        std::fs::create_dir_all(&wt_dir)?;
        Ok(wt_dir)
    }
}

/// Recursively merge `overlay` into `base`. When both contain a table under
/// the same key, they are merged together so that keys the overlay omits
/// keep the base's value; all other value kinds are replaced wholesale.
///
/// Used to layer `.wt.toml` (repo) over `~/.wt/config.toml` (global) so
/// that a partial section in the repo file does not reset the global
/// file's other keys to defaults.
fn deep_merge_tables(base: &mut toml::Table, overlay: toml::Table) {
    for (key, overlay_value) in overlay {
        match (base.get_mut(&key), overlay_value) {
            (Some(toml::Value::Table(base_table)), toml::Value::Table(overlay_table)) => {
                deep_merge_tables(base_table, overlay_table);
            }
            (_, value) => {
                base.insert(key, value);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.session.panes, 2);
        assert_eq!(config.session.agent_cmd, "claude");
        assert_eq!(config.session.editor_cmd, "nvim");
    }

    #[test]
    fn test_effective_panes_clamp() {
        let config = Config::default();
        assert_eq!(config.effective_panes(Some(1)), 2);
        assert_eq!(config.effective_panes(Some(2)), 2);
        assert_eq!(config.effective_panes(Some(3)), 3);
        assert_eq!(config.effective_panes(Some(4)), 3);
        assert_eq!(config.effective_panes(None), 2);
    }

    #[test]
    fn test_parse_toml() {
        let toml_str = r#"
[session]
panes = 3
agent_cmd = "aider"
editor_cmd = "vim"
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.session.panes, 3);
        assert_eq!(config.session.agent_cmd, "aider");
        assert_eq!(config.session.editor_cmd, "vim");
    }

    #[test]
    fn test_partial_toml() {
        let toml_str = r#"
[session]
panes = 3
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.session.panes, 3);
        assert_eq!(config.session.agent_cmd, "claude");
        assert_eq!(config.session.editor_cmd, "nvim");
    }

    #[test]
    fn test_default_mode_is_panes() {
        let config = Config::default();
        assert_eq!(config.session.mode, SessionMode::Panes);
    }

    #[test]
    fn test_parse_mode_panes() {
        let toml_str = r#"
[session]
mode = "panes"
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.session.mode, SessionMode::Panes);
    }

    #[test]
    fn test_parse_mode_windows() {
        let toml_str = r#"
[session]
mode = "windows"
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.session.mode, SessionMode::Windows);
    }

    #[test]
    fn test_mode_missing_uses_default() {
        let toml_str = r#"
[session]
panes = 3
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.session.mode, SessionMode::Panes);
    }

    #[test]
    fn test_default_session_prefix() {
        let config = Config::default();
        assert_eq!(config.session.session_prefix, "wt-");
    }

    #[test]
    fn test_session_name_for_default_prefix() {
        let config = Config::default();
        assert_eq!(
            config.session.session_name_for("detect-pii"),
            "wt-detect-pii"
        );
    }

    #[test]
    fn test_session_name_for_empty_prefix() {
        let mut config = Config::default();
        config.session.session_prefix = String::new();
        assert_eq!(config.session.session_name_for("detect-pii"), "detect-pii");
    }

    #[test]
    fn test_session_name_for_custom_prefix() {
        let mut config = Config::default();
        config.session.session_prefix = "proj/".to_string();
        assert_eq!(config.session.session_name_for("foo"), "proj/foo");
    }

    #[test]
    fn test_parse_session_prefix_empty_string() {
        let toml_str = r#"
[session]
session_prefix = ""
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.session.session_prefix, "");
    }

    #[test]
    fn test_deep_merge_tables_preserves_unshadowed_keys() {
        let mut base: toml::Table = toml::from_str(
            r#"
[session]
agent_cmd = "aider"
panes = 2
"#,
        )
        .unwrap();
        let overlay: toml::Table = toml::from_str(
            r#"
[session]
mode = "windows"
"#,
        )
        .unwrap();

        deep_merge_tables(&mut base, overlay);

        // `session` is a table in both — deep-merged.
        let session = base.get("session").and_then(|v| v.as_table()).unwrap();
        assert_eq!(session.get("agent_cmd").unwrap().as_str(), Some("aider"));
        assert_eq!(session.get("panes").unwrap().as_integer(), Some(2));
        assert_eq!(session.get("mode").unwrap().as_str(), Some("windows"));
    }

    #[test]
    fn test_deep_merge_tables_overlay_scalar_replaces() {
        let mut base: toml::Table = toml::from_str(
            r#"
[session]
panes = 2
"#,
        )
        .unwrap();
        let overlay: toml::Table = toml::from_str(
            r#"
[session]
panes = 3
"#,
        )
        .unwrap();
        deep_merge_tables(&mut base, overlay);
        let session = base.get("session").and_then(|v| v.as_table()).unwrap();
        assert_eq!(session.get("panes").unwrap().as_integer(), Some(3));
    }

    #[test]
    fn test_load_layered_partial_local_preserves_global_fields() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let global = dir.path().join("global.toml");
        let local = dir.path().join("local.toml");

        writeln!(
            std::fs::File::create(&global).unwrap(),
            "[session]\nagent_cmd = \"aider\"\npanes = 3\n"
        )
        .unwrap();
        writeln!(
            std::fs::File::create(&local).unwrap(),
            "[session]\nmode = \"windows\"\nsession_prefix = \"\"\n"
        )
        .unwrap();

        let config = Config::load_layered(Some(&global), Some(&local));
        assert_eq!(config.session.mode, SessionMode::Windows);
        assert_eq!(config.session.session_prefix, "");
        // Preserved from the global file — bug was that these reset to defaults.
        assert_eq!(config.session.agent_cmd, "aider");
        assert_eq!(config.session.panes, 3);
    }

    #[test]
    fn test_load_layered_local_overrides_scalar() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let global = dir.path().join("global.toml");
        let local = dir.path().join("local.toml");

        writeln!(
            std::fs::File::create(&global).unwrap(),
            "[session]\npanes = 2\nagent_cmd = \"aider\"\n"
        )
        .unwrap();
        writeln!(
            std::fs::File::create(&local).unwrap(),
            "[session]\npanes = 3\n"
        )
        .unwrap();

        let config = Config::load_layered(Some(&global), Some(&local));
        assert_eq!(config.session.panes, 3);
        assert_eq!(config.session.agent_cmd, "aider");
    }

    #[test]
    fn test_load_layered_returns_default_when_both_missing() {
        let config = Config::load_layered(None, None);
        assert_eq!(config.session.mode, SessionMode::Panes);
        assert_eq!(config.session.panes, 2);
        assert_eq!(config.session.agent_cmd, "claude");
    }
}
