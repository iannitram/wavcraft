//! Keyboard shortcuts as data (roadmap A295): one table of `command -> key chord`, kept apart
//! from the action table so a branch that adds a command adds a row here and touches no code.
//!
//! - [`BINDINGS`] is the default table: Adobe Audition's default keys (only the binding is
//!   matched, never Adobe's text) for every command that exists. A chord's `cmd` flag means
//!   "the platform's command key": Command on a Mac, Ctrl on Windows and Linux, so one row
//!   carries both variants ([`Shortcut::text`] spells either).
//! - [`Command`] is what a chord runs: a row of the action table, or one of the few window
//!   commands that are not in it (Open, Save, Save As, Play/Stop).
//! - [`Keymap`] is the table in use. [`Keymap::new`] holds the defaults and checks them for two
//!   commands sharing a chord ([`conflicts`], and the tests). It is the seam for the user's
//!   override file (roadmap A291, the editor): [`Keymap::with_overrides`] already takes a list
//!   of `(chord, command)` replacements; reading them from a file is the later row.
//! - The control channel's `KEY <chord>` runs [`Keymap::resolve`] on a chord typed as text
//!   (`Ctrl+Shift+Z`, `Cmd+Z`, `Space`, `F3`), parsed by [`Shortcut::parse`].

use std::fmt;

use crate::actions::{self, ActionId};

/// A key with its modifiers. `key` is egui's key name (`"Z"`, `"Delete"`, `"ArrowUp"`); `cmd` is
/// Command on a Mac and Ctrl elsewhere.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Shortcut {
    pub cmd: bool,
    pub shift: bool,
    pub alt: bool,
    pub key: &'static str,
}

const fn key(key: &'static str) -> Shortcut {
    Shortcut { cmd: false, shift: false, alt: false, key }
}
const fn cmd(key: &'static str) -> Shortcut {
    Shortcut { cmd: true, shift: false, alt: false, key }
}
const fn cmd_shift(key: &'static str) -> Shortcut {
    Shortcut { cmd: true, shift: true, alt: false, key }
}
const fn cmd_alt(key: &'static str) -> Shortcut {
    Shortcut { cmd: true, shift: false, alt: true, key }
}
const fn shift(key: &'static str) -> Shortcut {
    Shortcut { cmd: false, shift: true, alt: false, key }
}

/// Every key name a chord may use (egui's `Key::name`), for parsing text into a chord.
const KEY_NAMES: &[&str] = &[
    "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R", "S", "T", "U", "V", "W",
    "X", "Y", "Z", "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8",
    "F9", "F10", "F11", "F12", "Space", "Enter", "Tab", "Escape", "Delete", "Backspace", "Home", "End", "PageUp",
    "PageDown", "ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight", "Equals", "Minus", "Plus", "Backslash", "Comma",
    "Period", "Slash", "Semicolon", "Backtick", "OpenBracket", "CloseBracket",
];

/// Typed-out spellings of keys: the symbol, or a shorter word.
const KEY_ALIASES: &[(&str, &str)] = &[
    ("=", "Equals"),
    ("-", "Minus"),
    ("+", "Plus"),
    ("\\", "Backslash"),
    (",", "Comma"),
    (".", "Period"),
    ("/", "Slash"),
    (";", "Semicolon"),
    ("`", "Backtick"),
    ("[", "OpenBracket"),
    ("]", "CloseBracket"),
    ("up", "ArrowUp"),
    ("down", "ArrowDown"),
    ("left", "ArrowLeft"),
    ("right", "ArrowRight"),
    ("del", "Delete"),
    ("esc", "Escape"),
    ("return", "Enter"),
    ("pgup", "PageUp"),
    ("pgdn", "PageDown"),
];

impl Shortcut {
    /// How a menu shows it: `⌘⇧Z` on a Mac, `Ctrl+Shift+Z` elsewhere.
    pub fn text(&self, mac: bool) -> String {
        let key = match self.key {
            "ArrowUp" => "↑",
            "ArrowDown" => "↓",
            "Delete" if mac => "⌦",
            "Backspace" if mac => "⌫",
            "Equals" => "=",
            "Minus" => "−",
            "Backslash" => "\\",
            k => k,
        };
        if mac {
            format!("{}{}{}{key}", if self.alt { "⌥" } else { "" }, if self.shift { "⇧" } else { "" }, if self.cmd { "⌘" } else { "" })
        } else {
            let mut s = String::new();
            for (on, name) in [(self.cmd, "Ctrl+"), (self.alt, "Alt+"), (self.shift, "Shift+")] {
                if on {
                    s.push_str(name);
                }
            }
            s + key
        }
    }

