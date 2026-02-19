//! TOML-based configuration system.
//!
//! Loads settings from a `config.toml` file, falling back to sensible defaults
//! that match the original hardcoded values. Every struct implements `Default`
//! so a missing or partial config file produces the same behavior as before.
//!
//! ## Config file search order
//!
//! 1. `SURIBROWS_CONFIG` environment variable (explicit override)
//! 2. Next to the executable (`<exe_dir>/config.toml`)
//! 3. Platform config directory (`%APPDATA%\SuriBrows\config.toml` on Windows)
//! 4. Current working directory (`./config.toml`)
//! 5. No file found → `Config::default()`

use std::fs;
use std::io;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tracing::{info, warn};

// ─────────────────────────────────────────────────────────────────────────────
// Config structs
// ─────────────────────────────────────────────────────────────────────────────

/// Top-level configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub general: GeneralConfig,
    pub window: WindowConfig,
    pub chrome: ChromeConfig,
    pub search: SearchConfig,
    pub servo: ServoConfig,
    pub privacy: PrivacyConfig,
}

/// General application settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GeneralConfig {
    pub default_url: String,
    pub window_title: String,
}

/// Window dimensions.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct WindowConfig {
    pub width: u32,
    pub height: u32,
}

/// Chrome (URL bar area) appearance.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ChromeConfig {
    pub height: u32,
    pub font_size: f32,
    pub text_left_pad: f32,
    pub bar_margin: f32,
    pub bar_h_pad: f32,
    pub colors: ChromeColors,
}

/// RGBA colors for the chrome UI (values 0.0–1.0).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ChromeColors {
    pub background: [f32; 4],
    pub background_focused: [f32; 4],
    pub text: [f32; 4],
    pub cursor: [f32; 4],
    pub bar_background: [f32; 4],
    pub bar_border: [f32; 4],
}

/// Search engine configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SearchConfig {
    pub engine_url: String,
}

/// Servo engine performance tuning.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ServoConfig {
    /// Maximum layout threads. 0 = auto-detect from CPU count.
    pub layout_threads: i64,
    /// HTTP cache size in bytes.
    pub cache_size: i64,
    /// User-agent string. Empty = default privacy UA.
    pub user_agent: String,
    /// Pre-cache GPU shaders at startup.
    pub precache_shaders: bool,
}

/// Privacy and security toggles.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PrivacyConfig {
    pub enforce_tls: bool,
    pub disable_mime_sniff: bool,
    pub disable_geolocation: bool,
    pub disable_bluetooth: bool,
    pub disable_notifications: bool,
    pub disable_webrtc: bool,
}

// ─────────────────────────────────────────────────────────────────────────────
// Default impls — match original hardcoded values exactly
// ─────────────────────────────────────────────────────────────────────────────

// Config derives Default since all fields implement Default.
// (Other structs have custom defaults with non-zero values.)

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            default_url: "https://example.com".to_string(),
            window_title: "SuriBrows".to_string(),
        }
    }
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            width: 1280,
            height: 800,
        }
    }
}

impl Default for ChromeConfig {
    fn default() -> Self {
        Self {
            height: 40,
            font_size: 16.0,
            text_left_pad: 12.0,
            bar_margin: 6.0,
            bar_h_pad: 8.0,
            colors: ChromeColors::default(),
        }
    }
}

impl Default for ChromeColors {
    fn default() -> Self {
        Self {
            background: [0.17, 0.17, 0.17, 1.0],
            background_focused: [0.23, 0.23, 0.23, 1.0],
            text: [0.93, 0.93, 0.93, 1.0],
            cursor: [1.0, 1.0, 1.0, 1.0],
            bar_background: [0.13, 0.13, 0.13, 1.0],
            bar_border: [0.3, 0.3, 0.3, 1.0],
        }
    }
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            engine_url: "https://duckduckgo.com/?q=".to_string(),
        }
    }
}

impl Default for ServoConfig {
    fn default() -> Self {
        Self {
            layout_threads: 0,
            cache_size: 50_000,
            user_agent: String::new(),
            precache_shaders: true,
        }
    }
}

