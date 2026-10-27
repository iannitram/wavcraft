//! Many-channel files (roadmap A134): speaker labels from a `WAVE_FORMAT_EXTENSIBLE` channel
//! mask, and the fold-down of a file's channels to a stereo output.
//!
//! The mask is the `dwChannelMask` of the WAV `fmt ` chunk: bit `n` set means the file has the
//! speaker with that bit, and the file's channels run in ascending bit order. A mask whose
//! number of set bits is not the channel count is not trusted (labels fall back to 1..N).

/// Most channels a file may have (WAV open and save).
pub const MAX_CHANNELS: usize = 32;

/// The standard speaker positions, by mask bit 0 to 17 (Microsoft's `SPEAKER_*` order).
const SPEAKERS: [&str; 18] = ["FL", "FR", "FC", "LFE", "BL", "BR", "FLC", "FRC", "BC", "SL", "SR", "TC", "TFL", "TFC", "TFR", "TBL", "TBC", "TBR"];

/// The mask `mask` as a usable one for `channels` channels: `None` when it is 0 (unlabelled) or
/// has a different number of speakers than the file has channels.
pub fn usable_mask(mask: Option<u32>, channels: usize) -> Option<u32> {
    mask.filter(|m| *m != 0 && m.count_ones() as usize == channels)
}

/// One label per channel: the speaker names from the mask in channel order ("FL", "FR", "FC",
/// "LFE", "BL", "BR" for 5.1), or "1".."N" when there is no usable mask. Speaker bits above 17
/// (not in the standard list) are "Sp18"… .
pub fn labels(mask: Option<u32>, channels: usize) -> Vec<String> {
    let Some(mask) = usable_mask(mask, channels) else {
        return (1..=channels).map(|n| n.to_string()).collect();
    };
    (0..32u32).filter(|b| mask & (1 << b) != 0).map(|b| SPEAKERS.get(b as usize).map_or_else(|| format!("Sp{b}"), |s| (*s).to_string())).collect()
}

/// How one file channel reaches the left and right outputs.
pub type Gains = [f32; 2];

/// The stereo fold-down of a `channels`-channel file, one `[left, right]` gain per channel.
///
/// - 1 or 2 channels: mono on both sides, or as is.
/// - 5.1 or 7.1 with a usable mask (front left and right present): ITU-style, front centre and
///   the surround and back speakers at -3 dB (0.7071) on their own side (a back centre on both
///   sides at 0.5, the centre-ish fronts like the centre), the LFE dropped; each output is
///   divided by the sum of its gains so a full-scale file cannot clip.
/// - Anything else (unlabelled, ambisonic-like, other layouts): the first two channels as left
///   and right; the rest is not played.
pub fn stereo_downmix(mask: Option<u32>, channels: usize) -> Vec<Gains> {
    let mut gains = vec![[0.0f32; 2]; channels];
    match channels {
        0 => {}
        1 => gains[0] = [1.0, 1.0],
        _ => match usable_mask(mask, channels).filter(|m| channels > 2 && matches!(channels, 6 | 8) && m & 0b11 == 0b11) {
            Some(mask) => {
                const S: f32 = std::f32::consts::FRAC_1_SQRT_2;
                let bits: Vec<u32> = (0..32u32).filter(|b| mask & (1 << b) != 0).collect();
                for (g, bit) in gains.iter_mut().zip(&bits) {
                    *g = match bit {
                        0 => [1.0, 0.0],     // FL
                        1 => [0.0, 1.0],     // FR
                        2 | 6 | 7 => [S, S], // FC and the front-of-centre pair
                        3 => [0.0, 0.0],     // LFE
                        4 | 9 => [S, 0.0],   // BL, SL
                        5 | 10 => [0.0, S],  // BR, SR
                        8 => [0.5, 0.5],     // BC
                        _ => [0.0, 0.0],
                    };
                }
                let sums = gains.iter().fold([0.0f32; 2], |s, g| [s[0] + g[0], s[1] + g[1]]);
                for g in &mut gains {
                    g[0] /= sums[0].max(f32::MIN_POSITIVE);
                    g[1] /= sums[1].max(f32::MIN_POSITIVE);
                }
            }
            None => {
                gains[0] = [1.0, 0.0];
                gains[1] = [0.0, 1.0];
            }
        },
    }
    gains
}

