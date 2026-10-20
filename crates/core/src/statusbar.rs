//! The status bar's fields and which of them are shown. The set is kept in the settings file
//! (`status_fields = name,format,...`), changed from the status bar's right-click menu or the
//! control channel's `STATUSBAR show|hide <field>`.

use soundcraft_audio_io::SampleFormat;

/// One item the status bar can show.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StatusField {
    /// The file's name.
    Name,
    /// The file format (WAV, FLAC…).
    Format,
    SampleRate,
    /// The sample type: 16-bit, 24-bit, float…
    BitDepth,
    Channels,
    /// The size the audio takes uncompressed.
    FileSize,
    Duration,
    /// Free space in the session folder's disk.
    FreeSpace,
    /// The cursor, or the selection's start and end.
    Selection,
    /// The selection's length (when there is one).
    SelectionLength,
    /// The output device.
    Output,
}

impl StatusField {
    pub const ALL: [StatusField; 11] = [
        StatusField::Name,
        StatusField::Format,
        StatusField::SampleRate,
        StatusField::BitDepth,
        StatusField::Channels,
        StatusField::FileSize,
        StatusField::Duration,
        StatusField::FreeSpace,
        StatusField::Selection,
        StatusField::SelectionLength,
        StatusField::Output,
    ];

    /// The name used in the settings file and by the control channel.
    pub fn key(self) -> &'static str {
        match self {
            StatusField::Name => "name",
            StatusField::Format => "format",
            StatusField::SampleRate => "sample_rate",
            StatusField::BitDepth => "bit_depth",
            StatusField::Channels => "channels",
            StatusField::FileSize => "file_size",
            StatusField::Duration => "duration",
            StatusField::FreeSpace => "free_space",
            StatusField::Selection => "selection",
            StatusField::SelectionLength => "selection_length",
            StatusField::Output => "output",
        }
    }

    /// The name in the right-click menu.
    pub fn label(self) -> &'static str {
        match self {
            StatusField::Name => "File name",
            StatusField::Format => "File format",
            StatusField::SampleRate => "Sample rate",
            StatusField::BitDepth => "Bit depth",
            StatusField::Channels => "Channels",
            StatusField::FileSize => "File size (uncompressed)",
            StatusField::Duration => "Duration",
            StatusField::FreeSpace => "Free space",
            StatusField::Selection => "Cursor / selection",
            StatusField::SelectionLength => "Selection length",
            StatusField::Output => "Output device",
        }
    }

    /// The field called `key` (its settings name, ignoring case; `-` and spaces count as `_`).
    pub fn parse(key: &str) -> Option<StatusField> {
        let key = key.trim().to_ascii_lowercase().replace(['-', ' '], "_");
        StatusField::ALL.into_iter().find(|f| f.key() == key)
    }

    /// Shown in a new install: what the bar always showed, except size and free space.
    pub fn shown_by_default(self) -> bool {
        !matches!(self, StatusField::FileSize | StatusField::FreeSpace)
    }

    fn bit(self) -> u16 {
        1 << StatusField::ALL.iter().position(|f| *f == self).unwrap_or(0)
    }
}

/// Which fields are shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatusFields(u16);

impl Default for StatusFields {
    fn default() -> Self {
        StatusFields(StatusField::ALL.iter().filter(|f| f.shown_by_default()).fold(0, |bits, f| bits | f.bit()))
    }
}

impl StatusFields {
    pub fn is_shown(self, field: StatusField) -> bool {
        self.0 & field.bit() != 0
    }

    pub fn set(&mut self, field: StatusField, shown: bool) {
        if shown {
            self.0 |= field.bit();
        } else {
            self.0 &= !field.bit();
        }
    }

    /// The shown fields in the bar's order.
    pub fn shown(self) -> Vec<StatusField> {
        StatusField::ALL.into_iter().filter(|f| self.is_shown(*f)).collect()
    }

    /// `name,format,…` for the settings file; empty when none is shown.
    pub fn to_list(self) -> String {
        self.shown().iter().map(|f| f.key()).collect::<Vec<_>>().join(",")
    }

