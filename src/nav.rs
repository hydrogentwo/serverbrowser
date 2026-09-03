//! qutebrowser-style navigation: command parsing and key dispatch.
//!
//! Mirrors qutebrowser's model:
//!   - a "normal" mode where key sequences map to commands,
//!   - a "command" mode where `:` prompts accept `:open URL`, `:quit`, etc.
//!
//! This module is intentionally free of terminal I/O so it can be unit-tested;
//! the interactive loop lives in the binary.

use crate::config::{Config, KeyBinding};

/// A parsed command from `:` mode, or a built-in action name from a keybinding.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    Open { url: String, tab: bool },
    OpenUrl { url: String },
    Back,
    Forward,
    Reload,
    Quit,
    Scroll { dx: i32, dy: i32 },
    ScrollToPerc { perc: u32 },
    TabNext,
    TabPrev,
    TabClose,
    TabUndo,
    Hint,
    BookmarkAdd { url: Option<String>, parent: Option<String> },
    Bookmarks,
    Yank,
    Zoom { factor: f32 },
    SetCmdText { text: String },
    /// Key sequence that didn't map to a command.
    Unbound { keys: String },
    /// A `:` command string that didn't parse.
    Unknown { raw: String },
}

/// A navigation input: either a command-mode prompt (`:`...) or a raw key
/// sequence in normal mode.
#[derive(Debug, Clone)]
pub enum Input {
    Command(String),
    Keys(String),
}

/// Result of feeding input through a mode machine.
#[derive(Debug, Clone)]
pub enum Outcome {
    Command(Command),
    /// A partial multi-key prefix that needs more input (e.g. "g" before "g").
    Pending(String),
    Unbound(String),
}

/// A tiny keymap engine with prefix matching (like qutebrowser's `gg`).
pub struct Keymap {
    normal: Vec<KeyBinding>,
    /// Pending prefix (for `g` + `g` style bindings).
    pending: Option<String>,
}

impl Keymap {
    pub fn new(cfg: &Config) -> Self {
        Keymap {
            normal: cfg.keybindings.get("normal").cloned().unwrap_or_default(),
            pending: None,
        }
    }

    /// Feed a key sequence and return the mapped command (or Pending/Unbound).
    pub fn feed(&mut self, keys: &str) -> Outcome {
        // Combine any pending prefix with the new keys.
        let seq = match self.pending.take() {
            Some(p) => format!("{p}{keys}"),
            None => keys.to_string(),
        };

        // Exact match?
        if let Some(b) = self.normal.iter().find(|b| b.key == seq) {
            return Outcome::Command(parse_command_token(&b.command));
        }
        // Is `seq` a prefix of any binding? If so, wait for more.
        if self.normal.iter().any(|b| b.key.starts_with(&seq)) {
            self.pending = Some(seq.clone());
            return Outcome::Pending(seq);
        }
        Outcome::Unbound(seq)
    }

    pub fn reset(&mut self) {
        self.pending = None;
    }
}

