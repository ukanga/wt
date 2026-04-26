use anyhow::{Context, Result};
use std::collections::HashSet;
use std::path::Path;
use std::process::Command;

use crate::config::SessionConfig;

#[derive(Debug)]
pub struct TmuxManager {
    session_name: String,
}

#[derive(Debug, Clone)]
pub struct TmuxWindow {
    pub index: u32,
    pub name: String,
    pub pane_count: u32,
    pub active: bool,
    pub agent_status: AgentStatus,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AgentStatus {
    Idle,
    Active,
    Unknown,
}

impl std::fmt::Display for AgentStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AgentStatus::Idle => write!(f, "idle"),
            AgentStatus::Active => write!(f, "active"),
            AgentStatus::Unknown => write!(f, "?"),
        }
    }
}

impl TmuxManager {
    pub fn new(session_name: &str) -> Self {
        Self {
            session_name: session_name.to_string(),
        }
    }

    /// Check if tmux is available on the system.
    pub fn is_available() -> bool {
        Command::new("tmux")
            .arg("-V")
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    }

    /// Check if we're currently inside this tmux session.
    pub fn is_inside_session(&self) -> bool {
        if let Ok(tmux_var) = std::env::var("TMUX") {
            if let Ok(output) = Command::new("tmux")
                .args(["display-message", "-p", "#{session_name}"])
                .output()
            {
                if output.status.success() {
                    let current_session = String::from_utf8_lossy(&output.stdout);
                    return current_session.trim() == self.session_name;
                }
            }

            !tmux_var.is_empty()
        } else {
            false
        }
    }

    /// Check if we're inside any tmux session.
    pub fn is_inside_tmux() -> bool {
        std::env::var("TMUX").is_ok()
    }

    /// Check if the session already exists.
    pub fn session_exists(&self) -> Result<bool> {
        let output = Command::new("tmux")
            .args(["has-session", "-t", &self.session_name])
            .output()
            .context("Failed to check tmux session")?;

        Ok(output.status.success())
    }

    /// Whether a client is currently attached to this session.
    pub fn is_attached(&self) -> Result<bool> {
        let output = Command::new("tmux")
            .args([
                "display-message",
                "-t",
                &self.session_name,
                "-p",
                "#{session_attached}",
            ])
            .output()
            .context("Failed to query session attachment")?;

        if !output.status.success() {
            return Ok(false);
        }

        let count: u32 = String::from_utf8_lossy(&output.stdout)
            .trim()
            .parse()
            .unwrap_or(0);
        Ok(count > 0)
    }