impl Default for PrivacyConfig {
    fn default() -> Self {
        Self {
            enforce_tls: true,
            disable_mime_sniff: true,
            disable_geolocation: true,
            disable_bluetooth: true,
            disable_notifications: true,
            disable_webrtc: true,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Config loading and saving
// ─────────────────────────────────────────────────────────────────────────────

impl Config {
    /// Loads configuration from a TOML file. Never panics — returns defaults
    /// if no file is found or if parsing fails.
    pub fn load() -> Self {
        match find_config_path() {
            Some(path) => match fs::read_to_string(&path) {
                Ok(content) => match toml::from_str::<Config>(&content) {
                    Ok(config) => {
                        info!(path = %path.display(), "Configuration loaded");
                        config
                    }
                    Err(e) => {
                        warn!(path = %path.display(), error = %e, "Invalid config, using defaults");
                        Config::default()
                    }
                },
                Err(e) => {
                    warn!(path = %path.display(), error = %e, "Cannot read config, using defaults");
                    Config::default()
                }
            },
            None => {
                info!("No config file found, using defaults");
                Config::default()
            }
        }
    }

    /// Saves configuration to the platform config directory.
    /// Creates the directory if it doesn't exist.
    pub fn save(&self) -> io::Result<()> {
        let path = save_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let content = toml::to_string_pretty(self).map_err(io::Error::other)?;
        fs::write(&path, content)?;
        info!(path = %path.display(), "Configuration saved");
        Ok(())
    }
}

/// Searches for a config file in the standard locations.
fn find_config_path() -> Option<PathBuf> {
    // 1. Explicit env var override
    if let Ok(path) = std::env::var("SURIBROWS_CONFIG") {
        let p = PathBuf::from(path);
        if p.is_file() {
            return Some(p);
        }
    }

    // 2. Next to the executable
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        let p = dir.join("config.toml");
        if p.is_file() {
            return Some(p);
        }
    }

    // 3. Platform config directory
    let platform_dir = platform_config_dir();
    if let Some(dir) = platform_dir {
        let p = dir.join("config.toml");
        if p.is_file() {
            return Some(p);
        }
    }

    // 4. Current working directory
    let p = PathBuf::from("config.toml");
    if p.is_file() {
        return Some(p);
    }

    None
}

/// Returns the platform-specific save path for the config file.
fn save_path() -> PathBuf {
    platform_config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("config.toml")
}

/// Returns the platform config directory without adding a dependency.
fn platform_config_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        std::env::var("APPDATA")
            .ok()
            .map(|appdata| PathBuf::from(appdata).join("SuriBrows"))
    }
    #[cfg(not(windows))]
    {
        std::env::var("XDG_CONFIG_HOME")
            .ok()
            .or_else(|| std::env::var("HOME").ok().map(|h| format!("{h}/.config")))
            .map(|dir| PathBuf::from(dir).join("suribrows"))
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Query param serialization (for HTML settings save)
// ─────────────────────────────────────────────────────────────────────────────

impl Config {
    /// Builds a Config from URL query parameters (key=value&key=value).
    /// Unknown keys are silently ignored; missing keys use defaults.
    pub fn from_query_params(query: &str) -> Self {
        let mut config = Config::default();

        for pair in query.split('&') {
            let mut parts = pair.splitn(2, '=');
            let key = parts.next().unwrap_or("");
            let value = parts.next().unwrap_or("");
            let value = url_decode(value);

            match key {
                "default_url" => config.general.default_url = value,
                "window_title" => config.general.window_title = value,
                "window_width" => {
                    if let Ok(v) = value.parse() {
                        config.window.width = v;
                    }
                }
                "window_height" => {
                    if let Ok(v) = value.parse() {
                        config.window.height = v;
                    }
                }
                "chrome_height" => {
                    if let Ok(v) = value.parse() {
                        config.chrome.height = v;
                    }
                }
                "font_size" => {
                    if let Ok(v) = value.parse() {
                        config.chrome.font_size = v;
                    }
                }
                "search_engine_url" => config.search.engine_url = value,
                "layout_threads" => {
                    if let Ok(v) = value.parse() {
                        config.servo.layout_threads = v;
                    }
                }
                "cache_size" => {
                    if let Ok(v) = value.parse() {
                        config.servo.cache_size = v;
                    }
                }
                "user_agent" => config.servo.user_agent = value,
                "precache_shaders" => config.servo.precache_shaders = value == "true",
                "enforce_tls" => config.privacy.enforce_tls = value == "true",
                "disable_mime_sniff" => config.privacy.disable_mime_sniff = value == "true",
                "disable_geolocation" => config.privacy.disable_geolocation = value == "true",
                "disable_bluetooth" => config.privacy.disable_bluetooth = value == "true",
                "disable_notifications" => config.privacy.disable_notifications = value == "true",
                "disable_webrtc" => config.privacy.disable_webrtc = value == "true",
                _ => {}
            }
        }

        config
    }
}

/// Minimal percent-decoding for URL query values.
fn url_decode(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.bytes();
    while let Some(b) = chars.next() {
        match b {
            b'+' => result.push(' '),
            b'%' => {
                let hi = chars.next().and_then(hex_val);
                let lo = chars.next().and_then(hex_val);
                if let (Some(h), Some(l)) = (hi, lo) {
                    result.push((h << 4 | l) as char);
                }
            }
            _ => result.push(b as char),
        }
    }
    result
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_matches_original_values() {
        let c = Config::default();
        assert_eq!(c.general.default_url, "https://example.com");
        assert_eq!(c.general.window_title, "SuriBrows");
        assert_eq!(c.window.width, 1280);
        assert_eq!(c.window.height, 800);
        assert_eq!(c.chrome.height, 40);
        assert_eq!(c.chrome.font_size, 16.0);
        assert_eq!(c.search.engine_url, "https://duckduckgo.com/?q=");
        assert_eq!(c.servo.cache_size, 50_000);
        assert!(c.servo.user_agent.is_empty());
        assert!(c.privacy.enforce_tls);
        assert!(c.privacy.disable_webrtc);
    }

    #[test]
    fn test_empty_toml_returns_defaults() {
        let config: Config = toml::from_str("").unwrap();
        assert_eq!(config.window.width, 1280);
        assert_eq!(config.chrome.height, 40);
        assert!(config.privacy.enforce_tls);
    }

    #[test]
    fn test_partial_toml_fills_defaults() {
        let toml = r#"
[window]
width = 1920
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.window.width, 1920);
        assert_eq!(config.window.height, 800); // default
        assert_eq!(config.chrome.height, 40); // default
    }

    #[test]
    fn test_color_arrays_parse() {
        let toml = r#"
[chrome.colors]
background = [0.1, 0.2, 0.3, 1.0]
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.chrome.colors.background, [0.1, 0.2, 0.3, 1.0]);
        // Other colors remain default
        assert_eq!(config.chrome.colors.text, [0.93, 0.93, 0.93, 1.0]);
    }