    /// The set a settings value names; names it does not know are ignored.
    pub fn parse_list(text: &str) -> StatusFields {
        let mut fields = StatusFields(0);
        for field in text.split(',').filter_map(StatusField::parse) {
            fields.set(field, true);
        }
        fields
    }
}

/// Bytes a sample takes uncompressed in the file's own sample type. A compressed source counts
/// as the 4 bytes a sample the editor holds it in.
pub fn bytes_per_sample(format: SampleFormat) -> u64 {
    match format {
        SampleFormat::Int8 => 1,
        SampleFormat::Int16 => 2,
        SampleFormat::Int24 => 3,
        SampleFormat::Int32 | SampleFormat::Float32 | SampleFormat::Compressed => 4,
        SampleFormat::Float64 => 8,
    }
}

/// "16-bit", "32-bit float", "compressed": the sample type for the status bar.
pub fn sample_label(format: SampleFormat) -> &'static str {
    match format {
        SampleFormat::Int8 => "8-bit",
        SampleFormat::Int16 => "16-bit",
        SampleFormat::Int24 => "24-bit",
        SampleFormat::Int32 => "32-bit",
        SampleFormat::Float32 => "32-bit float",
        SampleFormat::Float64 => "64-bit float",
        SampleFormat::Compressed => "compressed",
    }
}

/// The size of `frames` of `channels` channels uncompressed, in bytes.
pub fn uncompressed_bytes(frames: u64, channels: u64, format: SampleFormat) -> u64 {
    frames.saturating_mul(channels).saturating_mul(bytes_per_sample(format))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_install_shows_what_the_bar_always_showed() {
        let f = StatusFields::default();
        assert!(f.is_shown(StatusField::SampleRate) && f.is_shown(StatusField::Duration) && f.is_shown(StatusField::Selection));
        assert!(!f.is_shown(StatusField::FileSize) && !f.is_shown(StatusField::FreeSpace));
    }

    #[test]
    fn show_and_hide_change_only_that_field() {
        let mut f = StatusFields::default();
        f.set(StatusField::FileSize, true);
        f.set(StatusField::Channels, false);
        assert!(f.is_shown(StatusField::FileSize) && !f.is_shown(StatusField::Channels) && f.is_shown(StatusField::Duration));
        f.set(StatusField::FileSize, true);
        assert_eq!(f.shown().iter().filter(|x| **x == StatusField::FileSize).count(), 1);
    }

    #[test]
    fn the_settings_list_round_trips_and_ignores_what_it_does_not_know() {
        let mut f = StatusFields::default();
        f.set(StatusField::FreeSpace, true);
        f.set(StatusField::Name, false);
        assert_eq!(StatusFields::parse_list(&f.to_list()), f);
        assert_eq!(StatusFields::parse_list(""), StatusFields::parse_list(" , nonsense"), "none shown");
        assert!(StatusFields::parse_list("").shown().is_empty());
        let p = StatusFields::parse_list("Sample-Rate, file size ,bogus,duration");
        assert_eq!(p.shown(), vec![StatusField::SampleRate, StatusField::FileSize, StatusField::Duration]);
    }

    #[test]
    fn every_field_has_its_own_bit_and_a_name_that_parses_back() {
        let mut all = StatusFields::parse_list("");
        for f in StatusField::ALL {
            assert_eq!(StatusField::parse(f.key()), Some(f));
            assert!(!all.is_shown(f));
            all.set(f, true);
        }
        assert_eq!(all.shown().len(), StatusField::ALL.len());
    }

    #[test]
    fn size_is_frames_times_channels_times_the_sample_size() {
        assert_eq!(uncompressed_bytes(48_000, 2, SampleFormat::Int24), 288_000);
        assert_eq!(uncompressed_bytes(10, 1, SampleFormat::Float64), 80);
        assert_eq!(uncompressed_bytes(u64::MAX, 2, SampleFormat::Int16), u64::MAX);
        assert_eq!(sample_label(SampleFormat::Float32), "32-bit float");
        assert_eq!(sample_label(SampleFormat::Int24), "24-bit");
    }
}