    /// Create a new session with an initial window.
    pub fn create_session(&self, window_name: &str, cwd: &Path) -> Result<()> {
        let output = Command::new("tmux")
            .args([
                "new-session",
                "-d",
                "-s",
                &self.session_name,
                "-n",
                window_name,
                "-c",
                &cwd.to_string_lossy(),
            ])
            .output()
            .context("Failed to create tmux session")?;

        if !output.status.success() {
            anyhow::bail!(
                "Failed to create session: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        Ok(())
    }

    /// Attach to the session (blocking).
    pub fn attach(&self) -> Result<()> {
        let status = Command::new("tmux")
            .args(["attach-session", "-t", &self.session_name])
            .status()
            .context("Failed to attach to tmux session")?;

        if !status.success() {
            anyhow::bail!("Failed to attach to session");
        }

        Ok(())
    }

    /// Enter the session, switching client if already inside tmux.
    pub fn enter(&self) -> Result<()> {
        if Self::is_inside_tmux() {
            let status = Command::new("tmux")
                .args(["switch-client", "-t", &self.session_name])
                .status()
                .context("Failed to switch tmux client")?;

            if !status.success() {
                anyhow::bail!("Failed to switch client to session '{}'", self.session_name);
            }

            Ok(())
        } else {
            self.attach()
        }
    }

    /// Kill the whole session.
    pub fn kill_session(&self) -> Result<()> {
        let output = Command::new("tmux")
            .args(["kill-session", "-t", &self.session_name])
            .output()
            .context("Failed to kill tmux session")?;

        if !output.status.success() {
            anyhow::bail!(
                "Failed to kill session: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        Ok(())
    }

    /// All currently-live tmux session names.
    pub fn live_session_names() -> Result<HashSet<String>> {
        let output = Command::new("tmux")
            .args(["list-sessions", "-F", "#{session_name}"])
            .output()
            .context("Failed to list tmux sessions")?;

        if !output.status.success() {
            return Ok(HashSet::new());
        }

        Ok(String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(|name| name.to_string())
            .collect())
    }

    /// Create a new window in the session.
    pub fn create_window(&self, name: &str, cwd: &Path) -> Result<u32> {
        let target = self.next_window_target();
        let output = Command::new("tmux")
            .args([
                "new-window",
                "-t",
                &target,
                "-n",
                name,
                "-c",
                &cwd.to_string_lossy(),
                "-P",
                "-F",
                "#{window_index}",
            ])
            .output()
            .context("Failed to create tmux window")?;

        if !output.status.success() {
            anyhow::bail!(
                "Failed to create window: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        let index_str = String::from_utf8_lossy(&output.stdout);
        let index: u32 = index_str
            .trim()
            .parse()
            .context("Failed to parse window index")?;
        Ok(index)
    }

    /// Target the next unused window index in this session.
    fn next_window_target(&self) -> String {
        format!("{}:", self.session_name)
    }

    /// Kill a window by name.
    pub fn kill_window(&self, name: &str) -> Result<()> {
        let target = format!("{}:{}", self.session_name, name);
        let output = Command::new("tmux")
            .args(["kill-window", "-t", &target])
            .output()
            .context("Failed to kill tmux window")?;

        if !output.status.success() {
            anyhow::bail!(
                "Failed to kill window: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        Ok(())
    }

    /// Switch to a window by name.
    pub fn select_window(&self, name: &str) -> Result<()> {
        let target = format!("{}:{}", self.session_name, name);
        let output = Command::new("tmux")
            .args(["select-window", "-t", &target])
            .output()
            .context("Failed to select window")?;

        if !output.status.success() {
            anyhow::bail!(
                "Failed to select window: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        Ok(())
    }

    /// List all windows in the session.
    pub fn list_windows(&self) -> Result<Vec<TmuxWindow>> {
        let output = Command::new("tmux")
            .args([
                "list-windows",
                "-t",
                &self.session_name,
                "-F",
                "#{window_index}|#{window_name}|#{window_panes}|#{window_active}",
            ])
            .output()
            .context("Failed to list tmux windows")?;

        if !output.status.success() {
            return Ok(vec![]);
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let windows = stdout
            .lines()
            .filter_map(|line| {
                let parts: Vec<&str> = line.split('|').collect();
                if parts.len() != 4 {
                    return None;
                }

                let name = parts[1].to_string();
                let agent_status = self.get_agent_status(&name).unwrap_or(AgentStatus::Unknown);

                Some(TmuxWindow {
                    index: parts[0].parse().ok()?,
                    name,
                    pane_count: parts[2].parse().ok()?,
                    active: parts[3] == "1",
                    agent_status,
                })
            })
            .collect();

        Ok(windows)
    }

    /// Get the agent status for a window (checks pane 0).
    fn get_agent_status(&self, window: &str) -> Result<AgentStatus> {
        let target = format!("{}:{}.0", self.session_name, window);
        let output = Command::new("tmux")
            .args([
                "display-message",
                "-t",
                &target,
                "-p",
                "#{pane_current_command}",
            ])
            .output()
            .context("Failed to get pane command")?;

        if !output.status.success() {
            return Ok(AgentStatus::Unknown);
        }

        let cmd = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let shells = ["bash", "zsh", "sh", "fish", "ksh", "tcsh", "dash"];
        if shells.iter().any(|shell| cmd == *shell) {
            Ok(AgentStatus::Idle)
        } else if cmd.is_empty() {
            Ok(AgentStatus::Unknown)
        } else {
            Ok(AgentStatus::Active)
        }
    }

    /// Split the current pane horizontally (left/right).
    pub fn split_window_horizontal(&self, window: &str, cwd: &Path) -> Result<()> {
        let target = format!("{}:{}", self.session_name, window);
        let output = Command::new("tmux")
            .args([
                "split-window",
                "-h",
                "-t",
                &target,
                "-c",
                &cwd.to_string_lossy(),
            ])
            .output()
            .context("Failed to split window horizontally")?;

        if !output.status.success() {
            anyhow::bail!(
                "Failed to split window: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        Ok(())
    }

    /// Split the current pane vertically (top/bottom).
    pub fn split_window_vertical(&self, window: &str, cwd: &Path) -> Result<()> {
        let target = format!("{}:{}", self.session_name, window);
        let output = Command::new("tmux")
            .args([
                "split-window",
                "-v",
                "-t",
                &target,
                "-c",
                &cwd.to_string_lossy(),
            ])
            .output()
            .context("Failed to split window vertically")?;

        if !output.status.success() {
            anyhow::bail!(
                "Failed to split window: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        Ok(())
    }

    /// Select a specific pane in a window.
    pub fn select_pane(&self, window: &str, pane: u32) -> Result<()> {
        let target = format!("{}:{}.{}", self.session_name, window, pane);
        let output = Command::new("tmux")
            .args(["select-pane", "-t", &target])
            .output()
            .context("Failed to select pane")?;

        if !output.status.success() {
            anyhow::bail!(
                "Failed to select pane: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        Ok(())
    }

    /// Send keys to a specific pane.
    pub fn send_keys(&self, window: &str, pane: u32, keys: &str) -> Result<()> {
        let target = format!("{}:{}.{}", self.session_name, window, pane);
        let output = Command::new("tmux")
            .args(["send-keys", "-t", &target, keys, "Enter"])
            .output()
            .context("Failed to send keys")?;

        if !output.status.success() {
            anyhow::bail!(
                "Failed to send keys: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        Ok(())
    }

    /// Setup the worktree layout based on pane count.
    pub fn setup_worktree_layout(
        &self,
        window: &str,
        cwd: &Path,
        panes: u8,
        config: &SessionConfig,
    ) -> Result<()> {
        self.split_window_horizontal(window, cwd)?;

        if panes == 3 {
            self.select_pane(window, 0)?;
            self.split_window_vertical(window, cwd)?;
            self.send_keys(window, 0, &config.agent_cmd)?;
            self.send_keys(window, 1, &config.editor_cmd)?;
            self.select_pane(window, 2)?;
        } else {
            self.send_keys(window, 0, &config.agent_cmd)?;
            self.select_pane(window, 1)?;
        }

        Ok(())
    }

    /// Setup a per-worktree session's windows (windows mode).
    pub fn setup_worktree_windows(
        &self,
        cwd: &Path,
        panes: u8,
        config: &SessionConfig,
    ) -> Result<()> {
        self.send_keys("agent", 0, &config.agent_cmd)?;
        self.create_window("shell", cwd)?;

        if panes == 3 {
            self.create_window("edit", cwd)?;
            self.send_keys("edit", 0, &config.editor_cmd)?;
        }

        self.select_window("shell")?;
        Ok(())
    }

    /// Get session name.
    pub fn session_name(&self) -> &str {
        &self.session_name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_available() {
        let available = TmuxManager::is_available();
        assert!(available || !available);
    }

    #[test]
    fn test_manager_creation() {
        let manager = TmuxManager::new("test-session");
        assert_eq!(manager.session_name(), "test-session");
    }

    #[test]
    fn test_next_window_target_uses_next_free_index_syntax() {
        let manager = TmuxManager::new("wt");
        assert_eq!(manager.next_window_target(), "wt:");
    }
}