    #[test]
    fn test_full_toml_roundtrip() {
        let config = Config::default();
        let serialized = toml::to_string_pretty(&config).unwrap();
        let deserialized: Config = toml::from_str(&serialized).unwrap();
        assert_eq!(deserialized.window.width, config.window.width);
        assert_eq!(deserialized.chrome.height, config.chrome.height);
        assert_eq!(deserialized.search.engine_url, config.search.engine_url);
    }

    #[test]
    fn test_from_query_params_basic() {
        let config = Config::from_query_params("window_width=1920&window_height=1080");
        assert_eq!(config.window.width, 1920);
        assert_eq!(config.window.height, 1080);
        assert_eq!(config.chrome.height, 40); // untouched
    }

    #[test]
    fn test_from_query_params_booleans() {
        let config = Config::from_query_params("enforce_tls=false&disable_webrtc=true");
        assert!(!config.privacy.enforce_tls);
        assert!(config.privacy.disable_webrtc);
    }

    #[test]
    fn test_from_query_params_url_encoded() {
        let config =
            Config::from_query_params("search_engine_url=https%3A%2F%2Fgoogle.com%2F%3Fq%3D");
        assert_eq!(config.search.engine_url, "https://google.com/?q=");
    }

    #[test]
    fn test_from_query_params_unknown_keys_ignored() {
        let config = Config::from_query_params("unknown_key=value&window_width=999");
        assert_eq!(config.window.width, 999);
    }

    #[test]
    fn test_url_decode() {
        assert_eq!(url_decode("hello+world"), "hello world");
        assert_eq!(
            url_decode("https%3A%2F%2Fexample.com"),
            "https://example.com"
        );
        assert_eq!(url_decode("no+encoding+needed"), "no encoding needed");
    }

    #[test]
    fn test_save_path_not_empty() {
        let path = save_path();
        assert!(!path.as_os_str().is_empty());
    }
}
