use anyhow::Result;
use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Tmux layout for `wt session`. `Panes` packs all worktrees into one
/// shared `wt` session with a window per worktree split into panes.
/// `Windows` gives each worktree its own tmux session with one window
/// per role (agent / shell / edit).
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
    /// Resolve the tmux session name for a worktree in windows mode.
    /// An empty `session_prefix` returns the worktree name unchanged.
    pub fn session_name_for(&self, worktree: &str) -> String {
        format!("{}{}", self.session_prefix, worktree)
    }
}

impl Config {
    /// Load config with precedence: .wt.toml > ~/.wt/config.toml > defaults.
    pub fn load() -> Self {
        let global = dirs::home_dir().map(|h| h.join(".wt").join("config.toml"));
        Self::load_layered(global.as_deref(), Some(Path::new(".wt.toml")))
    }

    /// Load config for a specific repo path.
    pub fn load_for_repo(repo_path: &Path) -> Self {
        let global = dirs::home_dir().map(|h| h.join(".wt").join("config.toml"));
        let local = repo_path.join(".wt.toml");
        Self::load_layered(global.as_deref(), Some(&local))
    }

    /// Deep-merge the two TOML files field-by-field (local wins) and
    /// deserialize into `Config`. A partial `[session]` table in the
    /// local file therefore preserves keys set only in the global file.
    ///
    /// Each file is validated as a full `Config` independently before its
    /// table is merged, so a malformed file is skipped (with a warning on
    /// stderr) and cannot poison the other layer.
    fn load_layered(global: Option<&Path>, local: Option<&Path>) -> Self {
        let mut merged = toml::Table::new();
        for path in [global, local].into_iter().flatten() {
            if let Some(table) = load_valid_config_table(path) {
                deep_merge_tables(&mut merged, table);
            }
        }
        toml::Value::Table(merged)
            .try_into::<Config>()
            .unwrap_or_default()
    }

    /// Get effective pane count (flag override if provided).
    pub fn effective_panes(&self, flag_override: Option<u8>) -> u8 {
        flag_override.unwrap_or(self.session.panes).clamp(2, 3)
    }

    /// Ensure ~/.wt directory exists.
    pub fn ensure_wt_dir() -> Result<std::path::PathBuf> {
        let home =
            dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Could not find home directory"))?;
        let wt_dir = home.join(".wt");
        std::fs::create_dir_all(&wt_dir)?;
        Ok(wt_dir)
    }
}

/// Recursively merge `overlay` into `base`. Tables merge key-by-key so the
/// overlay only replaces the fields it sets; all other value kinds are
/// replaced wholesale.
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

