//! Markers (A170): point markers and range markers with a name and a position in frames. A
//! [`Markers`] list is part of the document state ([`crate::DocState`]), so undo and redo bring
//! it back with the audio. Edits move them (A182: [`Markers::moved_by`]); a WAV file stores them
//! as `cue ` and `LIST`/`adtl` chunks (A183: [`Markers::to_wav_chunks`], [`Markers::from_wav_chunks`]).

use std::ops::Range;

/// One marker. A point marker has `end == start`; a range marker has `end > start`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Marker {
    /// Stable within one document: never reused, kept by undo and redo.
    pub id: u64,
    pub name: String,
    /// First frame (the position of a point marker).
    pub start: u64,
    /// Frame just past the range (equal to `start` for a point marker).
    pub end: u64,
    /// Free text shown in the Markers panel (not saved in the WAV yet).
    pub description: String,
}

impl Marker {
    pub fn is_point(&self) -> bool {
        self.end <= self.start
    }

    pub fn range(&self) -> Range<u64> {
        self.start..self.end.max(self.start)
    }
}

/// The markers of one document state, ordered by start (then id).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Markers {
    list: Vec<Marker>,
    next_id: u64,
}

impl Markers {
    pub fn new() -> Markers {
        Markers::default()
    }

    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    pub fn len(&self) -> usize {
        self.list.len()
    }

    pub fn iter(&self) -> std::slice::Iter<'_, Marker> {
        self.list.iter()
    }

    pub fn as_slice(&self) -> &[Marker] {
        &self.list
    }

    /// The default name of the next marker: "Marker 01", "Marker 02", … (the first not taken).
    pub fn default_name(&self) -> String {
        (1u64..).map(|n| format!("Marker {n:02}")).find(|name| self.list.iter().all(|m| &m.name != name)).unwrap_or_default()
    }

    /// A copy with a marker added over `range` (a point when empty), named `name` or by default.
    /// None when a marker with exactly this position and extent exists already.
    pub fn with_added(&self, range: Range<u64>, name: Option<&str>) -> Option<Markers> {
        let (start, end) = (range.start, range.end.max(range.start));
        if self.list.iter().any(|m| m.start == start && m.end.max(m.start) == end) {
            return None;
        }
        let mut out = self.clone();
        let name = name.map_or_else(|| self.default_name(), str::to_string);
        out.list.push(Marker { id: out.next_id, name, start, end, description: String::new() });
        out.next_id += 1;
        out.list.sort_by_key(|m| (m.start, m.id));
        Some(out)
    }

    /// The start of the first marker strictly after `frame`.
    pub fn next_after(&self, frame: u64) -> Option<u64> {
        self.list.iter().map(|m| m.start).find(|&s| s > frame)
    }

    /// The start of the last marker strictly before `frame`.
    pub fn prev_before(&self, frame: u64) -> Option<u64> {
        self.list.iter().map(|m| m.start).rev().find(|&s| s < frame)
    }

    /// A copy with every position held to `len` frames (after an edit shortened the document).
    /// None when nothing needs it.
    pub fn clamped(&self, len: u64) -> Option<Markers> {
        if self.list.iter().all(|m| m.start <= len && m.end <= len) {
            return None;
        }
        let mut out = self.clone();
        for m in &mut out.list {
            m.start = m.start.min(len);
            m.end = m.end.min(len);
        }
        out.list.sort_by_key(|m| (m.start, m.id));
        Some(out)
    }
}

/// How an edit changed the frame positions of the audio (what markers follow).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Shift {
    /// Nothing moved (same length, or only some channels).
    None,
    /// `range` was removed; later audio moved earlier.
    Delete(Range<u64>),
    /// `len` frames were inserted at `at`; later audio moved later.
    Insert { at: u64, len: u64 },
    /// `range` was replaced by `len` frames (a delete, then an insert at its start).
    Replace { range: Range<u64>, len: u64 },
    /// Only `range` was kept.
    Crop(Range<u64>),
}

const CUE_ENTRY: usize = 24;
const MAX_WAV_MARKERS: usize = 100_000;