/// Parse a `:` command line into a [`Command`].
pub fn parse_command(raw: &str) -> Command {
    let raw = raw.trim().trim_start_matches(':').trim();
    let (cmd, args) = match raw.find(char::is_whitespace) {
        Some(i) => (&raw[..i], raw[i..].trim()),
        None => (raw, ""),
    };
    match cmd {
        "open" | "o" => {
            let (url, tab) = if args.starts_with("-t ") {
                (&args[3..], true)
            } else {
                (args, false)
            };
            Command::Open {
                url: url.to_string(),
                tab,
            }
        }
        "back" | "b" => Command::Back,
        "forward" | "f" => Command::Forward,
        "reload" | "r" => Command::Reload,
        "quit" | "q" | "exit" => Command::Quit,
        "scroll" => {
            let mut dx = 0;
            let mut dy = 0;
            for tok in args.split_whitespace() {
                if let Some(v) = tok.strip_prefix("dx=") {
                    dx = v.parse().unwrap_or(0);
                } else if let Some(v) = tok.strip_prefix("dy=") {
                    dy = v.parse().unwrap_or(0);
                }
            }
            Command::Scroll { dx, dy }
        }
        "scroll-to-perc" | "G" => {
            let perc = args.parse().unwrap_or(100);
            Command::ScrollToPerc { perc }
        }
        "tab-next" | "gt" => Command::TabNext,
        "tab-prev" | "gT" => Command::TabPrev,
        "tab-close" | "d" => Command::TabClose,
        "tab-undo" | "u" => Command::TabUndo,
        "hint" => Command::Hint,
        "bookmark-add" => {
            let url = args.split_whitespace().next().map(|s| s.to_string());
            let parent = args
                .split_whitespace()
                .nth(1)
                .map(|s| s.to_string());
            Command::BookmarkAdd { url, parent }
        }
        "bookmarks" => Command::Bookmarks,
        "yank" => Command::Yank,
        "zoom" => {
            let factor = args.parse().unwrap_or(1.0);
            Command::Zoom { factor }
        }
        "set-cmd-text" => Command::SetCmdText {
            text: args.to_string(),
        },
        "" => Command::SetCmdText {
            text: String::new(),
        },
        other => Command::Unknown {
            raw: other.to_string(),
        },
    }
}

/// Parse a token that appears as the RHS of a keybinding (e.g. "scroll down").
fn parse_command_token(token: &str) -> Command {
    // Reuse parse_command by treating the token as a command name + args.
    match token.split_whitespace().next().unwrap_or("") {
        "scroll" => {
            let dir = token.split_whitespace().nth(1).unwrap_or("");
            let dy = match dir {
                "down" => 200,
                "up" => -200,
                "left" => -200,
                "right" => 200,
                _ => 0,
            };
            let dx = match dir {
                "left" => -100,
                "right" => 100,
                _ => 0,
            };
            Command::Scroll { dx, dy }
        }
        "scroll-to-perc" => {
            let perc = token
                .split_whitespace()
                .nth(1)
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
            Command::ScrollToPerc { perc }
        }
        "tab-next" => Command::TabNext,
        "tab-prev" => Command::TabPrev,
        "tab-close" => Command::TabClose,
        "tab-undo" => Command::TabUndo,
        "reload" => Command::Reload,
        "back" => Command::Back,
        "forward" => Command::Forward,
        "hint" => Command::Hint,
        "set-cmd-text" => Command::SetCmdText {
            text: token
                .strip_prefix("set-cmd-text ")
                .unwrap_or("")
                .to_string(),
        },
        "yank" => Command::Yank,
        "quit" => Command::Quit,
        "zoom" => {
            let arg = token.split_whitespace().nth(1).unwrap_or("1");
            let factor = if arg.starts_with('+') {
                arg[1..].parse::<f32>().unwrap_or(1.0)
            } else if arg.starts_with('-') {
                -arg[1..].parse::<f32>().unwrap_or(0.0)
            } else {
                arg.parse().unwrap_or(1.0)
            };
            Command::Zoom { factor }
        }
        _ => Command::Unknown {
            raw: token.to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_open() {
        assert_eq!(
            parse_command(":open https://example.com"),
            Command::Open {
                url: "https://example.com".into(),
                tab: false
            }
        );
        assert_eq!(
            parse_command(":open -t https://example.com"),
            Command::Open {
                url: "https://example.com".into(),
                tab: true
            }
        );
    }

    #[test]
    fn parse_quit() {
        assert_eq!(parse_command(":quit"), Command::Quit);
    }

    #[test]
    fn keymap_gg() {
        let cfg = Config::default();
        let mut km = Keymap::new(&cfg);
        assert!(matches!(km.feed("g"), Outcome::Pending(_)));
        assert!(matches!(km.feed("g"), Outcome::Command(Command::ScrollToPerc { perc: 0 })));
    }

    #[test]
    fn keymap_j() {
        let cfg = Config::default();
        let mut km = Keymap::new(&cfg);
        assert!(matches!(km.feed("j"), Outcome::Command(Command::Scroll { dy: 200, .. })));
    }
}