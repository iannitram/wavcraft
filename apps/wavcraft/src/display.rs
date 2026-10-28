//! Screen maths: drawing in physical pixels on high-DPI displays, and keeping the window on a
//! monitor that is still connected.
//!
//! egui lays out in points; a Retina or 150 % display has `pixels_per_point` of 2.0 or 1.5.
//! A column per *point* is then a half-resolution waveform with blurry edges, so the editor
//! asks for a column per *physical pixel* ([`physical_columns`]) and puts each on a pixel
//! centre ([`column_x`]) with a one-pixel line ([`hairline`]). The window rectangle is kept in
//! physical pixels (what the operating system and winit report) in a small text file.

/// `pixels_per_point` as the maths may use it: finite and above zero, else 1.
pub fn sane_scale(ppp: f32) -> f32 {
    if ppp.is_finite() && ppp > 0.0 { ppp } else { 1.0 }
}

/// How many columns span `width_points` at `ppp`: one per physical pixel (at least 1).
pub fn physical_columns(width_points: f32, ppp: f32) -> usize {
    (width_points.max(0.0) * sane_scale(ppp)).round().max(1.0) as usize
}

/// The x (in points) of the centre of physical-pixel column `i` of a lane whose left edge is
/// `left_points`. The left edge is snapped down to a whole physical pixel first, so the columns never run past
/// the lane's right edge.
pub fn column_x(left_points: f32, i: usize, ppp: f32) -> f32 {
    let ppp = sane_scale(ppp);
    ((left_points * ppp).floor() + i as f32 + 0.5) / ppp
}

/// A line one physical pixel wide, in points.
pub fn hairline(ppp: f32) -> f32 {
    1.0 / sane_scale(ppp)
}

/// A rectangle in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PxRect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

/// How much of a window must lie on one monitor to count as on screen (enough to see and grab
/// the title bar).
const MIN_VISIBLE_W: i64 = 120;
const MIN_VISIBLE_H: i64 = 48;

impl PxRect {
    pub fn new(x: i32, y: i32, w: i32, h: i32) -> PxRect {
        PxRect { x, y, w, h }
    }

    /// Width and height of the overlap with `o` (0 where they do not touch).
    fn overlap(&self, o: &PxRect) -> (i64, i64) {
        let w = (i64::from(self.x) + i64::from(self.w)).min(i64::from(o.x) + i64::from(o.w)) - i64::from(self.x).max(i64::from(o.x));
        let h = (i64::from(self.y) + i64::from(self.h)).min(i64::from(o.y) + i64::from(o.h)) - i64::from(self.y).max(i64::from(o.y));
        (w.max(0), h.max(0))
    }

    /// `x,y,w,h`
    pub fn to_text(self) -> String {
        format!("{},{},{},{}", self.x, self.y, self.w, self.h)
    }

    /// Parse `x,y,w,h` (what [`to_text`](Self::to_text) writes). A size at or below 0, or one
    /// beyond any real screen, is refused.
    pub fn parse(text: &str) -> Option<PxRect> {
        let mut it = text.trim().split(',').map(|p| p.trim().parse::<i32>());
        let (x, y, w, h) = (it.next()?.ok()?, it.next()?.ok()?, it.next()?.ok()?, it.next()?.ok()?);
        if it.next().is_some() || !(1..=100_000).contains(&w) || !(1..=100_000).contains(&h) {
            return None;
        }
        Some(PxRect { x, y, w, h })
    }
}

/// Is enough of `win` on some monitor to be seen and grabbed?
pub fn is_on_screen(win: &PxRect, monitors: &[PxRect]) -> bool {
    monitors.iter().any(|m| {
        let (w, h) = win.overlap(m);
        w >= MIN_VISIBLE_W.min(i64::from(win.w)) && h >= MIN_VISIBLE_H.min(i64::from(win.h))
    })
}

