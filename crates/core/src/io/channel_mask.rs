//! Channel mask utilities for WAV file channel layout conversion.
//!
//! This module provides functions to convert between channel masks and
//! speaker layouts for WAV files. The bit ordering follows the
//! WAVEFORMATEXTENSIBLE standard where bit n represents speaker n.

/// Enum representing individual speakers in a channel layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Speaker {
    FrontLeft,
    FrontRight,
    FrontCenter,
    LowFrequency,
    BackLeft,
    BackRight,
    FrontLeftOfCenter,
    FrontRightOfCenter,
    BackCenter,
    SideLeft,
    SideRight,
    TopCenter,
    TopFrontLeft,
    TopFrontCenter,
    TopFrontRight,
    TopBackLeft,
    TopBackCenter,
    TopBackRight,
}

/// Enum representing channel layouts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelLayout {
    Unspecified,
    Mono,
    Stereo,
    Quad,
    Surround51,
    Surround51Side,
    Surround71,
    Custom(u32),
}

/// Converts a channel mask to a vector of speakers in ascending bit order.
///
/// Bits above 17 are ignored.
pub fn speakers_from_mask(mask: u32) -> Vec<Speaker> {
    const ALL: [Speaker; 18] = [
        Speaker::FrontLeft,
        Speaker::FrontRight,
        Speaker::FrontCenter,
        Speaker::LowFrequency,
        Speaker::BackLeft,
        Speaker::BackRight,
        Speaker::FrontLeftOfCenter,
        Speaker::FrontRightOfCenter,
        Speaker::BackCenter,
        Speaker::SideLeft,
        Speaker::SideRight,
        Speaker::TopCenter,
        Speaker::TopFrontLeft,
        Speaker::TopFrontCenter,
        Speaker::TopFrontRight,
        Speaker::TopBackLeft,
        Speaker::TopBackCenter,
        Speaker::TopBackRight,
    ];

    ALL.iter().enumerate().filter(|(i, _)| mask & (1u32 << i) != 0).map(|(_, s)| *s).collect()
}

/// Converts a slice of speakers to a channel mask by OR-ing their bits.
///
/// Duplicates are removed and order does not matter.
pub fn mask_from_speakers(speakers: &[Speaker]) -> u32 {
    let mut mask = 0u32;
    for speaker in speakers {
        mask |= match speaker {
            Speaker::FrontLeft => 1 << 0,
            Speaker::FrontRight => 1 << 1,
            Speaker::FrontCenter => 1 << 2,
            Speaker::LowFrequency => 1 << 3,
            Speaker::BackLeft => 1 << 4,
            Speaker::BackRight => 1 << 5,
            Speaker::FrontLeftOfCenter => 1 << 6,
            Speaker::FrontRightOfCenter => 1 << 7,
            Speaker::BackCenter => 1 << 8,
            Speaker::SideLeft => 1 << 9,
            Speaker::SideRight => 1 << 10,
            Speaker::TopCenter => 1 << 11,
            Speaker::TopFrontLeft => 1 << 12,
            Speaker::TopFrontCenter => 1 << 13,
            Speaker::TopFrontRight => 1 << 14,
            Speaker::TopBackLeft => 1 << 15,
            Speaker::TopBackCenter => 1 << 16,
            Speaker::TopBackRight => 1 << 17,
        };
    }
    mask
}

/// Converts a channel mask to a channel layout.
pub fn layout_from_mask(mask: u32) -> ChannelLayout {
    match mask {
        0x0 => ChannelLayout::Unspecified,
        0x4 => ChannelLayout::Mono,
        0x3 => ChannelLayout::Stereo,
        0x33 => ChannelLayout::Quad,
        0x3F => ChannelLayout::Surround51,
        0x60F => ChannelLayout::Surround51Side,
        0x63F => ChannelLayout::Surround71,
        _ => ChannelLayout::Custom(mask),
    }
}

/// Converts a channel layout to a channel mask.
pub fn mask_from_layout(layout: ChannelLayout) -> u32 {
    match layout {
        ChannelLayout::Unspecified => 0,
        ChannelLayout::Mono => 0x4,
        ChannelLayout::Stereo => 0x3,
        ChannelLayout::Quad => 0x33,
        ChannelLayout::Surround51 => 0x3F,
        ChannelLayout::Surround51Side => 0x60F,
        ChannelLayout::Surround71 => 0x63F,
        ChannelLayout::Custom(mask) => mask,
    }
}

