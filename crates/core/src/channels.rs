//! How the editor shows channels: layered (every channel drawn over the others in one lane) or
//! stacked (one lane each), what each channel is called, and the colour it is drawn in.
//!
//! Names come from the file's speaker mask when it has one (a WAV `WAVE_FORMAT_EXTENSIBLE`
//! header), else `L`, `R` for two channels, else `1`..`N`. The palette is our own.

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

/// Short names for the speaker bits of a WAV channel mask, bit 0 upward. The surround pair at
/// bits 4/5 (back) is called `Ls`/`Rs` unless the side pair (bits 9/10) is also present.
fn speaker_name(bit: u32, has_side: bool) -> Option<&'static str> {
    Some(match bit {
        0 => "L",
        1 => "R",
        2 => "C",
        3 => "LFE",
        4 if has_side => "Lb",
        4 => "Ls",
        5 if has_side => "Rb",
        5 => "Rs",
        6 => "Lc",
        7 => "Rc",
        8 => "Cb",
        9 => "Ls",
        10 => "Rs",
        _ => return None,
    })
}

/// The name of each of `count` channels. `mask` is the file's speaker mask when known; it is
/// used only when it names exactly `count` speakers we have a name for.
pub fn channel_labels(count: usize, mask: Option<u32>) -> Vec<String> {
    if let Some(mask) = mask.filter(|m| m.count_ones() as usize == count) {
        let has_side = mask & (0b11 << 9) != 0;
        let names: Vec<Option<&str>> = (0..32).filter(|b| mask & (1 << b) != 0).map(|b| speaker_name(b, has_side)).collect();
        if names.iter().all(Option::is_some) {
            return names.into_iter().flatten().map(str::to_string).collect();
        }
    }
    match count {
        1 => vec!["M".to_string()],
        2 => vec!["L".to_string(), "R".to_string()],
        n => (1..=n).map(|i| i.to_string()).collect(),
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
    fn stereo_without_a_mask_is_l_r_and_mono_is_m() {
        assert_eq!(channel_labels(2, None), ["L", "R"]);
        assert_eq!(channel_labels(1, None), ["M"]);
    }

    #[test]
    fn other_counts_without_a_mask_are_numbered_from_one() {
        assert_eq!(channel_labels(6, None), ["1", "2", "3", "4", "5", "6"]);
        assert_eq!(channel_labels(3, None), ["1", "2", "3"]);
    }

    #[test]
    fn a_5_1_mask_gives_l_r_c_lfe_ls_rs_for_back_or_side_pairs() {
        assert_eq!(channel_labels(6, Some(0x3F)), ["L", "R", "C", "LFE", "Ls", "Rs"]);
        assert_eq!(channel_labels(6, Some(0x60F)), ["L", "R", "C", "LFE", "Ls", "Rs"]);
    }

    #[test]
    fn a_7_1_mask_tells_back_from_side() {
        assert_eq!(channel_labels(8, Some(0x63F)), ["L", "R", "C", "LFE", "Lb", "Rb", "Ls", "Rs"]);
    }

    #[test]
    fn a_mask_that_does_not_fit_the_count_is_ignored() {
        assert_eq!(channel_labels(6, Some(0x3)), ["1", "2", "3", "4", "5", "6"]);
        assert_eq!(channel_labels(2, Some(0)), ["L", "R"]);
        // Two speakers we have no short name for (top front left / right): numbered.
        assert_eq!(channel_labels(2, Some(0x1000 | 0x4000)), ["L", "R"]);
        assert_eq!(channel_labels(3, Some(0x1000 | 0x4000 | 0x8000)), ["1", "2", "3"]);
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
