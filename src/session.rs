use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use crate::config::Config;
use crate::tmux_manager::TmuxManager;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionState {
    pub session_name: String,
    pub worktrees: HashMap<String, WindowInfo>,
    /// Windows-mode sessions keyed by worktree name. Absent from
    /// pre-windows-mode state files (handled by `serde(default)`).
    #[serde(default)]
    pub windows_sessions: HashMap<String, WindowsSessionInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowInfo {
    pub window_index: u32,
    pub pane_count: u8,
    pub worktree_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowsSessionInfo {
    pub session_name: String,
    pub worktree_path: PathBuf,
}

impl SessionState {
    pub fn new(session_name: &str) -> Self {
        Self {
            session_name: session_name.to_string(),
            worktrees: HashMap::new(),
            windows_sessions: HashMap::new(),
        }
    }

    /// Load existing state, or return a fresh instance if the file is
    /// missing. Every caller that both reads and writes needs this
    /// unwrap, so the helper sits alongside `load()`.
    pub fn load_or_new() -> Result<Self> {
        Ok(Self::load()?.unwrap_or_else(|| Self::new("wt")))
    }

    fn state_file_path() -> Result<PathBuf> {
        let wt_dir = Config::ensure_wt_dir()?;
        Ok(wt_dir.join("sessions.json"))
    }

    /// Load session state from ~/.wt/sessions.json
    pub fn load() -> Result<Option<Self>> {
        let path = Self::state_file_path()?;
        if !path.exists() {
            return Ok(None);
        }

        let contents = std::fs::read_to_string(&path).context("Failed to read sessions.json")?;

        let state: SessionState =
            serde_json::from_str(&contents).context("Failed to parse sessions.json")?;

        Ok(Some(state))
    }

    /// Save session state to ~/.wt/sessions.json
    pub fn save(&self) -> Result<()> {
        let path = Self::state_file_path()?;
        let contents =
            serde_json::to_string_pretty(self).context("Failed to serialize session state")?;

        std::fs::write(&path, contents).context("Failed to write sessions.json")?;

        Ok(())
    }

    /// Add a worktree window to the session
    pub fn add_worktree(&mut self, name: &str, window_index: u32, pane_count: u8, path: PathBuf) {
        self.worktrees.insert(
            name.to_string(),
            WindowInfo {
                window_index,
                pane_count,
                worktree_path: path,
            },
        );
    }

    /// Remove a worktree from the session
    pub fn remove_worktree(&mut self, name: &str) -> Option<WindowInfo> {
        self.worktrees.remove(name)
    }

    /// Get worktree info by name
    pub fn get_worktree(&self, name: &str) -> Option<&WindowInfo> {
        self.worktrees.get(name)
    }

    /// Check if a worktree is in the session
    pub fn has_worktree(&self, name: &str) -> bool {
        self.worktrees.contains_key(name)
    }

    /// Sync session state with actual tmux windows
    /// Removes entries for windows that no longer exist
    pub fn sync_with_tmux(&mut self, tmux: &TmuxManager) -> Result<()> {
        let windows = tmux.list_windows()?;
        let window_names: std::collections::HashSet<_> =
            windows.iter().map(|w| w.name.clone()).collect();

        // Remove worktrees that no longer have windows
        self.worktrees.retain(|name, _| window_names.contains(name));

        // Update pane counts
        for window in &windows {
            if let Some(info) = self.worktrees.get_mut(&window.name) {
                info.pane_count = window.pane_count as u8;
            }
        }

        Ok(())
    }

    /// Clear the session state
    pub fn clear() -> Result<()> {
        let path = Self::state_file_path()?;
        if path.exists() {
            std::fs::remove_file(&path).context("Failed to remove sessions.json")?;
        }
        Ok(())
    }

    /// Whether this state holds no panes-mode or windows-mode entries.
    pub fn is_empty(&self) -> bool {
        self.worktrees.is_empty() && self.windows_sessions.is_empty()
    }

    /// Drop all panes-mode entries, preserving `windows_sessions`.
    /// Needed so a panes-mode cleanup doesn't wipe live windows-mode
    /// associations that happen to share the same state file.
    pub fn clear_panes_state(&mut self) {
        self.worktrees.clear();
    }

    /// Upsert a windows-mode session association.
    pub fn add_windows_session(&mut self, worktree: &str, info: WindowsSessionInfo) {
        self.windows_sessions.insert(worktree.to_string(), info);
    }

    /// Remove a windows-mode session association, returning its info if
    /// present.
    pub fn remove_windows_session(&mut self, worktree: &str) -> Option<WindowsSessionInfo> {
        self.windows_sessions.remove(worktree)
    }

    /// Drop stale `windows_sessions` entries whose tmux session is no
    /// longer live. Returns true when anything was removed — callers use
    /// it to skip a needless save.
    pub fn prune_windows_sessions(&mut self, live: &HashSet<String>) -> bool {
        let before = self.windows_sessions.len();
        retain_live_sessions(&mut self.windows_sessions, live);
        before != self.windows_sessions.len()
    }
}

/// Drop entries whose `session_name` is not in `live`. Pure helper so
/// the pruning logic is unit-testable without touching tmux.
pub fn retain_live_sessions(
    entries: &mut HashMap<String, WindowsSessionInfo>,
    live: &HashSet<String>,
) {
    entries.retain(|_, info| live.contains(&info.session_name));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_state_new() {
        let state = SessionState::new("wt");
        assert_eq!(state.session_name, "wt");
        assert!(state.worktrees.is_empty());
    }

    #[test]
    fn test_add_remove_worktree() {
        let mut state = SessionState::new("wt");
        state.add_worktree("feature-1", 1, 2, PathBuf::from("/path/to/feature-1"));

        assert!(state.has_worktree("feature-1"));
        assert!(!state.has_worktree("feature-2"));

        let info = state.get_worktree("feature-1").unwrap();
        assert_eq!(info.window_index, 1);
        assert_eq!(info.pane_count, 2);

        state.remove_worktree("feature-1");
        assert!(!state.has_worktree("feature-1"));
    }

    #[test]
    fn test_serialize_deserialize() {
        let mut state = SessionState::new("wt");
        state.add_worktree("feature-1", 1, 3, PathBuf::from("/path/to/feature-1"));

        let json = serde_json::to_string(&state).unwrap();
        let loaded: SessionState = serde_json::from_str(&json).unwrap();

        assert_eq!(loaded.session_name, "wt");
        assert!(loaded.has_worktree("feature-1"));
    }

    fn windows_info(session: &str, path: &str) -> WindowsSessionInfo {
        WindowsSessionInfo {
            session_name: session.to_string(),
            worktree_path: PathBuf::from(path),
        }
    }

    #[test]
    fn test_add_remove_windows_session() {
        let mut state = SessionState::new("wt");
        let info = windows_info("wt-feature", "/p/feature");
        state.add_windows_session("feature", info.clone());
        assert_eq!(state.windows_sessions.get("feature"), Some(&info));
        assert_eq!(state.remove_windows_session("feature"), Some(info));
        assert!(state.windows_sessions.is_empty());
    }

    #[test]
    fn test_windows_session_serde_round_trip() {
        let mut state = SessionState::new("wt");
        state.add_windows_session("feature", windows_info("wt-feature", "/p/feature"));
        let json = serde_json::to_string(&state).unwrap();
        let loaded: SessionState = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.windows_sessions, state.windows_sessions);
    }

    #[test]
    fn test_deserialize_legacy_state_without_windows_sessions() {
        let legacy = r#"{"session_name":"wt","worktrees":{}}"#;
        let state: SessionState = serde_json::from_str(legacy).unwrap();
        assert_eq!(state.session_name, "wt");
        assert!(state.windows_sessions.is_empty());
    }

    #[test]
    fn test_retain_live_sessions_drops_stale_entries() {
        let mut entries = HashMap::new();
        entries.insert("alive".to_string(), windows_info("wt-alive", "/p/alive"));
        entries.insert("stale".to_string(), windows_info("wt-stale", "/p/stale"));
        let live: HashSet<String> = ["wt-alive".to_string()].into_iter().collect();
        retain_live_sessions(&mut entries, &live);
        assert_eq!(entries.len(), 1);
        assert!(entries.contains_key("alive"));
    }

    #[test]
    fn test_retain_live_sessions_empty_live_set_clears_all() {
        let mut entries = HashMap::new();
        entries.insert("foo".to_string(), windows_info("wt-foo", "/p/foo"));
        retain_live_sessions(&mut entries, &HashSet::new());
        assert!(entries.is_empty());
    }

    #[test]
    fn test_prune_windows_sessions_returns_true_only_when_changed() {
        let mut state = SessionState::new("wt");
        state.add_windows_session("alive", windows_info("wt-alive", "/p/alive"));
        state.add_windows_session("stale", windows_info("wt-stale", "/p/stale"));
        let live: HashSet<String> = ["wt-alive".to_string()].into_iter().collect();

        assert!(state.prune_windows_sessions(&live));
        // Second call is a no-op because the stale entry is already gone.
        assert!(!state.prune_windows_sessions(&live));
    }

    #[test]
    fn test_clear_panes_state_preserves_windows_sessions() {
        let mut state = SessionState::new("wt");
        state.add_worktree("feat", 1, 2, PathBuf::from("/p/feat"));
        state.add_windows_session("other", windows_info("wt-other", "/p/other"));
        state.clear_panes_state();
        assert!(state.worktrees.is_empty());
        assert!(state.windows_sessions.contains_key("other"));
        assert!(!state.is_empty());
    }

    #[test]
    fn test_is_empty_tracks_both_halves() {
        let mut state = SessionState::new("wt");
        assert!(state.is_empty());
        state.add_worktree("feat", 1, 2, PathBuf::from("/p/feat"));
        assert!(!state.is_empty());
        state.clear_panes_state();
        assert!(state.is_empty());
    }
}
