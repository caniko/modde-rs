#![allow(clippy::wildcard_imports)]
//! Keyboard shortcut definitions for modde.
//!
//! Shortcuts are defined as static mappings. The app's subscription
//! system calls `handle_key_event()` to convert key events into Messages.

use iced::keyboard::{Key, Modifiers, key::Named};

/// A keyboard shortcut definition.
#[derive(Debug, Clone)]
pub struct Shortcut {
    pub key: Key,
    pub modifiers: Modifiers,
    pub description: &'static str,
    pub action: &'static str, // human-readable action name
}

/// All registered shortcuts.
#[must_use]
pub fn all_shortcuts() -> Vec<Shortcut> {
    vec![
        Shortcut {
            key: Key::Character("d".into()),
            modifiers: Modifiers::CTRL,
            description: "Deploy mods",
            action: "deploy",
        },
        Shortcut {
            key: Key::Character("f".into()),
            modifiers: Modifiers::CTRL,
            description: "Focus filter",
            action: "focus_filter",
        },
        Shortcut {
            key: Key::Named(Named::F5),
            modifiers: Modifiers::empty(),
            description: "Refresh",
            action: "refresh",
        },
        Shortcut {
            key: Key::Named(Named::Delete),
            modifiers: Modifiers::empty(),
            description: "Remove selected mod",
            action: "remove_selected",
        },
        Shortcut {
            key: Key::Named(Named::Space),
            modifiers: Modifiers::empty(),
            description: "Toggle selected mod",
            action: "toggle_selected",
        },
        Shortcut {
            key: Key::Character("s".into()),
            modifiers: Modifiers::CTRL,
            description: "Save settings",
            action: "save_settings",
        },
        Shortcut {
            key: Key::Character("i".into()),
            modifiers: Modifiers::CTRL,
            description: "Open mod info",
            action: "open_info",
        },
        Shortcut {
            key: Key::Character("e".into()),
            modifiers: Modifiers::CTRL,
            description: "Export mod list",
            action: "export",
        },
        Shortcut {
            key: Key::Named(Named::Escape),
            modifiers: Modifiers::empty(),
            description: "Dismiss modal / cancel",
            action: "dismiss_modal",
        },
    ]
}

/// Match a key event against registered shortcuts.
/// Returns the action name if a shortcut matches, None otherwise.
#[must_use]
pub fn match_shortcut(key: &Key, modifiers: Modifiers) -> Option<&'static str> {
    for shortcut in all_shortcuts() {
        if keys_match(key, &shortcut.key) && modifiers == shortcut.modifiers {
            return Some(shortcut.action);
        }
    }
    None
}

fn keys_match(a: &Key, b: &Key) -> bool {
    match (a, b) {
        (Key::Character(a), Key::Character(b)) => a == b,
        (Key::Named(a), Key::Named(b)) => a == b,
        _ => false,
    }
}

/// Format all shortcuts as a human-readable help string.
#[must_use]
pub fn help_text() -> String {
    let mut lines = vec!["Keyboard Shortcuts:".to_string(), String::new()];
    for s in all_shortcuts() {
        let mod_str = format_modifiers(s.modifiers);
        let key_str = format_key(&s.key);
        let combo = if mod_str.is_empty() {
            key_str
        } else {
            format!("{mod_str}+{key_str}")
        };
        lines.push(format!("  {:<20} {}", combo, s.description));
    }
    lines.join("\n")
}

fn format_modifiers(m: Modifiers) -> String {
    let mut parts = Vec::new();
    if m.control() {
        parts.push("Ctrl");
    }
    if m.shift() {
        parts.push("Shift");
    }
    if m.alt() {
        parts.push("Alt");
    }
    parts.join("+")
}

fn format_key(key: &Key) -> String {
    match key {
        Key::Character(c) => c.to_uppercase(),
        Key::Named(n) => format!("{n:?}"),
        _ => "?".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_match_ctrl_d() {
        let key = Key::Character("d".into());
        let result = match_shortcut(&key, Modifiers::CTRL);
        assert_eq!(result, Some("deploy"));
    }

    #[test]
    fn test_match_f5() {
        let key = Key::Named(Named::F5);
        let result = match_shortcut(&key, Modifiers::empty());
        assert_eq!(result, Some("refresh"));
    }

    #[test]
    fn test_match_escape() {
        let key = Key::Named(Named::Escape);
        let result = match_shortcut(&key, Modifiers::empty());
        assert_eq!(result, Some("dismiss_modal"));
    }

    #[test]
    fn test_no_match() {
        let key = Key::Character("z".into());
        let result = match_shortcut(&key, Modifiers::empty());
        assert_eq!(result, None);
    }

    #[test]
    fn test_help_text() {
        let text = help_text();
        assert_ne!(text, "");
        assert!(text.contains("Ctrl+D"));
        assert!(text.contains("Deploy mods"));
        assert!(text.contains("F5"));
        assert!(text.contains("Refresh"));
    }

    #[test]
    fn test_all_shortcuts_unique() {
        let shortcuts = all_shortcuts();
        let mut seen = std::collections::HashSet::new();
        for s in &shortcuts {
            let key_repr = format!("{:?}+{:?}", s.modifiers, s.key);
            assert!(
                seen.insert(key_repr.clone()),
                "Duplicate shortcut: {key_repr}"
            );
        }
    }
}
