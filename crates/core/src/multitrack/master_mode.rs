//! C171: the master output mode, mono / stereo / 5.1 (core only). The base (f118) lets a session
//! be made with a stereo or a 5.1 master. This adds the mono mode beside them: a small wrapper
//! enum over the model's `ChannelFormat` (the model has many formats; a master is one of three),
//! the channel count of each mode, parsing of the control verb's `mono|stereo|5.1`, and the
//! folds down to one channel.
//!
//! Convention: the fold to mono is the mean, `(L + R) / 2`, the same one the document editor uses
//! when it mixes a stereo clip into a mono document (`doc::edit`). A full-scale signal in both
//! channels stays at full scale. A 5.1 source goes to stereo through the base's
//! [`downmix_51_to_stereo`] (not repeated here) and then through that mean. Samples are not
//! checked: NaN and infinity pass through as they would in any sum.

use soundcraft_model::ChannelFormat;

use super::{MtError, downmix_51_to_stereo, err};

/// The modes as the verb spells them, for error messages.
pub const MODE_LIST: &str = "mono, stereo or 5.1";

/// How many channels the master output has.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MasterMode {
    Mono,
    Stereo,
    Surround51,
}

impl MasterMode {
    /// The number of channels: 1, 2 or 6.
    pub fn channels(self) -> usize {
        match self {
            MasterMode::Mono => 1,
            MasterMode::Stereo => 2,
            MasterMode::Surround51 => 6,
        }
    }

    /// The model's channel format for this mode.
    pub fn format(self) -> ChannelFormat {
        match self {
            MasterMode::Mono => ChannelFormat::Mono,
            MasterMode::Stereo => ChannelFormat::Stereo,
            MasterMode::Surround51 => ChannelFormat::Surround51,
        }
    }

    /// The mode for a model format; any format other than mono, stereo or 5.1 is refused.
    pub fn from_format(format: ChannelFormat) -> Result<MasterMode, MtError> {
        match format {
            ChannelFormat::Mono => Ok(MasterMode::Mono),
            ChannelFormat::Stereo => Ok(MasterMode::Stereo),
            ChannelFormat::Surround51 => Ok(MasterMode::Surround51),
            other => err(format!("a master is {MODE_LIST}, not {other:?}")),
        }
    }

    /// The mode for the verb's word: `mono`, `stereo` or `5.1`, in any letter case, nothing else
    /// (no spaces around it).
    pub fn parse(text: &str) -> Result<MasterMode, MtError> {
        if text.eq_ignore_ascii_case("mono") {
            Ok(MasterMode::Mono)
        } else if text.eq_ignore_ascii_case("stereo") {
            Ok(MasterMode::Stereo)
        } else if text.eq_ignore_ascii_case("5.1") {
            Ok(MasterMode::Surround51)
        } else {
            err(format!("master must be {MODE_LIST}, not {text:?}"))
        }
    }

    /// The verb's word for this mode: `mono`, `stereo` or `5.1`.
    pub fn name(self) -> &'static str {
        match self {
            MasterMode::Mono => "mono",
            MasterMode::Stereo => "stereo",
            MasterMode::Surround51 => "5.1",
        }
    }
}

/// One frame of stereo folded to mono: `(l + r) / 2`.
pub fn stereo_to_mono(l: f32, r: f32) -> f32 {
    (l + r) / 2.0
}

/// A stereo buffer (one `Vec` per channel, L then R) folded to one mono buffer. Refuses anything
/// but two channels of the same length; no frames at all is fine.
pub fn stereo_buffers_to_mono(channels: &[Vec<f32>]) -> Result<Vec<f32>, MtError> {
    let [l, r] = channels else {
        return err(format!("a stereo buffer needs 2 channels, got {}", channels.len()));
    };
    if l.len() != r.len() {
        return err(format!("the channels differ in length ({} frames and {} frames)", l.len(), r.len()));
    }
    Ok(l.iter().zip(r).map(|(a, b)| stereo_to_mono(*a, *b)).collect())
}

/// One 5.1 frame (L R C LFE Ls Rs) folded to mono: [`downmix_51_to_stereo`], then
/// [`stereo_to_mono`].
pub fn downmix_51_to_mono(frame: [f32; 6]) -> f32 {
    let [lo, ro] = downmix_51_to_stereo(frame);
    stereo_to_mono(lo, ro)
}

/// A 5.1 buffer (one `Vec` per channel, in L R C LFE Ls Rs order) folded to one mono buffer.
/// Refuses anything but six channels of the same length; no frames at all is fine.
pub fn buffers_51_to_mono(channels: &[Vec<f32>]) -> Result<Vec<f32>, MtError> {
    let [l, r, c, lfe, ls, rs] = channels else {
        return err(format!("a 5.1 buffer needs 6 channels, got {}", channels.len()));
    };
    let len = l.len();
    if [r, c, lfe, ls, rs].iter().any(|ch| ch.len() != len) {
        return err(format!("the channels differ in length (the first has {len} frames)"));
    }
    Ok(l.iter().zip(r).zip(c).zip(lfe).zip(ls).zip(rs).map(|(((((a, b), cc), dd), e), f)| downmix_51_to_mono([*a, *b, *cc, *dd, *e, *f])).collect())
}
