//! The Playlist panel (A188-A190) and the runner that plays it through the existing player:
//! each range in order, its loops, then the next. The list logic is `wavcraft_core::Playlist`.
//!
//! The playlist lives in the window's session: it is not saved in the WAV and is cleared when
//! another file is opened.

use std::ops::Range;

use wavcraft_core::{Markers, Playlist, format_time};

use crate::player::Player;

/// A playlist being played: the ranges still to go and the one now sounding.
struct Run {
    seq: Vec<Range<u64>>,
    next: usize,
    /// The range the player was last given, if it really started (there may be no device).
    current: Option<Range<u64>>,
}

#[derive(Default)]
pub struct PlaylistUi {
    pub list: Playlist,
    run: Option<Run>,
    /// Every range handed to the player by the last run, in order (checked by `PLAYLIST state`).
    pub played: Vec<Range<u64>>,
    /// The row picked in the panel.
    pick: Option<usize>,
    /// The marker (by position) the "Add" selector offers.
    add_index: usize,
}

/// What the panel asks the window to do.
#[derive(Default)]
pub struct PanelOut {
    pub play: bool,
    pub stop: bool,
}

impl PlaylistUi {
    pub fn running(&self) -> bool {
        self.run.is_some()
    }

    /// Forget the list and any run (another file was opened).
    pub fn reset(&mut self) {
        *self = PlaylistUi::default();
    }

    pub fn cancel(&mut self) {
        self.run = None;
    }

    /// Start playing from the first item. False when there is nothing to play.
    pub fn start(&mut self, markers: &Markers, frames: u64) -> bool {
        let seq = self.list.sequence(markers, frames);
        if seq.is_empty() {
            return false;
        }
        self.played.clear();
        self.run = Some(Run { seq, next: 0, current: None });
        true
    }

    /// Called every frame: when the player has finished a range, hand it the next one. A player
    /// that stopped before its range ended (the user stopped it) ends the run.
    pub fn tick(&mut self, player: &mut Player) {
        let Some(run) = self.run.as_mut() else { return };
        if player.is_playing() {
            return;
        }
        if let Some(cur) = run.current.take()
            && player.position() + 1 < cur.end as usize
        {
            self.run = None;
            return;
        }
        let Some(range) = run.seq.get(run.next).cloned() else {
            self.run = None;
            return;
        };
        run.next += 1;
        player.play(range.start as usize, range.end as usize);
        self.played.push(range.clone());
        // With no output device nothing starts: the run still walks the list.
        run.current = player.is_playing().then_some(range);
    }

    /// `OK items=<n> list=<marker index>x<loops>;… running=<0|1> next=<k>/<total> played=<a-b>;…`
    pub fn state_line(&self, markers: &Markers) -> String {
        let items: Vec<String> = self
            .list
            .items()
            .iter()
            .map(|it| format!("{}x{}", markers.index_of(it.marker).map_or("-".to_string(), |i| i.to_string()), it.loops))
            .collect();
        let (next, total) = self.run.as_ref().map_or((0, 0), |r| (r.next, r.seq.len()));
        let played: Vec<String> = self.played.iter().map(|r| format!("{}-{}", r.start, r.end)).collect();
        format!(
            "OK items={} list={} running={} next={next}/{total} played={}",
            self.list.len(),
            items.join(";"),
            u8::from(self.running()),
            played.join(";")
        )
    }

