//! The Levels meter's scale (v1 item B4): a 60 dB range ending at 0 dBFS, and the colour zones
//! with crossovers at -18 and -6 dBFS, as in Audition's Levels panel. Pure arithmetic; the app
//! draws the bars from these.

// Stub for wav-c-118-a: remove this allowance with the todo!() bodies.
#![allow(clippy::todo, dead_code)]

/// Range shown by the meter, in dB below 0 dBFS.
pub const RANGE_DB: f32 = 60.0;

/// The level, in dBFS, from which the bar is yellow.
pub const YELLOW_FROM_DB: f32 = -18.0;

/// The level, in dBFS, from which the bar is red.
pub const RED_FROM_DB: f32 = -6.0;

/// The colour zone a level falls in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Zone {
    Green,
    Yellow,
    Red,
}

/// The part of the meter's length filled by a level of `db` dBFS: 0.0 at -60 dBFS and below,
/// 1.0 at 0 dBFS and above, linear in dB between. NaN gives 0.0.
pub fn bar_fraction(db: f32) -> f32 {
    todo!("wav-c-118-a: bar_fraction({db})")
}

/// The colour zone of a level of `db` dBFS: `Red` from -6 dBFS up, `Yellow` from -18 dBFS up to
/// -6, `Green` below -18. A level exactly on a crossover takes the upper zone. NaN gives `Green`.
pub fn zone(db: f32) -> Zone {
    todo!("wav-c-118-a: zone({db})")
}