fn le32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at.checked_add(4)?)?.try_into().ok()?))
}

fn pad_even(out: &mut Vec<u8>) {
    if out.len() & 1 == 1 {
        out.push(0);
    }
}

fn push_chunk(out: &mut Vec<u8>, id: &[u8; 4], body: &[u8]) {
    out.extend_from_slice(id);
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(body);
    pad_even(out);
}

impl Markers {
    /// Markers from `(name, start, end)` triples (ids are made here, in order).
    pub fn from_items(items: Vec<(String, u64, u64)>) -> Markers {
        let mut out = Markers::new();
        for (name, start, end) in items {
            out.list.push(Marker { id: out.next_id, name, start, end: end.max(start), description: String::new() });
            out.next_id += 1;
        }
        out.list.sort_by_key(|m| (m.start, m.id));
        out
    }

    /// A copy with the positions an edit leaves. A marker inside a removed range goes (a point) or
    /// is cut to what is left (a range that overlaps it; one wholly inside goes). A point exactly
    /// where audio is inserted stays; a range that spans it grows. None when nothing changes.
    pub fn moved_by(&self, shift: &Shift) -> Option<Markers> {
        if *shift == Shift::None || self.list.is_empty() {
            return None;
        }
        let mut out = self.clone();
        out.list.retain_mut(|m| {
            let was_point = m.is_point();
            let (s, e) = (m.start, m.end.max(m.start));
            let replace = |range: &Range<u64>, len: u64| -> Option<(u64, u64)> {
                let n = range.end - range.start;
                if n > 0 && ((was_point && s > range.start && s < range.end) || (!was_point && s >= range.start && e <= range.end)) {
                    return None;
                }
                let map = |p: u64, is_start: bool| {
                    if p <= range.start {
                        p
                    } else if p >= range.end {
                        p - n + len
                    } else if is_start {
                        range.start + len
                    } else {
                        range.start
                    }
                };
                Some((map(s, true), map(e, false)))
            };
            let moved = match shift {
                Shift::None => Some((s, e)),
                Shift::Delete(r) => replace(r, 0),
                Shift::Insert { at, len } => replace(&(*at..*at), *len),
                Shift::Replace { range, len } => replace(range, *len),
                Shift::Crop(r) => {
                    if (was_point && (s < r.start || s > r.end)) || (!was_point && (e <= r.start || s >= r.end)) {
                        None
                    } else {
                        Some((s.clamp(r.start, r.end) - r.start, e.clamp(r.start, r.end) - r.start))
                    }
                }
            };
            let Some((ns, ne)) = moved else { return false };
            m.start = ns;
            m.end = if was_point { ns } else { ne };
            true
        });
        out.list.sort_by_key(|m| (m.start, m.id));
        (out != *self).then_some(out)
    }

    /// Ids of the markers the selection touches: a point at a frame in `sel` (both ends count, so
    /// a cursor finds a marker exactly there), or a range that overlaps `sel`.
    pub fn touched(&self, sel: &Range<u64>) -> Vec<u64> {
        self.list.iter().filter(|m| m.start <= sel.end && m.end.max(m.start) >= sel.start).map(|m| m.id).collect()
    }

    /// [`Markers::touched`], only the point markers (`points`) or only the range markers.
    pub fn touched_kind(&self, sel: &Range<u64>, points: bool) -> Vec<u64> {
        let ids = self.touched(sel);
        self.list.iter().filter(|m| ids.contains(&m.id) && m.is_point() == points).map(|m| m.id).collect()
    }

    /// A copy without the markers `ids`; None when none of them exists.
    pub fn without(&self, ids: &[u64]) -> Option<Markers> {
        if !self.list.iter().any(|m| ids.contains(&m.id)) {
            return None;
        }
        let mut out = self.clone();
        out.list.retain(|m| !ids.contains(&m.id));
        Some(out)
    }