    /// A chord typed as text: modifiers and a key joined by `+`, in any order and case
    /// (`Ctrl+Shift+Z`, `cmd+z`, `shift+=`, `F3`, `Space`). `Ctrl`, `Cmd`, `Command` and `Primary`
    /// all mean the platform's command key, so a script writes one spelling for every platform.
    pub fn parse(text: &str) -> Result<Shortcut, String> {
        let text = text.trim();
        // The key is the last part; a trailing `+` is the Plus key ("Ctrl++").
        let (mods, key_text) = match text.strip_suffix('+') {
            Some(rest) if rest.is_empty() || rest.ends_with('+') => (rest.trim_end_matches('+'), "+"),
            _ => text.rsplit_once('+').unwrap_or(("", text)),
        };
        let mut chord = Shortcut { cmd: false, shift: false, alt: false, key: "" };
        for m in mods.split('+').map(str::trim).filter(|m| !m.is_empty()) {
            match m.to_ascii_lowercase().as_str() {
                "ctrl" | "control" | "cmd" | "command" | "primary" | "⌘" => chord.cmd = true,
                "shift" | "⇧" => chord.shift = true,
                "alt" | "option" | "opt" | "⌥" => chord.alt = true,
                other => return Err(format!("unknown modifier {other:?} in {text:?}")),
            }
        }
        let key_text = key_text.trim();
        let lower = key_text.to_ascii_lowercase();
        let name = KEY_ALIASES
            .iter()
            .find(|(alias, _)| *alias == lower)
            .map(|(_, name)| *name)
            .or_else(|| KEY_NAMES.iter().copied().find(|n| n.eq_ignore_ascii_case(key_text)));
        match name {
            Some(name) => Ok(Shortcut { key: name, ..chord }),
            None => Err(format!("unknown key {key_text:?} in {text:?}")),
        }
    }
}

impl fmt::Display for Shortcut {
    /// The Windows/Linux spelling, which [`Shortcut::parse`] reads back.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text(false))
    }
}

/// What a chord runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Command {
    /// A row of the action table (`actions::TABLE`).
    Action(ActionId),
    /// File › Open… (a file dialog).
    Open,
    /// File › Save.
    Save,
    /// File › Save As….
    SaveAs,
    /// Start or stop playback.
    PlayStop,
}

impl Command {
    /// The control channel's name for it: the action's own name, or `file.open` and so on.
    pub fn name(self) -> &'static str {
        match self {
            Command::Action(id) => actions::action(id).name,
            Command::Open => "file.open",
            Command::Save => "file.save",
            Command::SaveAs => "file.saveAs",
            Command::PlayStop => "transport.playStop",
        }
    }
}

/// One row of the default table.
#[derive(Debug, Clone, Copy)]
pub struct Binding {
    pub command: Command,
    pub chord: Shortcut,
}

const fn bind(command: Command, chord: Shortcut) -> Binding {
    Binding { command, chord }
}
const fn act(id: ActionId, chord: Shortcut) -> Binding {
    Binding { command: Command::Action(id), chord }
}

/// The default shortcuts: Audition 26.5's defaults for every command that exists. A command with
/// two keys has two rows (the first is the one a menu shows). Add a row when a branch adds a
/// command; the tests fail if a chord is used twice.
pub const BINDINGS: &[Binding] = &[
    // File and transport.
    bind(Command::Open, cmd("O")),
    bind(Command::Save, cmd("S")),
    bind(Command::SaveAs, cmd_shift("S")),
    bind(Command::PlayStop, key("Space")),
    // Edit.
    act(ActionId::Undo, cmd("Z")),
    act(ActionId::Redo, cmd_shift("Z")),
    act(ActionId::Redo, cmd("Y")),
    act(ActionId::RepeatLast, key("F3")),
    act(ActionId::RepeatLast, shift("R")),
    act(ActionId::RepeatLastDialog, key("F2")),
    act(ActionId::RepeatLastDialog, cmd("R")),
    act(ActionId::Cut, cmd("X")),
    act(ActionId::Copy, cmd("C")),
    act(ActionId::Paste, cmd("V")),
    act(ActionId::Clipboard1, cmd("1")),
    act(ActionId::Clipboard2, cmd("2")),
    act(ActionId::Clipboard3, cmd("3")),
    act(ActionId::Clipboard4, cmd("4")),
    act(ActionId::Clipboard5, cmd("5")),
    act(ActionId::CaptureNoisePrint, shift("P")),
    act(ActionId::Delete, key("Delete")),
    act(ActionId::Delete, key("Backspace")),
    act(ActionId::Crop, cmd("T")),
    act(ActionId::SelectAll, cmd("A")),
    act(ActionId::DeselectAll, cmd_shift("A")),
    act(ActionId::DeselectAll, cmd("D")),
    act(ActionId::ClearTimeSelection, key("G")),
    // Zero crossings.
    act(ActionId::ZeroInward, shift("I")),
    act(ActionId::ZeroOutward, shift("O")),
    act(ActionId::ZeroLeftToLeft, shift("H")),
    act(ActionId::ZeroLeftToRight, shift("J")),
    act(ActionId::ZeroRightToLeft, shift("K")),
    act(ActionId::ZeroRightToRight, shift("L")),
    // Selection points and channels.
    act(ActionId::SetIn, key("I")),
    act(ActionId::SetOut, key("O")),
    act(ActionId::LeftChannel, key("ArrowUp")),
    act(ActionId::RightChannel, key("ArrowDown")),
    // View and the mouse tools.
    act(ActionId::CursorToStart, key("Home")),
    act(ActionId::CursorToEnd, key("End")),
    act(ActionId::GotoTime, cmd("J")),
    act(ActionId::MarkerAdd, key("M")),
    act(ActionId::MarkerPrev, cmd_alt("ArrowLeft")),
    act(ActionId::MarkerNext, cmd_alt("ArrowRight")),
    act(ActionId::ZoomIn, key("Equals")),
    act(ActionId::ZoomOut, key("Minus")),
    act(ActionId::ZoomToSelection, shift("S")),
    // Snapping on or off (f09; Audition's default key S).
    act(ActionId::SnapToggle, key("S")),
    act(ActionId::ZoomOutFull, cmd("Backslash")),
    act(ActionId::ToolSelect, key("T")),
    act(ActionId::ToggleReturnOnStop, shift("X")),
    act(ActionId::ToolHand, key("H")),
    act(ActionId::ToolZoom, key("Z")),
    // The Marquee tool (B135); E is free in the table.
    act(ActionId::ToolMarquee, key("E")),
];

