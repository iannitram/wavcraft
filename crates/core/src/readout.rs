//! What is under the pointer in the editor (A079), and how long an operation took (A327),
//! as plain maths with no window: the app draws the numbers, the tests check them.
//!
//! Pixel positions are relative to the editor's top-left corner, as the app's painter uses them.

use crate::spectrogram::SpectrogramParams;
use crate::view::{Viewport, format_time};

/// The editor's geometry: the ruler on top, then the waveform lanes (if shown) over the spectral
/// lanes (if shown). Layered waveforms share one lane; spectral lanes stay per channel.
/// Must match how the app paints (`lane()` there splits an area evenly between channels; the spectral picture is inset one pixel top and bottom, the
/// waveform keeps two pixels of margin above and below full scale).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EditorLayout {
    pub width: f32,
    pub height: f32,
    pub ruler_height: f32,
    pub show_waveform: bool,
    pub show_spectrum: bool,
    pub channels: usize,
    /// Fraction of the body occupied by the waveform when both displays are shown.
    pub split_fraction: f32,
    /// All waveform channels share a lane (spectral channels remain stacked).
    pub layered: bool,
    /// The painter multiplies samples by this vertical zoom.
    pub amp_zoom: f32,
}

/// Pixels between a waveform lane's edge and full scale.
const WAVE_MARGIN: f32 = 2.0;
/// Pixels between a spectral lane's edge and its picture.
const SPECTRUM_INSET: f32 = 1.0;

/// Which part of the editor a pixel is in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Pane {
    Outside,
    Ruler,
    /// A waveform lane: `amplitude` is the level at the pointer's height, clamped to ±1 / zoom.
    Wave {
        /// No single channel when the waveform is Layered.
        channel: Option<usize>,
        amplitude: f32,
    },
    /// A spectral lane: `hz` is the frequency at the pointer's height.
    Spectrum {
        channel: usize,
        hz: f32,
    },
}

impl EditorLayout {
    /// The part of the editor at pixel (`x`, `y`) and what the height means there. A spectral
    /// lane's frequency needs the sample rate and the display's parameters.
    pub fn locate(&self, x: f32, y: f32, params: &SpectrogramParams, sample_rate: u32) -> Pane {
        if !(x.is_finite() && y.is_finite()) || x < 0.0 || y < 0.0 || x >= self.width || y >= self.height {
            return Pane::Outside;
        }
        if y < self.ruler_height {
            return Pane::Ruler;
        }
        let body_top = self.ruler_height;
        let body_h = self.height - body_top;
        let split = if self.split_fraction.is_finite() { self.split_fraction.clamp(0.0, 1.0) } else { 0.5 };
        let wave_h = body_h * split;
        let (wave, spectrum) = match (self.show_waveform, self.show_spectrum) {
            (true, true) => (Some((body_top, wave_h)), Some((body_top + wave_h, body_h - wave_h))),
            (true, false) => (Some((body_top, body_h)), None),
            (false, true) => (None, Some((body_top, body_h))),
            (false, false) => (None, None),
        };
        let channels = self.channels.max(1);
        let layered = self.layered && channels > 1;
        let in_area = |area: Option<(f32, f32)>, lanes: usize| -> Option<(usize, f32, f32)> {
            let (top, h) = area?;
            if h <= 0.0 || y < top || y >= top + h {
                return None;
            }
            let lane_h = h / lanes as f32;
            let channel = (((y - top) / lane_h) as usize).min(lanes - 1);
            Some((channel, y - top - lane_h * channel as f32, lane_h))
        };
        if let Some((channel, ly, lane_h)) = in_area(wave, if layered { 1 } else { channels }) {
            let half = lane_h * 0.5 - WAVE_MARGIN;
            let zoom = if self.amp_zoom.is_finite() && self.amp_zoom > 0.0 { self.amp_zoom } else { 1.0 };
            let amplitude = if half != 0.0 { ((lane_h * 0.5 - ly) / half).clamp(-1.0, 1.0) / zoom } else { 0.0 };
            return Pane::Wave { channel: (!layered).then_some(channel), amplitude };
        }
        if let Some((channel, ly, lane_h)) = in_area(spectrum, channels) {
            let picture_h = (lane_h - 2.0 * SPECTRUM_INSET).max(1.0);
            let t = 1.0 - ((ly - SPECTRUM_INSET) / picture_h).clamp(0.0, 1.0);
            return Pane::Spectrum { channel, hz: params.hz_at(t, sample_rate) };
        }
        Pane::Outside
    }
}