    /// A copy where marker `id` covers `range` (a point when empty). None when it is unknown, is
    /// already exactly that, or another marker has exactly that extent.
    pub fn with_extent(&self, id: u64, range: Range<u64>) -> Option<Markers> {
        let (start, end) = (range.start, range.end.max(range.start));
        let at = self.list.iter().position(|m| m.id == id)?;
        let same = |m: &Marker| m.start == start && m.end.max(m.start) == end;
        if same(&self.list[at]) || self.list.iter().any(|m| m.id != id && same(m)) {
            return None;
        }
        let mut out = self.clone();
        out.list[at].start = start;
        out.list[at].end = end;
        out.list.sort_by_key(|m| (m.start, m.id));
        Some(out)
    }

    /// The `cue ` chunk and the `LIST`/`adtl` chunk (each with its 8-byte header and pad byte),
    /// for a WAV file; empty when there are no markers. A point is a cue point with a `labl`; a
    /// range also has an `ltxt` (purpose `rgn `) with its length. Positions past 32 bits are left out.
    /// This list with marker `id` named `name` (an empty name is refused; `None`: no change).
    pub fn with_renamed(&self, id: u64, name: &str) -> Option<Markers> {
        let name = name.trim();
        let at = self.list.iter().position(|m| m.id == id)?;
        if name.is_empty() || self.list[at].name == name {
            return None;
        }
        let mut out = self.clone();
        out.list[at].name = name.to_string();
        Some(out)
    }

    /// Marker `id` moved so its start is `start`, keeping its length, held inside `0..=len`
    /// (`None`: no marker, no change, or another marker is exactly there).
    pub fn with_moved(&self, id: u64, start: u64, len: u64) -> Option<Markers> {
        let m = self.list.iter().find(|m| m.id == id)?;
        let span = m.end.max(m.start) - m.start;
        let start = start.min(len.saturating_sub(span));
        self.with_extent(id, start..start + span)
    }

    /// The id of the marker at position `index` in list order.
    pub fn id_at(&self, index: usize) -> Option<u64> {
        self.list.get(index).map(|m| m.id)
    }

    /// The position of marker `id` in list order.
    pub fn index_of(&self, id: u64) -> Option<usize> {
        self.list.iter().position(|m| m.id == id)
    }

    pub fn to_wav_chunks(&self) -> Vec<u8> {
        let kept: Vec<&Marker> = self.list.iter().filter(|m| m.end <= u64::from(u32::MAX)).take(MAX_WAV_MARKERS).collect();
        if kept.is_empty() {
            return Vec::new();
        }
        let mut cue = Vec::with_capacity(4 + kept.len() * CUE_ENTRY);
        cue.extend_from_slice(&(kept.len() as u32).to_le_bytes());
        let mut adtl = b"adtl".to_vec();
        for (i, m) in kept.iter().enumerate() {
            let id = (i + 1) as u32;
            cue.extend_from_slice(&id.to_le_bytes());
            cue.extend_from_slice(&(m.start as u32).to_le_bytes());
            cue.extend_from_slice(b"data");
            cue.extend_from_slice(&0u32.to_le_bytes());
            cue.extend_from_slice(&0u32.to_le_bytes());
            cue.extend_from_slice(&(m.start as u32).to_le_bytes());
            let mut labl = id.to_le_bytes().to_vec();
            labl.extend_from_slice(m.name.as_bytes());
            labl.push(0);
            push_chunk(&mut adtl, b"labl", &labl);
            if !m.is_point() {
                let mut ltxt = id.to_le_bytes().to_vec();
                ltxt.extend_from_slice(&((m.end - m.start) as u32).to_le_bytes());
                ltxt.extend_from_slice(b"rgn ");
                ltxt.extend_from_slice(&[0u8; 8]);
                push_chunk(&mut adtl, b"ltxt", &ltxt);
            }
        }
        let mut out = Vec::new();
        push_chunk(&mut out, b"cue ", &cue);
        push_chunk(&mut out, b"LIST", &adtl);
        out
    }

