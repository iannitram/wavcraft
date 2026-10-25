//! The multitrack time ruler as a pure model (row C083): given a visible time range in seconds, the
//! pixel width available and the least room one label needs, pick a "nice" tick step and list the
//! ticks (time, major or minor, label). No drawing and no UI types live here.
//!
//! Two label formats: decimal seconds (`12s`, `0.25s`) and minutes:seconds (`1:05`, `1:00:00`,
//! with `.mmm` milliseconds only when the step is under one second). Steps come from fixed tables.
//! A tick is major when it lies on the next coarser step in the table (the last step has none, so
//! all its ticks are major). A range that would need more than [`MAX_TICKS`] ticks is refused, so a
//! huge range cannot allocate without bound; times above [`MAX_SECONDS`] are refused too.

use std::fmt;

/// The most ticks one ruler may hold.
pub const MAX_TICKS: usize = 2000;
/// The latest time (seconds) the ruler accepts, about 31 years.
pub const MAX_SECONDS: f64 = 1.0e9;

/// Decimal-seconds steps in milliseconds: 1, 2 and 5 times a power of ten, 1 ms to 5,000,000 s.
const DECIMAL_STEPS_MS: &[u64] = &[
    1,
    2,
    5,
    10,
    20,
    50,
    100,
    200,
    500,
    1000,
    2000,
    5000,
    10_000,
    20_000,
    50_000,
    100_000,
    200_000,
    500_000,
    1_000_000,
    2_000_000,
    5_000_000,
    10_000_000,
    20_000_000,
    50_000_000,
    100_000_000,
    200_000_000,
    500_000_000,
    1_000_000_000,
    2_000_000_000,
    5_000_000_000,
];
/// Minutes:seconds steps in milliseconds: sub-second steps, then 1, 2, 5, 10, 15, 30 s, 1, 2, 5, 10, 15, 30 min, 1 h.
const MINSEC_STEPS_MS: &[u64] =
    &[1, 2, 5, 10, 20, 50, 100, 200, 500, 1000, 2000, 5000, 10_000, 15_000, 30_000, 60_000, 120_000, 300_000, 600_000, 900_000, 1_800_000, 3_600_000];

/// How tick labels are written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RulerFormat {
    /// `12s`, `0.25s`.
    Decimal,
    /// `m:ss`, `h:mm:ss`, with `.mmm` when the step is under one second.
    MinSec,
}

/// Why the ruler refused its input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RulerError {
    NotFinite,
    NegativeTime,
    EmptyRange,
    ZeroWidth,
    ZeroSpacing,
    TimeTooLarge,
    NonPositiveStep,
    StepOutOfRange,
    TooManyTicks,
}

impl fmt::Display for RulerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RulerError::NotFinite => f.write_str("ruler input is not a finite number"),
            RulerError::NegativeTime => f.write_str("ruler time is negative"),
            RulerError::EmptyRange => f.write_str("ruler range is empty or reversed: the end must be after the start"),
            RulerError::ZeroWidth => f.write_str("ruler width must be more than 0 pixels"),
            RulerError::ZeroSpacing => f.write_str("ruler label spacing must be more than 0 pixels"),
            RulerError::TimeTooLarge => write!(f, "ruler time is beyond {MAX_SECONDS} seconds"),
            RulerError::NonPositiveStep => f.write_str("ruler step must be more than 0 seconds"),
            RulerError::StepOutOfRange => write!(f, "no ruler step is wide enough for that label spacing"),
            RulerError::TooManyTicks => write!(f, "that range needs more than {MAX_TICKS} ruler ticks"),
        }
    }
}

impl std::error::Error for RulerError {}

/// One tick: where it is, whether it is a major tick, and its label.
#[derive(Clone, Debug, PartialEq)]
pub struct Tick {
    pub seconds: f64,
    pub major: bool,
    pub label: String,
}

/// The chosen step (seconds) and the ticks, in increasing time order.
#[derive(Clone, Debug, PartialEq)]
pub struct Ruler {
    pub step: f64,
    pub ticks: Vec<Tick>,
}

/// The step table for `format`.
fn steps_ms(format: RulerFormat) -> &'static [u64] {
    match format {
        RulerFormat::Decimal => DECIMAL_STEPS_MS,
        RulerFormat::MinSec => MINSEC_STEPS_MS,
    }
}

/// Refuse a range, width or spacing the ruler cannot use.
pub fn validate_ruler_input(start: f64, end: f64, width_px: f64, min_label_px: f64) -> Result<(), RulerError> {
    if !start.is_finite() || !end.is_finite() || !width_px.is_finite() || !min_label_px.is_finite() {
        return Err(RulerError::NotFinite);
    }
    if start < 0.0 {
        return Err(RulerError::NegativeTime);
    }
    if end <= start {
        return Err(RulerError::EmptyRange);
    }
    if end > MAX_SECONDS {
        return Err(RulerError::TimeTooLarge);
    }
    if width_px <= 0.0 {
        return Err(RulerError::ZeroWidth);
    }
    if min_label_px <= 0.0 {
        return Err(RulerError::ZeroSpacing);
    }
    Ok(())
}