/// Chords bound to more than one command in `bindings`, as `(chord, first, second)`.
pub fn conflicts(bindings: &[Binding]) -> Vec<(Shortcut, Command, Command)> {
    let mut out = Vec::new();
    for (i, a) in bindings.iter().enumerate() {
        for b in &bindings[i + 1..] {
            if a.chord == b.chord && a.command != b.command {
                out.push((a.chord, a.command, b.command));
            }
        }
    }
    out
}

/// The shortcuts in use: the defaults, with any overrides applied.
#[derive(Debug, Clone)]
pub struct Keymap {
    bindings: Vec<Binding>,
}

impl Keymap {
    /// The defaults.
    pub fn new() -> Keymap {
        Keymap { bindings: BINDINGS.to_vec() }
    }

    /// The defaults with `overrides` applied: a chord listed here runs its command and no longer
    /// runs what it ran before; a command listed here loses its default chords and keeps only the
    /// ones listed. The seam for the user's key file (A291): that row parses a file into these
    /// pairs and calls this. Fails, changing nothing, when the result has a chord on two commands.
    pub fn with_overrides(overrides: &[(Shortcut, Command)]) -> Result<Keymap, String> {
        let mut bindings: Vec<Binding> =
            BINDINGS.iter().filter(|b| !overrides.iter().any(|(chord, command)| *chord == b.chord || *command == b.command)).copied().collect();
        bindings.extend(overrides.iter().map(|(chord, command)| Binding { command: *command, chord: *chord }));
        let map = Keymap { bindings };
        match map.check() {
            Ok(()) => Ok(map),
            Err(e) => Err(e),
        }
    }

    /// `Err` naming every chord that two commands share.
    pub fn check(&self) -> Result<(), String> {
        let found = conflicts(&self.bindings);
        if found.is_empty() {
            return Ok(());
        }
        let list: Vec<String> = found.iter().map(|(chord, a, b)| format!("{chord} is bound to both {} and {}", a.name(), b.name())).collect();
        Err(list.join("; "))
    }

    /// The command a chord runs.
    pub fn resolve(&self, chord: Shortcut) -> Option<Command> {
        self.bindings.iter().find(|b| b.chord == chord).map(|b| b.command)
    }

    /// The command a chord typed as text runs (`Err` when the text is not a chord or nothing is
    /// bound to it).
    pub fn resolve_text(&self, text: &str) -> Result<Command, String> {
        let chord = Shortcut::parse(text)?;
        self.resolve(chord).ok_or_else(|| format!("no command is bound to {chord}"))
    }

    /// Every row, in table order.
    pub fn bindings(&self) -> &[Binding] {
        &self.bindings
    }

    /// The chords a command has, the first being the one a menu shows.
    pub fn chords(&self, command: Command) -> impl Iterator<Item = Shortcut> + '_ {
        self.bindings.iter().filter(move |b| b.command == command).map(|b| b.chord)
    }

    /// How a menu shows the command's first chord (empty when it has none).
    pub fn menu_text(&self, command: Command, mac: bool) -> String {
        self.chords(command).next().map(|c| c.text(mac)).unwrap_or_default()
    }
}

impl Default for Keymap {
    fn default() -> Keymap {
        Keymap::new()
    }
}