/// Read one config file and return its parsed `toml::Table`, but only if
/// the same contents also deserialize cleanly into `Config`. Missing file
/// → None silently; malformed file → None with a warning. This keeps a
/// broken file from taking down the sibling layer.
fn load_valid_config_table(path: &Path) -> Option<toml::Table> {
    let contents = std::fs::read_to_string(path).ok()?;
    let table: toml::Table = match toml::from_str(&contents) {
        Ok(t) => t,
        Err(e) => {
            eprintln!(
                "wt: warning: ignoring malformed TOML at {}: {}",
                path.display(),
                e
            );
            return None;
        }
    };
    if let Err(e) = toml::from_str::<Config>(&contents) {
        eprintln!(
            "wt: warning: ignoring invalid config at {}: {}",
            path.display(),
            e
        );
        return None;
    }
    Some(table)
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
        assert_eq!(Config::default().session.mode, SessionMode::Panes);
    }

    #[test]
    fn test_default_session_prefix() {
        assert_eq!(Config::default().session.session_prefix, "wt-");
    }

    #[test]
    fn test_parse_mode_panes() {
        let config: Config = toml::from_str("[session]\nmode = \"panes\"\n").unwrap();
        assert_eq!(config.session.mode, SessionMode::Panes);
    }

    #[test]
    fn test_parse_mode_windows() {
        let config: Config = toml::from_str("[session]\nmode = \"windows\"\n").unwrap();
        assert_eq!(config.session.mode, SessionMode::Windows);
    }

    #[test]
    fn test_mode_missing_uses_default() {
        let config: Config = toml::from_str("[session]\npanes = 3\n").unwrap();
        assert_eq!(config.session.mode, SessionMode::Panes);
    }

    #[test]
    fn test_parse_session_prefix_empty_string() {
        let config: Config = toml::from_str("[session]\nsession_prefix = \"\"\n").unwrap();
        assert_eq!(config.session.session_prefix, "");
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
    fn test_deep_merge_tables_preserves_unshadowed_keys() {
        let mut base: toml::Table =
            toml::from_str("[session]\nagent_cmd = \"aider\"\npanes = 2\n").unwrap();
        let overlay: toml::Table = toml::from_str("[session]\nmode = \"windows\"\n").unwrap();
        deep_merge_tables(&mut base, overlay);
        let session = base.get("session").and_then(|v| v.as_table()).unwrap();
        assert_eq!(session.get("agent_cmd").unwrap().as_str(), Some("aider"));
        assert_eq!(session.get("panes").unwrap().as_integer(), Some(2));
        assert_eq!(session.get("mode").unwrap().as_str(), Some("windows"));
    }

    #[test]
    fn test_deep_merge_tables_overlay_scalar_replaces() {
        let mut base: toml::Table = toml::from_str("[session]\npanes = 2\n").unwrap();
        let overlay: toml::Table = toml::from_str("[session]\npanes = 3\n").unwrap();
        deep_merge_tables(&mut base, overlay);
        assert_eq!(
            base.get("session")
                .and_then(|v| v.as_table())
                .and_then(|t| t.get("panes"))
                .and_then(|v| v.as_integer()),
            Some(3)
        );
    }

    fn write(path: &std::path::Path, body: &str) {
        use std::io::Write;
        writeln!(std::fs::File::create(path).unwrap(), "{}", body).unwrap();
    }

    #[test]
    fn test_load_layered_partial_local_preserves_global_fields() {
        let dir = tempfile::tempdir().unwrap();
        let global = dir.path().join("global.toml");
        let local = dir.path().join("local.toml");
        write(&global, "[session]\nagent_cmd = \"aider\"\npanes = 3\n");
        write(
            &local,
            "[session]\nmode = \"windows\"\nsession_prefix = \"\"\n",
        );
        let config = Config::load_layered(Some(&global), Some(&local));
        assert_eq!(config.session.mode, SessionMode::Windows);
        assert_eq!(config.session.session_prefix, "");
        assert_eq!(config.session.agent_cmd, "aider");
        assert_eq!(config.session.panes, 3);
    }

    #[test]
    fn test_load_layered_local_overrides_scalar() {
        let dir = tempfile::tempdir().unwrap();
        let global = dir.path().join("global.toml");
        let local = dir.path().join("local.toml");
        write(&global, "[session]\npanes = 2\nagent_cmd = \"aider\"\n");
        write(&local, "[session]\npanes = 3\n");
        let config = Config::load_layered(Some(&global), Some(&local));
        assert_eq!(config.session.panes, 3);
        assert_eq!(config.session.agent_cmd, "aider");
    }

    #[test]
    fn test_load_layered_returns_default_when_both_missing() {
        let config = Config::load_layered(None, None);
        assert_eq!(config.session.mode, SessionMode::Panes);
        assert_eq!(config.session.panes, 2);
    }

    #[test]
    fn test_load_layered_invalid_local_preserves_global() {
        let dir = tempfile::tempdir().unwrap();
        let global = dir.path().join("global.toml");
        let local = dir.path().join("local.toml");
        write(&global, "[session]\nagent_cmd = \"aider\"\npanes = 3\n");
        // panes must be u8 → string triggers Config deserialization error.
        write(&local, "[session]\npanes = \"two\"\n");
        let config = Config::load_layered(Some(&global), Some(&local));
        assert_eq!(config.session.agent_cmd, "aider");
        assert_eq!(config.session.panes, 3);
    }

    #[test]
    fn test_load_layered_invalid_global_preserves_local() {
        let dir = tempfile::tempdir().unwrap();
        let global = dir.path().join("global.toml");
        let local = dir.path().join("local.toml");
        write(&global, "[session]\nmode = \"invalid\"\n");
        write(&local, "[session]\nagent_cmd = \"aider\"\n");
        let config = Config::load_layered(Some(&global), Some(&local));
        assert_eq!(config.session.agent_cmd, "aider");
        assert_eq!(config.session.mode, SessionMode::Panes);
    }

    #[test]
    fn test_load_layered_both_invalid_returns_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let global = dir.path().join("global.toml");
        let local = dir.path().join("local.toml");
        write(&global, "[session]\npanes = \"two\"\n");
        write(&local, "[session]\nmode = \"invalid\"\n");
        let config = Config::load_layered(Some(&global), Some(&local));
        assert_eq!(config.session.mode, SessionMode::Panes);
        assert_eq!(config.session.panes, 2);
        assert_eq!(config.session.agent_cmd, "claude");
    }
}
