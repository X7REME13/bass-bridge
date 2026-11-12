use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Settings {
    pub input_device: Option<String>,
    pub output_device: Option<String>,
    pub enabled: bool,
    pub cutoff_hz: f32,
    pub gain_db: f32,
    pub delay_ms: f32,
    pub stereo: bool,
    #[serde(default)]
    pub phase_inverted: bool,
    #[serde(default)]
    pub bypass: bool,
    #[serde(default = "default_true")]
    pub soft_clip: bool,
    #[serde(default = "default_true")]
    pub direct_sum: bool,
}

fn default_true() -> bool {
    true
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            input_device: None,
            output_device: None,
            enabled: false,
            cutoff_hz: 90.0,
            gain_db: 6.0,
            delay_ms: 0.0,
            stereo: false,
            phase_inverted: false,
            bypass: false,
            soft_clip: true,
            direct_sum: true,
        }
    }
}

impl Settings {
    fn config_path() -> PathBuf {
        if let Some(mut dir) = dirs_config_dir() {
            dir.push("BassBridge");
            let _ = fs::create_dir_all(&dir);
            dir.push("config.json");
            dir
        } else {
            PathBuf::from("bass_bridge_config.json")
        }
    }

    pub fn load() -> Self {
        let path = Self::config_path();
        if let Ok(data) = fs::read_to_string(&path) {
            if let Ok(settings) = serde_json::from_str::<Settings>(&data) {
                return settings;
            }
        }
        Self::default()
    }

    pub fn save(&self) {
        let path = Self::config_path();
        if let Ok(data) = serde_json::to_string_pretty(self) {
            let _ = fs::write(&path, data);
        }
    }
}

fn dirs_config_dir() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("APPDATA").map(PathBuf::from)
    }
    #[cfg(not(target_os = "windows"))]
    {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
    }
}