impl SpectrogramParams {
    /// The frequency a fraction `t` of the way up the picture (0 bottom, 1 top).
    pub fn hz_at(&self, t: f32, sample_rate: u32) -> f32 {
        let t = if t.is_finite() { t.clamp(0.0, 1.0) } else { 0.0 };
        crate::freq_axis::FreqAxis::from_params(self).clamped(sample_rate).hz_at_fraction(t)
    }
}

/// A level in dBFS; `None` for silence (minus infinity).
pub fn dbfs(amplitude: f32) -> Option<f32> {
    let a = amplitude.abs();
    (a.is_finite() && a > 0.0).then(|| 20.0 * a.log10())
}

/// "-6.02 dBFS", or "-inf dBFS" for silence.
pub fn dbfs_text(amplitude: f32) -> String {
    match dbfs(amplitude) {
        Some(db) => format!("{db:.2} dBFS"),
        None => "-inf dBFS".to_string(),
    }
}

/// "440 Hz", "1.50 kHz".
pub fn hz_text(hz: f32) -> String {
    if hz >= 1000.0 { format!("{:.2} kHz", hz / 1000.0) } else { format!("{hz:.0} Hz") }
}

/// What the pointer is over, in numbers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Readout {
    /// The frame under the pointer, rounded and kept inside the file (the same rounding the
    /// editor uses for a click).
    pub frame: usize,
    pub pane: Pane,
}

impl Readout {
    /// The channel the pointer is over, when it is over a lane.
    pub fn channel(&self) -> Option<usize> {
        match self.pane {
            Pane::Wave { channel, .. } => channel,
            Pane::Spectrum { channel, .. } => Some(channel),
            _ => None,
        }
    }

    /// The readout for pixel (`x`, `y`); `None` outside the editor.
    pub fn at(
        layout: &EditorLayout,
        view: &Viewport,
        params: &SpectrogramParams,
        sample_rate: u32,
        total_frames: usize,
        x: f32,
        y: f32,
    ) -> Option<Readout> {
        let pane = layout.locate(x, y, params, sample_rate);
        if pane == Pane::Outside {
            return None;
        }
        let frame = view.frame_at(x).clamp(0.0, total_frames as f64).round() as usize;
        Some(Readout { frame, pane })
    }

    /// The line for the status bar. `sample` is the stored sample at `frame` in the channel
    /// (the app reads it from the document), shown beside the level at the pointer's height.
    pub fn text(&self, sample_rate: u32, sample: Option<f32>) -> String {
        let mut s = format_time(self.frame, sample_rate);
        match self.pane {
            Pane::Wave { channel, amplitude } => {
                let channel = channel.map_or_else(|| "all channels".to_string(), |ch| format!("ch {}", ch + 1));
                s += &format!("  {channel}  {amplitude:+.4} ({})", dbfs_text(amplitude));
                if let Some(v) = sample {
                    s += &format!("  sample {v:+.6}");
                }
            }
            Pane::Spectrum { channel, hz } => {
                s += &format!("  ch {}  {}", channel + 1, hz_text(hz));
            }
            _ => {}
        }
        s
    }
}