    /// Markers from a WAV file's `cue ` chunk body and its `LIST` chunk bodies (those that are
    /// `adtl` are used). Anything malformed is skipped; positions are held to `frames`.
    pub fn from_wav_chunks(cue: &[u8], lists: &[&[u8]], frames: u64) -> Markers {
        let count = le32(cue, 0).unwrap_or(0) as usize;
        let mut points: Vec<(u32, u64)> = Vec::new();
        for i in 0..count.min(MAX_WAV_MARKERS) {
            let at = 4 + i * CUE_ENTRY;
            let (Some(id), Some(off)) = (le32(cue, at), le32(cue, at + 20)) else { break };
            points.push((id, u64::from(off).min(frames)));
        }
        let mut names: Vec<(u32, String)> = Vec::new();
        let mut lens: Vec<(u32, u64)> = Vec::new();
        for list in lists {
            if list.get(..4) != Some(b"adtl") {
                continue;
            }
            let mut pos = 4usize;
            while let (Some(tag), Some(size)) = (list.get(pos..pos + 4), le32(list, pos + 4)) {
                let body = pos + 8;
                let Some(chunk) = list.get(body..body.saturating_add(size as usize).min(list.len())) else { break };
                match tag {
                    b"labl" => {
                        if let Some(id) = le32(chunk, 0) {
                            let text = &chunk[4..];
                            let text = text.split(|&b| b == 0).next().unwrap_or(&[]);
                            names.push((id, String::from_utf8_lossy(text).trim().to_string()));
                        }
                    }
                    b"ltxt" => {
                        if let (Some(id), Some(len)) = (le32(chunk, 0), le32(chunk, 4)) {
                            lens.push((id, u64::from(len)));
                        }
                    }
                    _ => {}
                }
                pos = body.saturating_add(size as usize).saturating_add(size as usize & 1);
            }
        }
        let mut out = Markers::new();
        let mut items = Vec::new();
        for (id, start) in points {
            let end = lens.iter().find(|(i, _)| *i == id).map_or(start, |&(_, l)| (start + l).min(frames));
            let name = names.iter().find(|(i, n)| *i == id && !n.is_empty()).map(|(_, n)| n.clone());
            items.push((name, start, end));
        }
        for (name, start, end) in items {
            let name = name.unwrap_or_else(|| out.default_name());
            out.list.push(Marker { id: out.next_id, name, start, end, description: String::new() });
            out.next_id += 1;
        }
        out.list.sort_by_key(|m| (m.start, m.id));
        out
    }
}

/// The markers picked in the Markers panel: ids (they survive edits that reorder the list) and
/// the row Shift-click extends from.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MarkerPick {
    pub ids: Vec<u64>,
    anchor: Option<u64>,
}

impl MarkerPick {
    /// A click on row `index`: plain picks only it; `ctrl` toggles it; `shift` picks the rows
    /// from the last plain/ctrl click to it (added to the pick when `ctrl` is also held).
    pub fn click(&mut self, markers: &Markers, index: usize, shift: bool, ctrl: bool) {
        let Some(id) = markers.id_at(index) else { return };
        let anchor = self.anchor.and_then(|a| markers.index_of(a));
        match (shift, anchor) {
            (true, Some(a)) => {
                let (lo, hi) = (a.min(index), a.max(index));
                let rows: Vec<u64> = (lo..=hi).filter_map(|i| markers.id_at(i)).collect();
                if !ctrl {
                    self.ids.clear();
                }
                for r in rows {
                    if !self.ids.contains(&r) {
                        self.ids.push(r);
                    }
                }
            }
            (_, _) if ctrl => {
                if let Some(p) = self.ids.iter().position(|&i| i == id) {
                    self.ids.remove(p);
                } else {
                    self.ids.push(id);
                }
                self.anchor = Some(id);
            }
            _ => {
                self.ids = vec![id];
                self.anchor = Some(id);
            }
        }
    }

    /// The picked ids that still exist, as positions in list order.
    pub fn indices(&self, markers: &Markers) -> Vec<usize> {
        let mut v: Vec<usize> = self.ids.iter().filter_map(|&i| markers.index_of(i)).collect();
        v.sort_unstable();
        v
    }