/// Layered or stacked waveform channels (View option, kept in the settings file).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ChannelLayout {
    /// One lane per channel (the original display).
    #[default]
    Stacked,
    /// Every channel drawn in one lane, each in its own colour.
    Layered,
}

impl ChannelLayout {
    pub fn name(self) -> &'static str {
        match self {
            ChannelLayout::Stacked => "stacked",
            ChannelLayout::Layered => "layered",
        }
    }

    pub fn parse(s: &str) -> Option<ChannelLayout> {
        match s.trim().to_ascii_lowercase().as_str() {
            "stacked" => Some(ChannelLayout::Stacked),
            "layered" => Some(ChannelLayout::Layered),
            _ => None,
        }
    }
}

/// The colour (red, green, blue) channel `ch` is drawn in. Eight distinct hues, then repeated.
pub fn channel_color(ch: usize) -> [u8; 3] {
    const PALETTE: [[u8; 3]; 8] = [
        [90, 200, 120],  // green
        [235, 140, 70],  // orange
        [90, 170, 235],  // blue
        [225, 95, 140],  // rose
        [200, 190, 80],  // olive
        [160, 120, 230], // violet
        [80, 205, 205],  // teal
        [210, 210, 210], // grey
    ];
    PALETTE[ch % PALETTE.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_follow_the_mask_or_count_from_one() {
        assert_eq!(labels(Some(0x3F), 6), ["FL", "FR", "FC", "LFE", "BL", "BR"]);
        assert_eq!(labels(Some(0x63F), 8), ["FL", "FR", "FC", "LFE", "BL", "BR", "SL", "SR"]);
        assert_eq!(labels(None, 3), ["1", "2", "3"]);
        assert_eq!(labels(Some(0), 2), ["1", "2"]);
        assert_eq!(labels(Some(0x3F), 5), ["1", "2", "3", "4", "5"], "a mask that disagrees with the count is not trusted");
        assert_eq!(labels(Some(1 << 20 | 1), 2), ["FL", "Sp20"]);
    }

    #[test]
    fn five_one_folds_to_stereo_without_clipping_and_drops_the_lfe() {
        let g = stereo_downmix(Some(0x3F), 6);
        assert_eq!(g[0][1], 0.0);
        assert_eq!(g[3], [0.0, 0.0], "LFE");
        let sums = g.iter().fold([0.0f32; 2], |s, x| [s[0] + x[0], s[1] + x[1]]);
        assert!((sums[0] - 1.0).abs() < 1e-6 && (sums[1] - 1.0).abs() < 1e-6, "{sums:?}");
        assert!(g[2][0] > 0.0 && g[2][0] == g[2][1], "centre on both sides");
        let g8 = stereo_downmix(Some(0x63F), 8);
        assert!(g8[6][0] > 0.0 && g8[6][1] == 0.0, "SL on the left");
        assert!(g8[7][1] > 0.0 && g8[7][0] == 0.0, "SR on the right");
    }

    #[test]
    fn other_layouts_play_their_first_two_channels() {
        assert_eq!(stereo_downmix(None, 6), [[1.0, 0.0], [0.0, 1.0], [0.0; 2], [0.0; 2], [0.0; 2], [0.0; 2]]);
        assert_eq!(stereo_downmix(None, 32).iter().filter(|g| **g != [0.0, 0.0]).count(), 2);
        assert_eq!(stereo_downmix(None, 1), [[1.0, 1.0]]);
        assert_eq!(stereo_downmix(Some(3), 2), [[1.0, 0.0], [0.0, 1.0]]);
        assert!(stereo_downmix(None, 0).is_empty());
    }

    #[test]
    fn colours_differ_for_the_first_eight_channels() {
        let mut seen = std::collections::HashSet::new();
        for ch in 0..8 {
            assert!(seen.insert(channel_color(ch)));
        }
        assert_eq!(channel_color(8), channel_color(0));
    }

    #[test]
    fn layout_names_round_trip() {
        for l in [ChannelLayout::Stacked, ChannelLayout::Layered] {
            assert_eq!(ChannelLayout::parse(l.name()), Some(l));
        }
        assert_eq!(ChannelLayout::parse("sideways"), None);
    }
}