/// `win` as it is when it is on a monitor already; otherwise (the monitor it was on is gone)
/// the same window centred on `monitors[primary]`, made smaller if it does not fit there.
/// With no monitors listed there is nothing to clamp to and `win` comes back unchanged.
pub fn clamp_to_monitors(win: PxRect, monitors: &[PxRect], primary: usize) -> PxRect {
    if monitors.is_empty() || is_on_screen(&win, monitors) {
        return win;
    }
    let p = monitors.get(primary).copied().unwrap_or(monitors[0]);
    let (w, h) = (win.w.min(p.w).max(1), win.h.min(p.h).max(1));
    PxRect { x: p.x + (p.w - w) / 2, y: p.y + (p.h - h) / 2, w, h }
}

// ---------------------------------------------------------------------------------------------
// The window keeper: restores the saved window rectangle at start, clamps it onto a connected
// monitor, and saves it again as the window moves.

use std::path::PathBuf;
use std::time::{Duration, Instant};

/// How the start-up placement came out, for `STATE`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Startup {
    /// Not decided yet (no frame has run).
    Pending,
    /// Nothing was saved and the window opened on a monitor.
    Fresh,
    /// The saved rectangle was on a monitor and was used as it was.
    Restored,
    /// The saved (or opening) rectangle was off every monitor and was moved onto the primary one.
    Clamped,
}

impl Startup {
    pub fn name(self) -> &'static str {
        match self {
            Startup::Pending => "pending",
            Startup::Fresh => "fresh",
            Startup::Restored => "restored",
            Startup::Clamped => "clamped",
        }
    }
}

/// What the window looks like to the operating system, in physical pixels.
#[derive(Debug, Clone, Copy, Default)]
pub struct WindowInfo {
    /// Outer position with the inner size (the rectangle that is saved and restored).
    pub rect: Option<PxRect>,
    pub primary: Option<PxRect>,
    pub monitors: usize,
}

pub struct WindowKeeper {
    path: Option<PathBuf>,
    saved: Option<PxRect>,
    pub startup: Startup,
    started_at: Option<Instant>,
    /// What start-up asked the window to become; writes wait until it gets there (or a while).
    target: Option<PxRect>,
    last_written: Option<PxRect>,
    last_write_at: Option<Instant>,
    warned: bool,
    pub info: WindowInfo,
}

/// Is the window within a few pixels of `want`? (Window managers round and add decorations.)
fn near(a: &PxRect, b: &PxRect) -> bool {
    (a.x - b.x).abs() <= 40 && (a.y - b.y).abs() <= 80 && (a.w - b.w).abs() <= 40 && (a.h - b.h).abs() <= 80
}

impl WindowKeeper {
    /// `<config dir>/wavcraft/window.txt`, next to the settings file.
    pub fn default_path() -> Option<PathBuf> {
        dirs::config_dir().map(|d| d.join("wavcraft").join("window.txt"))
    }

    pub fn new(path: Option<PathBuf>) -> WindowKeeper {
        let saved = path.as_deref().and_then(|p| std::fs::read_to_string(p).ok()).and_then(|t| PxRect::parse(&t));
        WindowKeeper {
            path,
            saved,
            startup: Startup::Pending,
            started_at: None,
            target: None,
            last_written: None,
            last_write_at: None,
            warned: false,
            info: WindowInfo::default(),
        }
    }

