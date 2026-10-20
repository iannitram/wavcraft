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
//!   override file (roadmap A291, the editor): [`Keymap::with_overrides`] takes a list of
//!   `(chord, command)` replacements. The editor's operations ([`Keymap::assign`],
//!   [`Keymap::assign_reassign`], [`Keymap::clear`], [`Keymap::reset`]) change a keymap, and
//!   [`Keymap::overrides`] reads off how it differs from the defaults: [`Overrides`] is that
//!   difference, written to and read from `shortcuts.txt` in the config folder ([`load`], [`save`]).
//! - The control channel's `KEY <chord>` runs [`Keymap::resolve`] on a chord typed as text
//!   (`Ctrl+Shift+Z`, `Cmd+Z`, `Space`, `F3`), parsed by [`Shortcut::parse`].

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

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
const fn shift(key: &'static str) -> Shortcut {
    Shortcut { cmd: false, shift: true, alt: false, key }
}
const fn cmd_alt(key: &'static str) -> Shortcut {
    Shortcut { cmd: true, shift: false, alt: true, key }
}
const fn alt(key: &'static str) -> Shortcut {
    Shortcut { cmd: false, shift: false, alt: true, key }
}

/// Every key name a chord may use (egui's `Key::name`), for parsing text into a chord.
const KEY_NAMES: &[&str] = &[
    "A",
    "B",
    "C",
    "D",
    "E",
    "F",
    "G",
    "H",
    "I",
    "J",
    "K",
    "L",
    "M",
    "N",
    "O",
    "P",
    "Q",
    "R",
    "S",
    "T",
    "U",
    "V",
    "W",
    "X",
    "Y",
    "Z",
    "0",
    "1",
    "2",
    "3",
    "4",
    "5",
    "6",
    "7",
    "8",
    "9",
    "F1",
    "F2",
    "F3",
    "F4",
    "F5",
    "F6",
    "F7",
    "F8",
    "F9",
    "F10",
    "F11",
    "F12",
    "Space",
    "Enter",
    "Tab",
    "Escape",
    "Delete",
    "Backspace",
    "Home",
    "End",
    "PageUp",
    "PageDown",
    "ArrowUp",
    "ArrowDown",
    "ArrowLeft",
    "ArrowRight",
    "Equals",
    "Minus",
    "Plus",
    "Backslash",
    "Comma",
    "Period",
    "Slash",
    "Semicolon",
    "Backtick",
    "OpenBracket",
    "CloseBracket",
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

impl Shortcut {
    /// A chord from a key name (egui's `Key::name`) and its modifiers; `None` for a key the
    /// table has no name for.
    pub fn from_parts(cmd: bool, shift: bool, alt: bool, key_name: &str) -> Option<Shortcut> {
        KEY_NAMES.iter().copied().find(|n| *n == key_name).map(|key| Shortcut { cmd, shift, alt, key })
    }

    /// The plain-ASCII spelling the key file and the control channel use (`Ctrl+Shift+ArrowUp`):
    /// the key's own name, never a symbol, so [`Shortcut::parse`] always reads it back.
    pub fn to_ascii(&self) -> String {
        let mut s = String::new();
        for (on, name) in [(self.cmd, "Ctrl+"), (self.alt, "Alt+"), (self.shift, "Shift+")] {
            if on {
                s.push_str(name);
            }
        }
        s + self.key
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

impl Command {
    /// Every command that can have a chord: the window commands, then the action table.
    pub fn all() -> Vec<Command> {
        let mut v = vec![Command::Open, Command::Save, Command::SaveAs, Command::PlayStop];
        v.extend(actions::TABLE.iter().map(|a| Command::Action(a.id)));
        v
    }

    /// The command with this control-channel name.
    pub fn from_name(name: &str) -> Option<Command> {
        Command::all().into_iter().find(|c| c.name() == name)
    }

    /// What a list of commands shows for it: the menu text.
    pub fn label(self) -> &'static str {
        match self {
            Command::Action(id) => actions::action(id).menu,
            Command::Open => "Open…",
            Command::Save => "Save",
            Command::SaveAs => "Save As…",
            Command::PlayStop => "Play/Stop",
        }
    }
}

/// One row of the default table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    // View.
    act(ActionId::CursorToStart, key("Home")),
    act(ActionId::CursorToEnd, key("End")),
    act(ActionId::ZoomIn, key("Equals")),
    act(ActionId::ZoomOut, key("Minus")),
    act(ActionId::ZoomToSelection, shift("S")),
    act(ActionId::ZoomOutFull, cmd("Backslash")),
    // Rows added by the branches merged after this table was cut (their keys used to sit in the action table).
    act(ActionId::Clipboard1, cmd("1")),
    act(ActionId::Clipboard2, cmd("2")),
    act(ActionId::Clipboard3, cmd("3")),
    act(ActionId::Clipboard4, cmd("4")),
    act(ActionId::Clipboard5, cmd("5")),
    act(ActionId::CaptureNoisePrint, shift("P")),
    act(ActionId::ConvertSampleType, shift("T")),
    act(ActionId::MarkerAdd, key("M")),
    act(ActionId::MarkerPrev, cmd_alt("ArrowLeft")),
    act(ActionId::MarkerNext, cmd_alt("ArrowRight")),
    act(ActionId::SnapToggle, key("S")),
    act(ActionId::ToolTimeSelection, key("T")),
    act(ActionId::ZoomAmpIn, alt("Equals")),
    act(ActionId::ZoomAmpOut, alt("Minus")),
    act(ActionId::ToggleReturnOnStop, shift("X")),
    act(ActionId::ToggleChannel(0), alt("1")),
    act(ActionId::ToggleChannel(1), alt("2")),
    act(ActionId::ToggleChannel(2), alt("3")),
    act(ActionId::ToggleChannel(3), alt("4")),
    act(ActionId::ToggleChannel(4), alt("5")),
    act(ActionId::ToggleChannel(5), alt("6")),
    act(ActionId::ToggleChannel(6), alt("7")),
    act(ActionId::ToggleChannel(7), alt("8")),
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

/// A chord that another command already has.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Conflict {
    pub chord: Shortcut,
    pub holder: Command,
}

impl Keymap {
    /// Give `command` exactly the one chord `chord` (its other chords go). Refused, changing
    /// nothing, when another command has the chord: the editor then asks, and calls
    /// [`Keymap::assign_reassign`] if the user agrees.
    pub fn assign(&mut self, command: Command, chord: Shortcut) -> Result<(), Conflict> {
        if let Some(holder) = self.resolve(chord).filter(|h| *h != command) {
            return Err(Conflict { chord, holder });
        }
        self.bindings.retain(|b| b.command != command);
        self.bindings.push(Binding { command, chord });
        Ok(())
    }

    /// Like [`Keymap::assign`], but takes the chord from the command that has it (which keeps
    /// its other chords, if any). Returns that command.
    pub fn assign_reassign(&mut self, command: Command, chord: Shortcut) -> Option<Command> {
        let holder = self.resolve(chord).filter(|h| *h != command);
        self.bindings.retain(|b| b.command != command && b.chord != chord);
        self.bindings.push(Binding { command, chord });
        holder
    }

    /// Take every chord off `command`.
    pub fn clear(&mut self, command: Command) {
        self.bindings.retain(|b| b.command != command);
    }

    /// Back to the defaults.
    pub fn reset(&mut self) {
        *self = Keymap::new();
    }

    /// Whether every command has exactly its default chords.
    pub fn is_default(&self) -> bool {
        self.overrides().entries.is_empty()
    }

    /// How this keymap differs from the defaults: each command whose chords are not its default
    /// ones, with the chords it has (none if cleared).
    pub fn overrides(&self) -> Overrides {
        let mut entries = Vec::new();
        for command in Command::all() {
            let now: Vec<Shortcut> = self.chords(command).collect();
            let default: Vec<Shortcut> = BINDINGS.iter().filter(|b| b.command == command).map(|b| b.chord).collect();
            if now != default {
                entries.push((command, now));
            }
        }
        Overrides { entries }
    }

    /// The defaults with `overrides` applied, through [`Keymap::with_overrides`]. Fails when
    /// the result has a chord on two commands.
    pub fn from_overrides(overrides: &Overrides) -> Result<Keymap, String> {
        let pairs: Vec<(Shortcut, Command)> = overrides.entries.iter().flat_map(|(command, chords)| chords.iter().map(|c| (*c, *command))).collect();
        let mut map = Keymap::with_overrides(&pairs)?;
        for (command, chords) in &overrides.entries {
            if chords.is_empty() {
                map.clear(*command);
            }
        }
        Ok(map)
    }
}

impl PartialEq for Keymap {
    /// Same chords on every command (the order of rows does not matter).
    fn eq(&self, other: &Keymap) -> bool {
        Command::all().into_iter().all(|c| self.chords(c).eq(other.chords(c)))
    }
}

/// The user's key file: what differs from the default table. Text, one `command = chord` per
/// line (`#` starts a comment); a command with two chords has two lines, and `command = none`
/// means it has no chord.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Overrides {
    pub entries: Vec<(Command, Vec<Shortcut>)>,
}