/// Counts the number of speakers in a channel mask.
///
/// Bits above 17 are ignored.
pub fn channel_count(mask: u32) -> u32 {
    (mask & 0x3FFFF).count_ones()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_from_mask_known_table() {
        assert_eq!(layout_from_mask(0x0), ChannelLayout::Unspecified);
        assert_eq!(layout_from_mask(0x4), ChannelLayout::Mono);
        assert_eq!(layout_from_mask(0x3), ChannelLayout::Stereo);
        assert_eq!(layout_from_mask(0x33), ChannelLayout::Quad);
        assert_eq!(layout_from_mask(0x3F), ChannelLayout::Surround51);
        assert_eq!(layout_from_mask(0x60F), ChannelLayout::Surround51Side);
        assert_eq!(layout_from_mask(0x63F), ChannelLayout::Surround71);
        assert_eq!(layout_from_mask(0x7), ChannelLayout::Custom(0x7));
        assert_eq!(layout_from_mask(0xFFFF_FFFF), ChannelLayout::Custom(0xFFFF_FFFF));
        assert_eq!(layout_from_mask(0x1), ChannelLayout::Custom(0x1));
    }

    #[test]
    fn layout_from_mask_unknown_is_custom() {
        assert_eq!(layout_from_mask(0x7), ChannelLayout::Custom(0x7));
        assert_eq!(layout_from_mask(0x1), ChannelLayout::Custom(0x1));
        assert_eq!(layout_from_mask(0x3FFFF), ChannelLayout::Custom(0x3FFFF));
    }

    #[test]
    fn mask_from_layout_table() {
        assert_eq!(mask_from_layout(ChannelLayout::Unspecified), 0);
        assert_eq!(mask_from_layout(ChannelLayout::Mono), 0x4);
        assert_eq!(mask_from_layout(ChannelLayout::Stereo), 0x3);
        assert_eq!(mask_from_layout(ChannelLayout::Quad), 0x33);
        assert_eq!(mask_from_layout(ChannelLayout::Surround51), 0x3F);
        assert_eq!(mask_from_layout(ChannelLayout::Surround51Side), 0x60F);
        assert_eq!(mask_from_layout(ChannelLayout::Surround71), 0x63F);
        assert_eq!(mask_from_layout(ChannelLayout::Custom(0x7)), 0x7);
    }

    #[test]
    fn layout_round_trip() {
        let layouts = [
            ChannelLayout::Unspecified,
            ChannelLayout::Mono,
            ChannelLayout::Stereo,
            ChannelLayout::Quad,
            ChannelLayout::Surround51,
            ChannelLayout::Surround51Side,
            ChannelLayout::Surround71,
        ];

        for layout in layouts {
            assert_eq!(layout_from_mask(mask_from_layout(layout)), layout);
        }
    }

    #[test]
    fn speakers_in_bit_order_51() {
        let speakers = speakers_from_mask(0x3F);
        assert_eq!(speakers.len(), 6);
        assert_eq!(speakers[0], Speaker::FrontLeft);
        assert_eq!(speakers[1], Speaker::FrontRight);
        assert_eq!(speakers[2], Speaker::FrontCenter);
        assert_eq!(speakers[3], Speaker::LowFrequency);
        assert_eq!(speakers[4], Speaker::BackLeft);
        assert_eq!(speakers[5], Speaker::BackRight);
    }

    #[test]
    fn speakers_in_bit_order_71() {
        let speakers = speakers_from_mask(0x63F);
        assert_eq!(speakers.len(), 8);
        assert_eq!(speakers[0], Speaker::FrontLeft);
        assert_eq!(speakers[1], Speaker::FrontRight);
        assert_eq!(speakers[2], Speaker::FrontCenter);
        assert_eq!(speakers[3], Speaker::LowFrequency);
        assert_eq!(speakers[4], Speaker::BackLeft);
        assert_eq!(speakers[5], Speaker::BackRight);
        assert_eq!(speakers[6], Speaker::SideLeft);
        assert_eq!(speakers[7], Speaker::SideRight);
    }

    #[test]
    fn speakers_mono_and_empty() {
        let speakers = speakers_from_mask(0x4);
        assert_eq!(speakers, [Speaker::FrontCenter]);

        let speakers = speakers_from_mask(0);
        assert_eq!(speakers, []);

        let speakers = speakers_from_mask(0x3);
        assert_eq!(speakers, [Speaker::FrontLeft, Speaker::FrontRight]);
    }

    #[test]
    fn speakers_ignore_high_bits() {
        let speakers = speakers_from_mask(0x4_0003);
        assert_eq!(speakers, [Speaker::FrontLeft, Speaker::FrontRight]);

        let speakers = speakers_from_mask(0x3FFFF);
        assert_eq!(speakers.len(), 18);
        assert_eq!(speakers[17], Speaker::TopBackRight);
    }

    #[test]
    fn mask_from_speakers_dedups_and_ors() {
        let mask = mask_from_speakers(&[Speaker::SideRight, Speaker::FrontLeft, Speaker::FrontLeft]);
        assert_eq!(mask, 0x401);

        let mask = mask_from_speakers(&[]);
        assert_eq!(mask, 0);

        let speakers = speakers_from_mask(0x63F);
        let mask = mask_from_speakers(&speakers);
        assert_eq!(mask, 0x63F);
    }

    #[test]
    fn speakers_mask_round_trip() {
        let mask = 0x63F;
        let speakers = speakers_from_mask(mask);
        let new_mask = mask_from_speakers(&speakers);
        assert_eq!(new_mask, mask);
    }

    #[test]
    fn channel_count_table() {
        assert_eq!(channel_count(0), 0);
        assert_eq!(channel_count(0x4), 1);
        assert_eq!(channel_count(0x3), 2);
        assert_eq!(channel_count(0x3F), 6);
        assert_eq!(channel_count(0x63F), 8);
        assert_eq!(channel_count(0xFFFF_FFFF), 18);
    }
}
