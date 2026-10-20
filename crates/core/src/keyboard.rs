//! The visual keyboard's logic (roadmap A292): an ANSI layout of key names, and which keys have a
//! binding for a set of modifiers. The dialog draws it; nothing here knows about drawing.

use crate::shortcuts::{Command, Keymap};

/// One key on the drawn keyboard: its name in the shortcut table (`None` for a key a chord can't
/// use, drawn blank), its label and its width in key units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KeyCap {
    pub name: Option<&'static str>,
    pub label: &'static str,
    pub width: f32,
}

const fn k(name: &'static str, label: &'static str) -> KeyCap {
    KeyCap { name: Some(name), label, width: 1.0 }
}
const fn kw(name: &'static str, label: &'static str, width: f32) -> KeyCap {
    KeyCap { name: Some(name), label, width }
}
const fn blank(label: &'static str, width: f32) -> KeyCap {
    KeyCap { name: None, label, width }
}

/// The ANSI layout, row by row, then a row of navigation and arrow keys.
pub const ANSI_ROWS: &[&[KeyCap]] = &[
    &[
        k("Escape", "Esc"),
        blank("", 1.0),
        k("F1", "F1"),
        k("F2", "F2"),
        k("F3", "F3"),
        k("F4", "F4"),
        blank("", 0.5),
        k("F5", "F5"),
        k("F6", "F6"),
        k("F7", "F7"),
        k("F8", "F8"),
        blank("", 0.5),
        k("F9", "F9"),
        k("F10", "F10"),
        k("F11", "F11"),
        k("F12", "F12"),
    ],
    &[
        k("Backtick", "`"),
        k("1", "1"),
        k("2", "2"),
        k("3", "3"),
        k("4", "4"),
        k("5", "5"),
        k("6", "6"),
        k("7", "7"),
        k("8", "8"),
        k("9", "9"),
        k("0", "0"),
        k("Minus", "-"),
        k("Equals", "="),
        kw("Backspace", "Bksp", 2.0),
    ],
    &[
        kw("Tab", "Tab", 1.5),
        k("Q", "Q"),
        k("W", "W"),
        k("E", "E"),
        k("R", "R"),
        k("T", "T"),
        k("Y", "Y"),
        k("U", "U"),
        k("I", "I"),
        k("O", "O"),
        k("P", "P"),
        k("OpenBracket", "["),
        k("CloseBracket", "]"),
        kw("Backslash", "\\", 1.5),
    ],
    &[
        blank("Caps", 1.75),
        k("A", "A"),
        k("S", "S"),
        k("D", "D"),
        k("F", "F"),
        k("G", "G"),
        k("H", "H"),
        k("J", "J"),
        k("K", "K"),
        k("L", "L"),
        k("Semicolon", ";"),
        blank("'", 1.0),
        kw("Enter", "Enter", 2.25),
    ],
    &[
        blank("Shift", 2.25),
        k("Z", "Z"),
        k("X", "X"),
        k("C", "C"),
        k("V", "V"),
        k("B", "B"),
        k("N", "N"),
        k("M", "M"),
        k("Comma", ","),
        k("Period", "."),
        k("Slash", "/"),
        blank("Shift", 2.75),
    ],
    &[blank("Ctrl", 1.5), blank("Alt", 1.5), blank("", 1.5), kw("Space", "Space", 6.25)],
    &[
        k("Delete", "Del"),
        k("Home", "Home"),
        k("End", "End"),
        k("PageUp", "PgUp"),
        k("PageDown", "PgDn"),
        blank("", 0.5),
        k("ArrowLeft", "←"),
        k("ArrowUp", "↑"),
        k("ArrowDown", "↓"),
        k("ArrowRight", "→"),
        k("Plus", "+"),
    ],
];

/// Modifiers as the drawn keyboard and the control channel hold them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Mods {
    pub cmd: bool,
    pub shift: bool,
    pub alt: bool,
}

impl Mods {
    /// `Ctrl+Shift`, `Alt`, `none` (or empty): modifiers in any order and case, with the same
    /// spellings a chord accepts.
    pub fn parse(text: &str) -> Result<Mods, String> {
        let mut mods = Mods::default();
        for m in text.split('+').map(str::trim).filter(|m| !m.is_empty() && !m.eq_ignore_ascii_case("none")) {
            match m.to_ascii_lowercase().as_str() {
                "ctrl" | "control" | "cmd" | "command" | "primary" | "⌘" => mods.cmd = true,
                "shift" | "⇧" => mods.shift = true,
                "alt" | "option" | "opt" | "⌥" => mods.alt = true,
                other => return Err(format!("unknown modifier {other:?}")),
            }
        }
        Ok(mods)
    }

    /// `Ctrl+Alt+Shift` style text (`none` for no modifier), as the control channel answers.
    pub fn text(&self) -> String {
        let parts: Vec<&str> = [(self.cmd, "Ctrl"), (self.alt, "Alt"), (self.shift, "Shift")].iter().filter(|(on, _)| *on).map(|(_, n)| *n).collect();
        if parts.is_empty() { "none".to_string() } else { parts.join("+") }
    }
}

impl Keymap {
    /// The keys that run a command with exactly these modifiers held, with the command, in
    /// table order.
    pub fn keys_with(&self, mods: Mods) -> Vec<(&'static str, Command)> {
        self.bindings()
            .iter()
            .filter(|b| b.chord.cmd == mods.cmd && b.chord.shift == mods.shift && b.chord.alt == mods.alt)
            .map(|b| (b.chord.key, b.command))
            .collect()
    }

    /// The commands with a chord on this key, whatever its modifiers, in table order.
    pub fn commands_on_key(&self, key: &str) -> Vec<Command> {
        let mut out: Vec<Command> = Vec::new();
        for b in self.bindings().iter().filter(|b| b.chord.key == key) {
            if !out.contains(&b.command) {
                out.push(b.command);
            }
        }
        out
    }
}