    /// The `PLAYLIST` control verb, except `play` (which needs the player's window).
    pub fn control(&mut self, rest: &str, markers: &Markers, frames: u64) -> String {
        const USAGE: &str =
            "ERR PLAYLIST add <marker> [loops=<n>] | move <i> <j> | remove <i> | loops <i> <n> | play | stop | clear | state | sequence";
        let words: Vec<&str> = rest.split_whitespace().collect();
        let num = |i: usize| words.get(i).and_then(|w| w.trim_start_matches("loops=").parse::<usize>().ok());
        match words.first().copied().unwrap_or("") {
            "add" => {
                let Some(i) = num(1) else { return USAGE.to_string() };
                let Some(m) = markers.id_at(i) else { return format!("ERR no marker {i} (there are {})", markers.len()) };
                if markers.iter().nth(i).is_some_and(|m| m.is_point()) {
                    return "ERR a point marker has no range to play".to_string();
                }
                let loops = if words.len() > 2 { num(2) } else { Some(1) };
                let Some(loops) = loops.filter(|n| *n >= 1) else { return USAGE.to_string() };
                self.list.add(m, loops as u32);
                format!("OK items={}", self.list.len())
            }
            "move" => match (num(1), num(2)) {
                (Some(a), Some(b)) if self.list.move_item(a, b) => "OK".to_string(),
                (Some(_), Some(_)) => format!("ERR no such position (there are {} items)", self.list.len()),
                _ => USAGE.to_string(),
            },
            "remove" => match num(1) {
                Some(i) if self.list.remove(i) => "OK".to_string(),
                Some(i) => format!("ERR no item {i}"),
                None => USAGE.to_string(),
            },
            "loops" => match (num(1), num(2)) {
                (Some(i), Some(n)) if n >= 1 && self.list.set_loops(i, n as u32) => "OK".to_string(),
                (Some(_), Some(_)) => "ERR no such item, or loops below 1".to_string(),
                _ => USAGE.to_string(),
            },
            "stop" => {
                self.cancel();
                "OK".to_string()
            }
            "clear" => {
                self.list.clear();
                self.cancel();
                "OK".to_string()
            }
            "state" => self.state_line(markers),
            "sequence" => {
                let seq = self.list.sequence(markers, frames);
                let body: Vec<String> = seq.iter().map(|r| format!("{}-{}", r.start, r.end)).collect();
                format!("OK count={} {}", seq.len(), body.join(";"))
            }
            _ => USAGE.to_string(),
        }
    }

    pub fn panel(&mut self, ui: &mut egui::Ui, markers: &Markers, rate: u32) -> PanelOut {
        let mut out = PanelOut::default();
        ui.separator();
        ui.heading("Playlist");
        let ranges: Vec<(usize, &wavcraft_core::Marker)> = markers.iter().enumerate().filter(|(_, m)| !m.is_point()).collect();
        ui.horizontal(|ui| {
            let label = ranges.iter().find(|(i, _)| *i == self.add_index).map_or("Pick a range".to_string(), |(_, m)| m.name.clone());
            egui::ComboBox::from_id_salt("playlist-add").selected_text(label).show_ui(ui, |ui| {
                for (i, m) in &ranges {
                    ui.selectable_value(&mut self.add_index, *i, &m.name);
                }
            });
            if ui.add_enabled(ranges.iter().any(|(i, _)| *i == self.add_index), egui::Button::new("Add")).clicked()
                && let Some(id) = markers.id_at(self.add_index)
            {
                self.list.add(id, 1);
            }
        });
        let mut remove = None;
        let mut mv: Option<(usize, usize)> = None;
        let mut loops: Option<(usize, u32)> = None;
        let count = self.list.len();
        for (row, item) in self.list.items().iter().enumerate() {
            ui.horizontal(|ui| {
                let (name, span) = markers.iter().find(|m| m.id == item.marker).map_or(("(deleted marker)".to_string(), String::new()), |m| {
                    (m.name.clone(), format!("{} – {}", format_time(m.start as usize, rate), format_time(m.end as usize, rate)))
                });
                let picked = self.pick == Some(row);
                if ui.selectable_label(picked, format!("{}. {name}  {span}", row + 1)).clicked() {
                    self.pick = Some(row);
                }
                let mut n = item.loops;
                if ui.add(egui::DragValue::new(&mut n).range(1..=wavcraft_core::playlist::MAX_LOOPS).prefix("x ")).changed() {
                    loops = Some((row, n));
                }
                if ui.add_enabled(row > 0, egui::Button::new("↑")).clicked() {
                    mv = Some((row, row - 1));
                }
                if ui.add_enabled(row + 1 < count, egui::Button::new("↓")).clicked() {
                    mv = Some((row, row + 1));
                }
                if ui.button("✕").clicked() {
                    remove = Some(row);
                }
            });
        }
        if let Some((r, n)) = loops {
            self.list.set_loops(r, n);
        }
        if let Some((a, b)) = mv {
            self.list.move_item(a, b);
            self.pick = Some(b);
        }
        if let Some(r) = remove {
            self.list.remove(r);
            self.pick = None;
        }
        ui.horizontal(|ui| {
            if self.running() {
                if ui.button("■ Stop playlist").clicked() {
                    out.stop = true;
                }
            } else if ui.add_enabled(!self.list.is_empty(), egui::Button::new("▶ Play playlist")).clicked() {
                out.play = true;
            }
        });
        out
    }
}