/// The smallest table step, in seconds, that is at least `min_seconds`.
pub fn nice_step(min_seconds: f64, format: RulerFormat) -> Result<f64, RulerError> {
    nice_step_ms(min_seconds, format).map(|ms| ms as f64 / 1000.0)
}

/// The smallest table step, in milliseconds, that is at least `min_seconds`.
fn nice_step_ms(min_seconds: f64, format: RulerFormat) -> Result<u64, RulerError> {
    if !min_seconds.is_finite() {
        return Err(RulerError::NotFinite);
    }
    if min_seconds <= 0.0 {
        return Err(RulerError::NonPositiveStep);
    }
    let table = steps_ms(format);
    for &s in table {
        if (s as f64) / 1000.0 >= min_seconds {
            return Ok(s);
        }
    }
    Err(RulerError::StepOutOfRange)
}

/// The next larger table step after `step_ms`, or `None` for the largest (or an unknown) step.
fn coarser_step_ms(step_ms: u64, format: RulerFormat) -> Option<u64> {
    let table = steps_ms(format);
    let i = table.iter().position(|s| *s == step_ms)?;
    table.get(i + 1).copied()
}

/// The label for a tick at `ms` milliseconds when the step is `step_ms`.
fn label_for(ms: u64, step_ms: u64, format: RulerFormat) -> String {
    // 1. how many fraction digits are shown
    let decimals: u32 = match format {
        RulerFormat::Decimal => {
            if step_ms >= 1000 {
                0
            } else if step_ms >= 100 {
                1
            } else if step_ms >= 10 {
                2
            } else {
                3
            }
        }
        RulerFormat::MinSec => {
            if step_ms >= 1000 {
                0
            } else {
                3
            }
        }
    };
    // 2. the size in milliseconds of the last digit shown: 1000, 100, 10 or 1
    let unit: u64 = match decimals {
        0 => 1000,
        1 => 100,
        2 => 10,
        _ => 1,
    };
    // 3. round ms half-up to a multiple of unit
    let rounded = (ms + unit / 2) / unit * unit;
    // 4. whole seconds and the fraction digits (an integer; zero-padded to `decimals` digits when printed)
    let whole = rounded / 1000;
    let frac = rounded % 1000 / unit;
    // 5. write it
    match format {
        RulerFormat::Decimal => {
            if decimals == 0 {
                format!("{whole}s")
            } else {
                format!("{whole}.{frac:0width$}s", width = decimals as usize)
            }
        }
        RulerFormat::MinSec => {
            let h = whole / 3600;
            let m = whole / 60 % 60;
            let s = whole % 60;
            let base = if h > 0 { format!("{h}:{m:02}:{s:02}") } else { format!("{m}:{s:02}") };
            if decimals == 3 { format!("{base}.{frac:03}") } else { base }
        }
    }
}

/// The label for a tick at `seconds` when the step is `step` seconds.
pub fn format_label(seconds: f64, step: f64, format: RulerFormat) -> Result<String, RulerError> {
    if !seconds.is_finite() || !step.is_finite() {
        return Err(RulerError::NotFinite);
    }
    if seconds < 0.0 {
        return Err(RulerError::NegativeTime);
    }
    if seconds > MAX_SECONDS {
        return Err(RulerError::TimeTooLarge);
    }
    if step <= 0.0 {
        return Err(RulerError::NonPositiveStep);
    }

    let ms = (seconds * 1000.0).round() as u64;
    let step_ms = (step * 1000.0).round() as u64;

    Ok(label_for(ms, step_ms, format))
}

/// The ticks for the visible range `start..=end` (seconds) over `width_px` pixels, with labels at
/// least `min_label_px` pixels apart.
pub fn ruler_ticks(start: f64, end: f64, width_px: f64, min_label_px: f64, format: RulerFormat) -> Result<Ruler, RulerError> {
    validate_ruler_input(start, end, width_px, min_label_px)?;

    let min_seconds = (end - start) / width_px * min_label_px;
    let step_ms = nice_step_ms(min_seconds, format)?;
    let step_f = step_ms as f64;
    let step = step_f / 1000.0;

    let first = (start * 1000.0 / step_f - 1.0e-6).ceil();
    let last = (end * 1000.0 / step_f + 1.0e-6).floor();

    if last < first {
        return Ok(Ruler { step, ticks: Vec::new() });
    }

    if last - first + 1.0 > MAX_TICKS as f64 {
        return Err(RulerError::TooManyTicks);
    }

    let coarser = coarser_step_ms(step_ms, format);
    let mut ticks = Vec::new();

    for k in (first as u64)..=(last as u64) {
        let ms = k * step_ms;
        let seconds = ms as f64 / 1000.0;
        let major = match coarser {
            Some(c) => ms % c == 0,
            None => true,
        };
        let label = label_for(ms, step_ms, format);
        ticks.push(Tick { seconds, major, label });
    }

    Ok(Ruler { step, ticks })
}
