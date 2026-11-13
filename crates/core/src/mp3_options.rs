//! MP3 export options and the check that a sample rate, channel count and options can be
//! encoded as MP3. Pure: no file, no encoder. The bitrates and rates are the MP3 values the
//! encoder in `craft-mp3` takes; the channel rule is the v1 decision (mono or stereo only).

/// The bitrates an MP3 export may use, in kbps.
pub const MP3_BITRATES: [u32; 14] = [32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320];

/// The bitrate a new MP3 export starts with, in kbps.
pub const MP3_DEFAULT_BITRATE: u32 = 192;

/// The sample rates an MP3 export may use, in Hz.
const MP3_RATES: [u32; 3] = [32000, 44100, 48000];

/// The choices for an MP3 export: the bitrate in kbps, and whether to mix down to mono.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mp3Options {
    pub bitrate_kbps: u32,
    pub mono: bool,
}

impl Default for Mp3Options {
    fn default() -> Self {
        Self { bitrate_kbps: MP3_DEFAULT_BITRATE, mono: false }
    }
}

/// Checks that a source of `rate` Hz and `channels` channels can be exported as MP3 with
/// `options`. Gives the channel count the file will have (1 when `mono` is set, else the
/// source's), or the reason it cannot be exported.
pub fn mp3_check(rate: u32, channels: u16, options: Mp3Options) -> Result<u16, String> {
    if !MP3_RATES.contains(&rate) {
        return Err(format!("MP3 cannot store {rate} Hz; use 32000, 44100 or 48000 Hz"));
    }
    if channels != 1 && channels != 2 {
        return Err(format!("MP3 export takes mono or stereo; this has {channels} channels"));
    }
    if !MP3_BITRATES.contains(&options.bitrate_kbps) {
        return Err(format!("{} kbps is not an MP3 bitrate; choose one of {:?}", options.bitrate_kbps, MP3_BITRATES));
    }
    Ok(if options.mono { 1 } else { channels })
}
