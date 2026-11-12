//! The MP3 choices a Save As MP3 offers (roadmap A3.2): the bitrate and the mono option, and the
//! check that a file's sample rate, channel count and options can be encoded as MP3 at all. Pure:
//! no encoder runs and no file is touched here.

/// The bitrates (kbps) an MP3 file may use: the v1 list.
pub const MP3_BITRATES: [u32; 14] = [32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320];

/// The bitrate a new MP3 save starts with.
pub const MP3_DEFAULT_BITRATE: u32 = 192;

/// The sample rates (Hz) the MP3 encoder takes.
pub const MP3_RATES: [u32; 3] = [32_000, 44_100, 48_000];

/// The settings of one MP3 save.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mp3Options {
    /// Constant bitrate in kbps; one of [`MP3_BITRATES`].
    pub bitrate_kbps: u32,
    /// Write one channel even when the source has two.
    pub mono: bool,
}

impl Default for Mp3Options {
    fn default() -> Self {
        Self { bitrate_kbps: MP3_DEFAULT_BITRATE, mono: false }
    }
}

/// Checks that a file with `rate` Hz and `channels` channels can be saved as MP3 with `options`.
/// Returns the channel count the MP3 file will have, or the reason it cannot be saved.
pub fn mp3_check(rate: u32, channels: u16, options: Mp3Options) -> Result<u16, String> {
    if !MP3_RATES.contains(&rate) {
        return Err(format!("MP3 needs a sample rate of 32000, 44100 or 48000 Hz, not {rate} Hz"));
    }
    if !(1..=2).contains(&channels) {
        return Err(format!("MP3 takes mono or stereo, not {channels} channels"));
    }
    if !MP3_BITRATES.contains(&options.bitrate_kbps) {
        return Err(format!("MP3 has no {} kbps bitrate", options.bitrate_kbps));
    }
    Ok(if options.mono { 1 } else { channels })
}