/// "0.35 s", "2.1 s", "1 min 5 s": how long an operation took.
pub fn duration_text(ms: u64) -> String {
    if ms < 1000 {
        format!("{ms} ms")
    } else if ms < 60_000 {
        format!("{:.1} s", ms as f64 / 1000.0)
    } else {
        format!("{} min {} s", ms / 60_000, ms / 1000 % 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout(wave: bool, spectrum: bool, channels: usize) -> EditorLayout {
        EditorLayout {
            width: 1000.0,
            height: 622.0,
            ruler_height: 22.0,
            show_waveform: wave,
            show_spectrum: spectrum,
            channels,
            split_fraction: 0.5,
            layered: false,
            amp_zoom: 1.0,
        }
    }

    #[test]
    fn the_ruler_and_the_edges() {
        let l = layout(true, false, 2);
        let p = SpectrogramParams::default();
        assert_eq!(l.locate(10.0, 5.0, &p, 48_000), Pane::Ruler);
        assert_eq!(l.locate(-1.0, 100.0, &p, 48_000), Pane::Outside);
        assert_eq!(l.locate(1000.0, 100.0, &p, 48_000), Pane::Outside);
        assert_eq!(l.locate(f32::NAN, 100.0, &p, 48_000), Pane::Outside);
        assert_eq!(layout(false, false, 2).locate(10.0, 100.0, &p, 48_000), Pane::Outside);
    }

    #[test]
    fn waveform_amplitude_is_plus_minus_one_at_the_edges_and_zero_in_the_middle() {
        // Two lanes of 300 px under the ruler: lane 0 is 22..322, centre 172, half 148.
        let l = layout(true, false, 2);
        let p = SpectrogramParams::default();
        let amp = |y| match l.locate(0.0, y, &p, 48_000) {
            Pane::Wave { channel, amplitude } => (channel.unwrap(), amplitude),
            other => panic!("{other:?}"),
        };
        assert_eq!(amp(172.0), (0, 0.0));
        assert_eq!(amp(24.0), (0, 1.0));
        assert!((amp(172.0 + 74.0).1 + 0.5).abs() < 1e-6);
        assert_eq!(amp(22.5).1, 1.0, "inside the margin clamps to full scale");
        // Lane 1: 322..622, centre 472.
        assert_eq!(amp(472.0), (1, 0.0));
        assert_eq!(amp(621.0), (1, -1.0));
    }

    #[test]
    fn spectral_lanes_sit_under_the_waveform_and_map_height_to_frequency() {
        let l = layout(true, true, 1);
        let p = SpectrogramParams { log_freq: false, min_hz: 0.0, max_hz: 24_000.0, ..SpectrogramParams::default() };
        let (lo, hi) = p.range_hz(48_000);
        // Body 22..622: waveform 22..322, spectral 322..622, picture 323..621.
        assert!(matches!(l.locate(5.0, 321.0, &p, 48_000), Pane::Wave { .. }));
        let at = |y| match l.locate(5.0, y, &p, 48_000) {
            Pane::Spectrum { hz, .. } => hz,
            other => panic!("{other:?}"),
        };
        assert!((at(323.0) - hi).abs() < 1.0, "top of the picture is the highest frequency");
        assert!((at(621.0) - lo).abs() < 1.0);
        assert!((at(472.0) - (lo + hi) / 2.0).abs() < 100.0);
        // The log axis: halfway up is the geometric mean.
        let log = SpectrogramParams { log_freq: true, ..p };
        let mid = match l.locate(5.0, 472.0, &log, 48_000) {
            Pane::Spectrum { hz, .. } => hz,
            other => panic!("{other:?}"),
        };
        let (lo, hi) = log.range_hz(48_000);
        assert!((mid - (lo * hi).sqrt()).abs() / mid < 0.02, "{mid}");
    }

    #[test]
    fn split_thirty_percent_uses_the_painted_boundary_and_picture_insets() {
        let l = EditorLayout { split_fraction: 0.3, ..layout(true, true, 2) };
        let p = SpectrogramParams { log_freq: false, min_hz: 0.0, max_hz: 24_000.0, ..SpectrogramParams::default() };
        // Waveform 22..202, lanes 90 px; spectral 202..622, lanes 210 px.
        assert!(matches!(l.locate(5.0, 201.5, &p, 48_000), Pane::Wave { channel: Some(1), .. }));
        assert_eq!(l.locate(5.0, 202.0, &p, 48_000), Pane::Spectrum { channel: 0, hz: 24_000.0 });
        assert_eq!(l.locate(5.0, 203.0, &p, 48_000), Pane::Spectrum { channel: 0, hz: 24_000.0 });
        assert_eq!(l.locate(5.0, 307.0, &p, 48_000), Pane::Spectrum { channel: 0, hz: 12_000.0 });
        assert_eq!(l.locate(5.0, 411.0, &p, 48_000), Pane::Spectrum { channel: 0, hz: 0.0 });
        assert_eq!(l.locate(5.0, 413.0, &p, 48_000), Pane::Spectrum { channel: 1, hz: 24_000.0 });
    }

    #[test]
    fn layered_waveforms_share_one_lane_and_zoom_two_halves_the_readout_level() {
        let l = EditorLayout { split_fraction: 0.3, layered: true, amp_zoom: 2.0, ..layout(true, true, 2) };
        let p = SpectrogramParams::default();
        // One waveform lane 22..202, centre 112, drawable half 88.
        for (y, amplitude) in [(24.0, 0.5), (68.0, 0.25), (112.0, 0.0), (156.0, -0.25), (200.0, -0.5), (201.0, -0.5)] {
            let pane = l.locate(5.0, y, &p, 48_000);
            assert_eq!(pane, Pane::Wave { channel: None, amplitude });
            let r = Readout { frame: 0, pane };
            assert_eq!(r.channel(), None);
            assert!(r.text(48_000, None).contains("all channels"));
        }
        let r = Readout { frame: 0, pane: l.locate(5.0, 24.0, &p, 48_000) };
        assert_eq!(r.text(48_000, None), "0:00.000  all channels  +0.5000 (-6.02 dBFS)");
        // Layered applies only to the waveform, as in the painter.
        assert!(matches!(l.locate(5.0, 500.0, &p, 48_000), Pane::Spectrum { channel: 1, .. }));
        let mono = EditorLayout { channels: 1, ..l };
        assert_eq!(mono.locate(5.0, 24.0, &p, 48_000), Pane::Wave { channel: Some(0), amplitude: 0.5 });
        let stacked = EditorLayout { layered: false, ..l };
        assert_eq!(stacked.locate(5.0, 24.0, &p, 48_000), Pane::Wave { channel: Some(0), amplitude: 0.5 });
        assert_eq!(stacked.locate(5.0, 114.0, &p, 48_000), Pane::Wave { channel: Some(1), amplitude: 0.5 });
    }

    #[test]
    fn partial_log_readout_follows_the_axis_across_the_knee() {
        let l = layout(false, true, 2);
        let p = SpectrogramParams { log_freq: true, partial_log: true, min_hz: 0.0, max_hz: 24_000.0, ..SpectrogramParams::default() };
        let axis = crate::freq_axis::FreqAxis::from_params(&p).clamped(48_000);
        for hz in [0.0, 500.0, 1_000.0, 8_000.0, 24_000.0] {
            for channel in 0..2 {
                // Each 300 px lane's picture has a 1 px inset at both ends.
                let y = 321.0 + channel as f32 * 300.0 - axis.fraction_of_hz(hz) * 298.0;
                let Pane::Spectrum { channel: found, hz: actual } = l.locate(5.0, y, &p, 48_000) else { panic!("missing spectral lane") };
                assert_eq!(found, channel);
                assert!((actual - hz).abs() < 0.1, "{actual} vs {hz}");
            }
        }
    }

    #[test]
    fn hz_at_agrees_with_the_picture_rows() {
        for log_freq in [true, false] {
            for partial_log in [false, true] {
                let p = SpectrogramParams { log_freq, partial_log, ..SpectrogramParams::default() };
                for row in [0usize, 10, 128, 256] {
                    let t = 1.0 - row as f32 / 256.0;
                    let (a, b) = (p.hz_at(t, 44_100), p.row_edge_hz(row, 256, 44_100));
                    assert!((a - b).abs() <= b * 1e-4, "row {row}: {a} vs {b}");
                }
            }
        }
    }

    #[test]
    fn the_frame_under_the_pointer_follows_the_view_and_stays_in_the_file() {
        let l = layout(true, false, 1);
        let p = SpectrogramParams::default();
        let view = Viewport { start: 1000.0, frames_per_px: 10.0 };
        let r = Readout::at(&l, &view, &p, 48_000, 50_000, 100.0, 300.0).unwrap();
        assert_eq!(r.frame, 2000);
        let r = Readout::at(&l, &view, &p, 48_000, 1500, 100.0, 300.0).unwrap();
        assert_eq!(r.frame, 1500, "past the end clamps");
        assert!(Readout::at(&l, &view, &p, 48_000, 1500, 1500.0, 300.0).is_none());
        let r = Readout::at(&l, &Viewport { start: 0.0, frames_per_px: 48.0 }, &p, 48_000, 480_000, 500.0, 100.0).unwrap();
        assert_eq!(format_time(r.frame, 48_000), "0:00.500");
    }

    #[test]
    fn levels_in_dbfs() {
        assert_eq!(dbfs_text(1.0), "0.00 dBFS");
        assert_eq!(dbfs_text(-0.5), "-6.02 dBFS");
        assert_eq!(dbfs_text(0.0), "-inf dBFS");
        assert_eq!(dbfs(f32::NAN), None);
        assert_eq!(hz_text(440.0), "440 Hz");
        assert_eq!(hz_text(1500.0), "1.50 kHz");
    }

    #[test]
    fn the_readout_line_names_time_channel_level_and_frequency() {
        let w = Readout { frame: 24_000, pane: Pane::Wave { channel: Some(1), amplitude: 0.5 } };
        assert_eq!(w.text(48_000, Some(0.25)), "0:00.500  ch 2  +0.5000 (-6.02 dBFS)  sample +0.250000");
        let s = Readout { frame: 0, pane: Pane::Spectrum { channel: 0, hz: 1500.0 } };
        assert_eq!(s.text(48_000, None), "0:00.000  ch 1  1.50 kHz");
        assert_eq!(Readout { frame: 48_000, pane: Pane::Ruler }.text(48_000, None), "0:01.000");
    }

    #[test]
    fn durations() {
        assert_eq!(duration_text(350), "350 ms");
        assert_eq!(duration_text(2100), "2.1 s");
        assert_eq!(duration_text(65_000), "1 min 5 s");
    }
}