impl Overrides {
    /// The file's text.
    pub fn to_text(&self) -> String {
        let mut s = String::from(
            "# WavCraft keyboard shortcuts: only the keys that differ from the defaults.\n# One `command = chord` per line; `command = none` removes a command's keys.\n",
        );
        for (command, chords) in &self.entries {
            if chords.is_empty() {
                s.push_str(&format!("{} = none\n", command.name()));
            }
            for chord in chords {
                s.push_str(&format!("{} = {}\n", command.name(), chord.to_ascii()));
            }
        }
        s
    }

    /// Read the file's text. Any line that is not understood fails the whole file: a half-read
    /// key file would leave keys the user did not choose.
    pub fn parse(text: &str) -> Result<Overrides, String> {
        let mut entries: Vec<(Command, Vec<Shortcut>)> = Vec::new();
        let mut none_seen: Vec<Command> = Vec::new();
        for (n, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let at = |why: String| format!("line {}: {why}", n + 1);
            let (name, value) = line.split_once('=').ok_or_else(|| at(format!("{line:?} is not `command = chord`")))?;
            let command = Command::from_name(name.trim()).ok_or_else(|| at(format!("no command named {:?}", name.trim())))?;
            let index = match entries.iter().position(|(c, _)| *c == command) {
                Some(i) => i,
                None => {
                    entries.push((command, Vec::new()));
                    entries.len() - 1
                }
            };
            if value.trim().eq_ignore_ascii_case("none") {
                if !entries[index].1.is_empty() || none_seen.contains(&command) {
                    return Err(at(format!("{} is given both `none` and a chord", command.name())));
                }
                none_seen.push(command);
            } else {
                if none_seen.contains(&command) {
                    return Err(at(format!("{} is given both `none` and a chord", command.name())));
                }
                entries[index].1.push(Shortcut::parse(value).map_err(at)?);
            }
        }
        Ok(Overrides { entries })
    }
}

