use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub theme: String,
    pub font_family: String,
    pub font_size: u16,
    pub scrollback: u32,
    pub cursor_style: String,
    pub cursor_blink: bool,
    pub copy_on_select: bool,
    /// Minutes of inactivity before the vault locks itself (0 = never).
    pub auto_lock_minutes: u32,
    pub local_shell: String,
    pub docker_log_tail: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            theme: "shellport-dark".into(),
            font_family: "'JetBrains Mono', 'Fira Code', 'DejaVu Sans Mono', monospace".into(),
            font_size: 14,
            scrollback: 10000,
            cursor_style: "block".into(),
            cursor_blink: true,
            copy_on_select: false,
            auto_lock_minutes: 15,
            local_shell: std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".into()),
            docker_log_tail: 500,
        }
    }
}

impl Settings {
    pub fn load(path: &Path) -> Settings {
        std::fs::read(path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(d) = path.parent() {
            std::fs::create_dir_all(d)?;
        }
        crate::paths::write_private(path, &serde_json::to_vec_pretty(self)?)?;
        Ok(())
    }
}