    /// The span of the picked markers (the time selection a click makes): the first start to the
    /// last end. `None` when nothing is picked.
    pub fn span(&self, markers: &Markers) -> Option<Range<u64>> {
        let picked: Vec<&Marker> = markers.iter().filter(|m| self.ids.contains(&m.id)).collect();
        let start = picked.iter().map(|m| m.start).min()?;
        let end = picked.iter().map(|m| m.end.max(m.start)).max()?;
        Some(start..end)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markers_stay_ordered_and_navigation_skips_the_frame_itself() {
        let m = Markers::new().with_added(500..500, None).unwrap().with_added(100..200, None).unwrap().with_added(900..900, Some("End")).unwrap();
        let starts: Vec<u64> = m.iter().map(|x| x.start).collect();
        assert_eq!(starts, [100, 500, 900]);
        assert_eq!(m.as_slice()[0].name, "Marker 02");
        assert_eq!(m.next_after(100), Some(500));
        assert_eq!(m.next_after(900), None);
        assert_eq!(m.prev_before(500), Some(100));
        assert_eq!(m.prev_before(100), None);
        assert!(m.with_added(500..500, None).is_none(), "the same place twice");
        assert!(m.as_slice()[0].range() == (100..200) && !m.as_slice()[0].is_point() && m.as_slice()[1].is_point());
    }

    #[test]
    fn clamping_holds_positions_inside_the_length() {
        let m = Markers::new().with_added(10..10, None).unwrap().with_added(50..90, None).unwrap();
        assert!(m.clamped(100).is_none());
        let c = m.clamped(60).unwrap();
        assert_eq!(c.as_slice()[1].range(), 50..60);
    }

    #[test]
    fn wav_chunks_round_trip_points_ranges_names_and_unicode() {
        let m = Markers::from_items(vec![("Intro".into(), 0, 0), ("Café ünï".into(), 1000, 4000), ("odd".into(), 77, 77)]);
        let bytes = m.to_wav_chunks();
        assert_eq!(bytes.len() % 2, 0);
        assert_eq!(&bytes[..4], b"cue ");
        let cue_len = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;
        let cue = &bytes[8..8 + cue_len];
        let list_at = 8 + cue_len + (cue_len & 1);
        assert_eq!(&bytes[list_at..list_at + 4], b"LIST");
        let list_len = u32::from_le_bytes(bytes[list_at + 4..list_at + 8].try_into().unwrap()) as usize;
        assert_eq!(bytes.len(), list_at + 8 + list_len + (list_len & 1));
        let list = &bytes[list_at + 8..list_at + 8 + list_len];
        assert_eq!(&list[..4], b"adtl");
        let back = Markers::from_wav_chunks(cue, &[list], 10_000);
        let view = |m: &Markers| m.iter().map(|x| (x.name.clone(), x.start, x.end)).collect::<Vec<_>>();
        assert_eq!(view(&back), view(&m));
        assert!(Markers::new().to_wav_chunks().is_empty());
    }

    #[test]
    fn damaged_or_foreign_marker_chunks_give_what_can_be_read_and_never_panic() {
        // A cue point with no label gets a default name; a position past the end is held to it.
        let mut cue = 2u32.to_le_bytes().to_vec();
        for (id, off) in [(5u32, 10u32), (6, 99_999)] {
            cue.extend_from_slice(&id.to_le_bytes());
            cue.extend_from_slice(&off.to_le_bytes());
            cue.extend_from_slice(b"data");
            cue.extend_from_slice(&[0; 8]);
            cue.extend_from_slice(&off.to_le_bytes());
        }
        let m = Markers::from_wav_chunks(&cue, &[], 500);
        assert_eq!(m.iter().map(|x| (x.start, x.end)).collect::<Vec<_>>(), [(10, 10), (500, 500)]);
        assert_eq!(m.as_slice()[0].name, "Marker 01");
        // Every truncation of a good pair of chunks, and a count that lies, is survived.
        let good = Markers::from_items(vec![("a".into(), 5, 9), ("b".into(), 20, 20)]).to_wav_chunks();
        for cut in 0..good.len() {
            let _ = Markers::from_wav_chunks(&good[8..cut.max(8)], &[&good[cut..]], 100);
            let _ = Markers::from_wav_chunks(&good[..cut], &[&good[..cut]], 100);
        }
        assert!(Markers::from_wav_chunks(&[255, 255, 255, 255], &[b"adtl\xff\xff\xff\xffabcd"], 10).is_empty());
        assert!(Markers::from_wav_chunks(&[], &[], 10).is_empty());
    }

    fn pts(m: &Markers) -> Vec<(u64, u64)> {
        m.iter().map(|x| (x.start, x.end)).collect()
    }

    #[test]
    fn moved_by_covers_each_kind_of_edit() {
        let m = Markers::from_items(vec![("p".into(), 10, 10), ("r".into(), 20, 40), ("q".into(), 60, 60)]);
        assert!(m.moved_by(&Shift::None).is_none());
        assert!(m.moved_by(&Shift::Delete(70..80)).is_none(), "nothing after the range: no new list");
        assert_eq!(pts(&m.moved_by(&Shift::Delete(30..50)).unwrap()), [(10, 10), (20, 30), (40, 40)]);
        assert_eq!(pts(&m.moved_by(&Shift::Delete(10..60)).unwrap()), [(10, 10), (10, 10)], "point at the edge stays; the range is gone");
        assert_eq!(
            pts(&m.moved_by(&Shift::Insert { at: 20, len: 5 }).unwrap()),
            [(10, 10), (20, 45), (65, 65)],
            "a range starting at the insertion stays and grows"
        );
        assert_eq!(pts(&m.moved_by(&Shift::Replace { range: 5..15, len: 2 }).unwrap()), [(12, 32), (52, 52)], "the point inside goes");
        assert_eq!(pts(&m.moved_by(&Shift::Crop(15..50)).unwrap()), [(5, 25)]);
    }
    #[test]
    fn rename_and_move_keep_ids_and_refuse_nonsense() {
        let m = Markers::from_items(vec![("A".into(), 100, 100), ("B".into(), 300, 400), ("C".into(), 900, 900)]);
        let (a, b) = (m.id_at(0).unwrap(), m.id_at(1).unwrap());
        let r = m.with_renamed(a, "  Verse ").unwrap();
        assert_eq!(r.as_slice()[0].name, "Verse");
        assert!(m.with_renamed(a, "A").is_none() && m.with_renamed(a, "  ").is_none() && m.with_renamed(99, "x").is_none());
        // A range keeps its length, a move is held inside the file, and the list stays ordered.
        let mv = m.with_moved(b, 1000, 1200).unwrap();
        assert_eq!(
            mv.as_slice().iter().map(|x| (x.name.as_str(), x.start, x.end)).collect::<Vec<_>>(),
            [("A", 100, 100), ("C", 900, 900), ("B", 1000, 1100)]
        );
        assert_eq!(m.with_moved(b, 5000, 1200).unwrap().as_slice()[2].range(), 1100..1200);
        assert!(m.with_moved(a, 900, 1200).is_none(), "onto another marker's place");
        assert!(m.with_moved(a, 100, 1200).is_none(), "no change");
    }

    #[test]
    fn picking_rows_with_click_ctrl_and_shift() {
        let m = Markers::from_items((0..5).map(|i| (format!("m{i}"), i * 100, i * 100 + if i == 3 { 50 } else { 0 })).collect());
        let mut p = MarkerPick::default();
        p.click(&m, 1, false, false);
        assert_eq!(p.indices(&m), [1]);
        assert_eq!(p.span(&m), Some(100..100));
        p.click(&m, 3, false, true);
        assert_eq!(p.indices(&m), [1, 3]);
        assert_eq!(p.span(&m), Some(100..350));
        p.click(&m, 1, false, true);
        assert_eq!(p.indices(&m), [3], "ctrl toggles off");
        p.click(&m, 0, false, false);
        p.click(&m, 2, true, false);
        assert_eq!(p.indices(&m), [0, 1, 2], "shift picks the rows between");
        p.click(&m, 4, true, true);
        assert_eq!(p.indices(&m), [0, 1, 2, 3, 4]);
        p.click(&m, 9, false, false);
        assert_eq!(p.indices(&m).len(), 5, "a row that is not there changes nothing");
        assert!(MarkerPick::default().span(&m).is_none());
    }
}