/// The key file: `shortcuts.txt` next to the settings file, in the config folder.
pub fn default_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("wavcraft").join("shortcuts.txt"))
}

/// The keymap the key file at `path` makes. A missing file is the defaults. A file that can't
/// be read or understood is ignored (the defaults) and the second value says why, to be logged:
/// a broken key file never stops the app starting.
pub fn load(path: &Path) -> (Keymap, Option<String>) {
    let text = match std::fs::read(path) {
        Ok(bytes) => match String::from_utf8(bytes) {
            Ok(t) => t,
            Err(_) => return (Keymap::new(), Some(format!("the shortcut file {} is not text: ignored, using the default keys", path.display()))),
        },
        Err(e) if e.kind() == io::ErrorKind::NotFound => return (Keymap::new(), None),
        Err(e) => return (Keymap::new(), Some(format!("could not read the shortcut file {}: {e}: ignored, using the default keys", path.display()))),
    };
    match Overrides::parse(&text).and_then(|o| Keymap::from_overrides(&o)) {
        Ok(map) => (map, None),
        Err(why) => (Keymap::new(), Some(format!("the shortcut file {} is ignored, using the default keys ({why})", path.display()))),
    }
}

/// Write `map`'s differences from the defaults to `path` (through a temporary file, then a
/// rename). With none, the file is removed.
pub fn save(path: &Path, map: &Keymap) -> io::Result<()> {
    let overrides = map.overrides();
    if overrides.entries.is_empty() {
        return match std::fs::remove_file(path) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        };
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("txt.tmp");
    std::fs::write(&tmp, overrides.to_text())?;
    std::fs::rename(&tmp, path)
}

impl Default for Keymap {
    fn default() -> Keymap {
        Keymap::new()
    }
}
