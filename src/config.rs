//! Configuration for serverbrowser.
//!
//! Modeled loosely on qutebrowser's `[aliases]`, keybindings, and general
//! settings, plus output options specific to rendering a browser in a
//! terminal.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// How the rendered page is displayed in the terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputMode {
    /// iTerm2/kitty inline-image protocol. Well-supported, no external deps.
    Kitty,
    /// DEC sixel graphics. Needs a sixel-capable terminal.
    Sixel,
    /// Plain ANSI color blocks (2x4 half-block) — works almost everywhere.
    Blocks,
    /// Extract readable text (no image) as a fallback for dumb terminals.
    Text,
}

impl Default for OutputMode {
    fn default() -> Self {
        OutputMode::Kitty
    }
}

/// A single key binding. `key` is a human-readable key sequence like "j",
/// "gg", "<ctrl>+d", "gt". `command` is the qutebrowser-style action.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyBinding {
    pub key: String,
    pub command: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Default start URL (or "about:blank").
    pub start_page: String,
    /// Terminal output mode.
    pub output_mode: OutputMode,
    /// Viewport size of the headless render, in CSS pixels.
    pub viewport_width: u32,
    pub viewport_height: u32,
    /// Maximum render tab count.
    pub max_tabs: usize,
    /// Key bindings (qutebrowser style). Keyed by mode ("normal", "command").
    pub keybindings: BTreeMap<String, Vec<KeyBinding>>,
    /// Path to the bookmark/mindmap vault (a directory of Markdown nodes).
    pub vault_dir: PathBuf,
    /// Homepage for the mindmap root node.
    pub url_prefixes: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            start_page: "about:blank".to_string(),
            output_mode: OutputMode::Kitty,
            viewport_width: 1280,
            viewport_height: 800,
            max_tabs: 16,
            keybindings: default_keybindings(),
            vault_dir: default_vault_dir(),
            url_prefixes: vec![],
        }
    }
}

impl Config {
    /// Load config from `path` (TOML). Missing file => defaults.
    pub fn load(path: &std::path::Path) -> Result<Self, Box<dyn std::error::Error>> {
        if !path.exists() {
            return Ok(Config::default());
        }
        let text = std::fs::read_to_string(path)?;
        let mut cfg: Config = toml::from_str(&text)?;
        // Fill any missing keybinding maps with defaults.
        if cfg.keybindings.is_empty() {
            cfg.keybindings = default_keybindings();
        }
        Ok(cfg)
    }

    pub fn save(&self, path: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, toml::to_string_pretty(self)?)?;
        Ok(())
    }
}

fn default_vault_dir() -> PathBuf {
    std::env::var("HOME")
        .map(|h| PathBuf::from(h).join(".local/share/serverbrowser/vault"))
        .unwrap_or_else(|_| PathBuf::from(".serverbrowser/vault"))
}

/// Default qutebrowser-inspired keybindings.
pub fn default_keybindings() -> BTreeMap<String, Vec<KeyBinding>> {
    let mut m = BTreeMap::new();
    m.insert(
        "normal".to_string(),
        vec![
            b("j", "scroll down"),
            b("k", "scroll up"),
            b("h", "scroll left"),
            b("l", "scroll right"),
            b("gg", "scroll-to-perc 0"),
            b("G", "scroll-to-perc 100"),
            b("gt", "tab-next"),
            b("gT", "tab-prev"),
            b("d", "tab-close"),
            b("u", "tab-undo"),
            b("r", "reload"),
            b("H", "back"),
            b("L", "forward"),
            b("f", "hint"),
            b(":", "set-cmd-text :"),
            b("o", "set-cmd-text :open "),
            b("O", "set-cmd-text :open -t "),
            b("b", "set-cmd-text :bookmark-add "),
            b("B", "set-cmd-text :bookmarks "),
            b("yy", "yank"),
            b("zz", "zoom 1"),
            b("+", "zoom +0.1"),
            b("-", "zoom -0.1"),
            b("q", "quit"),
        ],
    );
    m
}

fn b(key: &str, command: &str) -> KeyBinding {
    KeyBinding {
        key: key.to_string(),
        command: command.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_have_normal_bindings() {
        let cfg = Config::default();
        assert!(cfg.keybindings.contains_key("normal"));
        assert!(cfg.start_page == "about:blank");
    }

    #[test]
    fn roundtrip_toml() {
        let cfg = Config::default();
        let s = toml::to_string_pretty(&cfg).unwrap();
        let back: Config = toml::from_str(&s).unwrap();
        assert_eq!(back.viewport_width, cfg.viewport_width);
        assert_eq!(back.output_mode, cfg.output_mode);
    }
}