    /// Called before every frame. The first call places the window; later calls keep the saved
    /// rectangle current. Does nothing while the window is minimised (its position is not real).
    pub fn tick(&mut self, ctx: &egui::Context, frame: &eframe::Frame) {
        let Some(window) = frame.winit_window() else { return };
        if ctx.input(|i| i.viewport().minimized == Some(true)) {
            return;
        }
        let rect = match (window.outer_position(), window.inner_size()) {
            (Ok(p), s) if s.width > 0 && s.height > 0 => Some(PxRect::new(p.x, p.y, s.width as i32, s.height as i32)),
            _ => None,
        };
        let monitors: Vec<PxRect> =
            window.available_monitors().map(|m| PxRect::new(m.position().x, m.position().y, m.size().width as i32, m.size().height as i32)).collect();
        let primary_rect =
            window.primary_monitor().map(|m| PxRect::new(m.position().x, m.position().y, m.size().width as i32, m.size().height as i32));
        let primary = primary_rect.and_then(|p| monitors.iter().position(|m| *m == p)).unwrap_or(0);
        self.info = WindowInfo { rect, primary: monitors.get(primary).copied(), monitors: monitors.len() };

        if self.startup == Startup::Pending {
            self.started_at = Some(Instant::now());
            let want = self.saved.or(rect);
            let Some(want) = want else { return };
            let target = clamp_to_monitors(want, &monitors, primary);
            self.startup = match (self.saved.is_some(), target != want) {
                (_, true) => Startup::Clamped,
                (true, false) => Startup::Restored,
                (false, false) => Startup::Fresh,
            };
            log::info!(target: "wavcraft", "window start-up: {} (saved {:?}, monitors {}, scale {})", self.startup.name(), self.saved, monitors.len(), ctx.pixels_per_point());
            if self.startup != Startup::Fresh && rect != Some(target) {
                // Points = physical pixels / pixels_per_point (egui-winit multiplies back).
                let ppp = sane_scale(ctx.pixels_per_point());
                ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(target.x as f32 / ppp, target.y as f32 / ppp)));
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(target.w as f32 / ppp, target.h as f32 / ppp)));
                self.target = Some(target);
            }
            return;
        }

        // Keep the file current: not before the window has reached where start-up put it (or a
        // few seconds have passed), and at most once a second.
        let Some(now_rect) = rect else { return };
        if let Some(t) = self.target {
            let waited = self.started_at.is_some_and(|s| s.elapsed() > Duration::from_secs(3));
            if !near(&now_rect, &t) && !waited {
                return;
            }
            self.target = None;
        }
        if Some(now_rect) != self.last_written && self.last_write_at.is_none_or(|t| t.elapsed() > Duration::from_secs(1)) {
            self.write(now_rect);
        }
    }

    fn write(&mut self, rect: PxRect) {
        let Some(path) = &self.path else { return };
        let done = path.parent().is_none_or(|d| std::fs::create_dir_all(d).is_ok()) && std::fs::write(path, format!("{}\n", rect.to_text())).is_ok();
        if done {
            self.last_written = Some(rect);
        } else if !self.warned {
            self.warned = true;
            log::warn!(target: "wavcraft", "could not save the window position to {}", path.display());
        }
        self.last_write_at = Some(Instant::now());
    }

    /// On exit: save the rectangle once more.
    pub fn save_now(&mut self) {
        if self.target.is_none()
            && self.startup != Startup::Pending
            && let Some(r) = self.info.rect
            && Some(r) != self.last_written
        {
            self.write(r);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_column_per_physical_pixel() {
        assert_eq!(physical_columns(800.0, 1.0), 800);
        assert_eq!(physical_columns(800.0, 2.0), 1600);
        assert_eq!(physical_columns(801.4, 1.5), 1202);
        assert_eq!(physical_columns(0.0, 2.0), 1, "never zero columns");
        assert_eq!(physical_columns(100.0, f32::NAN), 100, "a bad scale counts as 1");
        assert_eq!(physical_columns(100.0, 0.0), 100);
    }

    #[test]
    fn columns_sit_on_pixel_centres_and_tile_the_width() {
        // Scale 1: the old maths (left + i + 0.5).
        assert_eq!(column_x(10.0, 0, 1.0), 10.5);
        assert_eq!(column_x(10.0, 3, 1.0), 13.5);
        // Scale 2: columns are half a point apart, centres on quarter points.
        assert_eq!(column_x(10.0, 0, 2.0), 10.25);
        assert_eq!(column_x(10.0, 1, 2.0), 10.75);
        // A left edge between physical pixels snaps to a pixel before counting.
        assert_eq!(column_x(10.3, 0, 2.0), 10.25, "10.3 pt is 20.6 px, which snaps down to 20 px");
        // The last column of a lane ends inside the lane.
        for ppp in [1.0f32, 1.5, 2.0, 3.0] {
            let (left, width) = (7.0f32, 640.0f32);
            let n = physical_columns(width, ppp);
            let last = column_x(left, n - 1, ppp);
            assert!(last > left && last <= left + width, "ppp {ppp}: {last}");
            // The left edge snaps by under a pixel, so the last centre is within a pixel of the right edge.
            assert!((last - (left + width - 0.5 / ppp)).abs() <= 1.0 / ppp + 1e-3, "ppp {ppp}: {last}");
        }
    }

    #[test]
    fn a_hairline_is_one_physical_pixel() {
        assert_eq!(hairline(1.0), 1.0);
        assert_eq!(hairline(2.0), 0.5);
        assert!((hairline(1.5) * 1.5 - 1.0).abs() < 1e-6);
        assert_eq!(hairline(-3.0), 1.0);
    }

    const MAIN: PxRect = PxRect { x: 0, y: 0, w: 1920, h: 1080 };
    const SIDE: PxRect = PxRect { x: 1920, y: 0, w: 1280, h: 1024 };

    #[test]
    fn a_window_on_a_monitor_is_left_alone() {
        let w = PxRect::new(100, 100, 1280, 720);
        assert_eq!(clamp_to_monitors(w, &[MAIN], 0), w);
        let on_side = PxRect::new(2000, 50, 900, 600);
        assert_eq!(clamp_to_monitors(on_side, &[MAIN, SIDE], 0), on_side);
        // Mostly hanging off the edge but with the title bar reachable.
        let hanging = PxRect::new(1700, 900, 1280, 720);
        assert_eq!(clamp_to_monitors(hanging, &[MAIN], 0), hanging);
    }

    #[test]
    fn a_window_on_an_unplugged_monitor_comes_back_centred_on_the_primary() {
        let gone = PxRect::new(2000, 50, 900, 600);
        assert_eq!(clamp_to_monitors(gone, &[MAIN], 0), PxRect::new(510, 240, 900, 600));
        let far = PxRect::new(-30000, -30000, 900, 600);
        assert_eq!(clamp_to_monitors(far, &[MAIN, SIDE], 0), PxRect::new(510, 240, 900, 600));
        // The primary need not be the first or at the origin.
        assert_eq!(clamp_to_monitors(far, &[MAIN, SIDE], 1), PxRect::new(1920 + 190, 212, 900, 600));
        // Just touching a monitor's edge is not on screen.
        let touching = PxRect::new(1920, 0, 900, 600);
        assert_eq!(clamp_to_monitors(touching, &[MAIN], 0), PxRect::new(510, 240, 900, 600));
    }

    #[test]
    fn a_window_too_big_for_the_primary_is_shrunk_to_it() {
        let big = PxRect::new(5000, 5000, 4000, 3000);
        assert_eq!(clamp_to_monitors(big, &[MAIN], 0), MAIN);
        // A tiny monitor list entry never gives a zero size.
        let tiny = PxRect::new(0, 0, 0, 0);
        let got = clamp_to_monitors(PxRect::new(9000, 9000, 500, 500), &[tiny], 0);
        assert!(got.w >= 1 && got.h >= 1);
    }

    #[test]
    fn no_monitor_list_changes_nothing_and_a_bad_primary_index_uses_the_first() {
        let w = PxRect::new(-5000, 0, 800, 600);
        assert_eq!(clamp_to_monitors(w, &[], 0), w);
        assert_eq!(clamp_to_monitors(w, &[MAIN], 7), PxRect::new(560, 240, 800, 600));
    }

    #[test]
    fn a_small_window_counts_as_on_screen_when_all_of_it_is() {
        let small = PxRect::new(10, 10, 80, 30);
        assert!(is_on_screen(&small, &[MAIN]));
        assert!(!is_on_screen(&small, &[SIDE]));
    }

    #[test]
    fn the_saved_rectangle_round_trips_and_bad_text_is_refused() {
        let r = PxRect::new(-30000, 25, 900, 600);
        assert_eq!(PxRect::parse(&r.to_text()), Some(r));
        assert_eq!(PxRect::parse(" 1, 2, 3, 4 \n"), Some(PxRect::new(1, 2, 3, 4)));
        for bad in ["", "1,2,3", "1,2,3,4,5", "a,b,c,d", "0,0,0,10", "0,0,10,-1", "0,0,200000,10"] {
            assert_eq!(PxRect::parse(bad), None, "{bad:?}");
        }
    }
}
