//! An edit and a history traversal use the same overview path. Shifted waveform
//! blocks retain their source grid; FFT blocks explicitly report whether they are fresh.
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use wavcraft_core::doc::edit::{self, ChannelMask, Crossfades, PasteAt};
use wavcraft_core::doc::history::History;
use wavcraft_core::doc::span::EditSpan;
use wavcraft_core::document::{Document, StoreChannel, WavePeaks};
use wavcraft_core::spectrogram::{self, Columns, Overview, Samples, SpectrogramParams};
use wavcraft_core::{AudioBuffer, DocState};

const NONE: Crossfades = Crossfades::NONE;

fn spiky(frames: usize) -> AudioBuffer {
    let mut seed = 0x2545_f491u32;
    let channels = (0..2)
        .map(|ch| {
            (0..frames)
                .map(|i| {
                    seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                    let noise = (seed >> 8) as f32 / (1u32 << 24) as f32 - 0.5;
                    let spike = if i % 99_991 == 1_234 * (ch + 1) {
                        0.99
                    } else if i % 65_521 == 7 {
                        -0.97
                    } else {
                        0.0
                    };
                    (0.5 * (i as f32 * 0.002 * (ch + 1) as f32).sin() + 0.3 * noise + spike).clamp(-1.0, 1.0)
                })
                .collect()
        })
        .collect();
    AudioBuffer { sample_rate: 48_000, channels }
}

fn verify_step(doc: &mut Document, overviews: &mut [Overview], state: DocState, params: &SpectrogramParams) {
    let span = EditSpan::between(&doc.state, &state);
    *doc = doc.after_step(state).unwrap();
    for (ch, overview) in overviews.iter_mut().enumerate() {
        assert_eq!(doc.peaks[ch], WavePeaks::build(&doc.state, ch).unwrap(), "all waveform levels, channel {ch}, {span:?}");
        let samples = StoreChannel::new(&doc.state, ch);
        overview.after_step(span, samples.len());
        let full = Overview::build_from(&samples, params);
        for ((fresh, got), (full_fresh, want)) in overview.blocks().zip(full.blocks()) {
            assert!(full_fresh);
            if fresh {
                assert_eq!(got, want, "fresh FFT block, channel {ch}, {span:?}");
            }
        }
        if span.inserted != span.removed && (span.inserted as i128 - span.removed as i128) % (8 * params.fft_size) as i128 != 0 {
            assert!(overview.blocks().any(|(fresh, _)| !fresh), "shifted FFT blocks must be marked stale");
        }
        // Refresh a view across the splice and compare the rendered pixels as well as blocks.
        let columns =
            Columns::covering(span.at.saturating_sub(30_000) as f64, (span.at + 40_000).min(doc.state.len_frames()) as f64, 8_000.0, params);
        overview.refresh_visible(&samples, params, columns);
        assert_eq!(
            spectrogram::render_columns_from(&samples, doc.sample_rate(), columns, 32, params, Some(overview)),
            spectrogram::render_columns_from(&samples, doc.sample_rate(), columns, 32, params, Some(&full))
        );
        overview.refresh_blocks(&samples, params, 0..usize::MAX);
        assert_eq!(*overview, full, "all FFT blocks after lazy refresh, channel {ch}, {span:?}");
        // Source-aligned waveform columns contain every sharp feature and add at most
        // one PEAK_BLOCK of slack at either edge after arbitrary shifts.
        let start = (span.at as usize).saturating_sub(1_031).min(doc.frames() - 1);
        let end = (start + 8_003).min(doc.frames());
        let mut audio = vec![0.0; end - start];
        doc.state.read(ch, start as u64, &mut audio).unwrap();
        let (lo, hi) = doc.peaks[ch].minmax(start, end);
        assert!(audio.iter().all(|sample| *sample >= lo && *sample <= hi));
    }
}

#[test]
fn real_edits_undo_and_redo_equal_full_overviews() {
    let mut doc = Document::from_buffer(spiky(2_100_013)).unwrap();
    let mut history = History::new(doc.state.clone(), u64::MAX);
    let params = SpectrogramParams { fft_size: 512, ..SpectrogramParams::default() };
    let mut overviews: Vec<_> = (0..2).map(|ch| Overview::build_from(&StoreChannel::new(&doc.state, ch), &params)).collect();
    for step in 0..5 {
        let state = history.current();
        let len = state.len_frames();
        let edit = match step {
            0 => edit::delete(state, 37..1_000_040, ChannelMask::ALL, NONE).unwrap(),
            1 => {
                let clip = edit::copy(state, 97..8_102, ChannelMask::ALL).unwrap();
                edit::paste(state, PasteAt::Insert(len / 2 + 3), &clip, ChannelMask::ALL, NONE).unwrap()
            }
            2 => edit::replace_range(
                state,
                len - 19_007..len - 1_019,
                ChannelMask::only(1),
                "Gain",
                NONE,
                &AtomicBool::new(false),
                |_, input, output| {
                    for (source, dest) in input.iter().zip(output) {
                        for (s, d) in source.iter().zip(dest.iter_mut()) {
                            *d = *s * 0.37;
                        }
                    }
                    Ok(())
                },
            )
            .unwrap(),
            3 => edit::crop(state, 313..len - 727, ChannelMask::ALL, NONE).unwrap(),
            _ => edit::insert_silence(state, 41, 1, NONE).unwrap(),
        };
        history.push_edit(edit);
        verify_step(&mut doc, &mut overviews, history.current().as_ref().clone(), &params);
    }
    for _ in 0..5 {
        let state = history.undo().unwrap();
        verify_step(&mut doc, &mut overviews, state.as_ref().clone(), &params);
    }
    for _ in 0..5 {
        let state = history.redo().unwrap();
        verify_step(&mut doc, &mut overviews, state.as_ref().clone(), &params);
    }
}

struct CountingSamples<'a> {
    source: StoreChannel<'a>,
    frames: AtomicUsize,
}
impl Samples for CountingSamples<'_> {
    fn len(&self) -> usize {
        self.source.len()
    }
    fn read(&self, start: usize, out: &mut [f32]) {
        self.frames.fetch_add(out.len(), Ordering::Relaxed);
        self.source.read(start, out);
    }
}

#[test]
fn small_edits_read_a_bounded_amount_independent_of_document_length() {
    for frames in [2_100_013, 8_000_031] {
        let doc = Document::from_buffer(spiky(frames)).unwrap();
        let params = SpectrogramParams::default();
        let overview = Overview::build_from(&StoreChannel::new(&doc.state, 0), &params);
        for edit in [
            edit::silence(&doc.state, frames as u64 / 2 + 7..frames as u64 / 2 + 4_106, ChannelMask::ALL, NONE).unwrap(),
            edit::delete(&doc.state, 37..38, ChannelMask::ALL, NONE).unwrap(),
            edit::insert_silence(&doc.state, 37, 1, NONE).unwrap(),
        ] {
            let before = doc.state.store().cache_stats();
            let next = doc.after_step(edit.state).unwrap();
            let after = next.state.store().cache_stats();
            // Every store read loads at most BLOCK_FRAMES, even on a cache miss.
            assert!((after.hits + after.misses - before.hits - before.misses) <= 16, "waveform reread the suffix of {frames} frames");
            let span = EditSpan::between(&doc.state, &next.state);
            let mut updated = overview.clone();
            let samples = CountingSamples { source: StoreChannel::new(&next.state, 0), frames: AtomicUsize::new(0) };
            updated.after_step(span, samples.len());
            assert_eq!(samples.frames.load(Ordering::Relaxed), 0);
            let columns = Columns::covering(span.at as f64, span.at as f64 + 20_000.0, 4_000.0, &params);
            updated.refresh_visible(&samples, &params, columns);
            assert!(samples.frames.load(Ordering::Relaxed) <= 4 * 16 * params.fft_size, "spectral refresh read outside the view");
        }
        let cropped = edit::crop(&doc.state, 313..frames as u64 - 727, ChannelMask::ALL, NONE).unwrap().state;
        let before = doc.state.store().cache_stats();
        let next = doc.after_step(cropped).unwrap();
        let after = next.state.store().cache_stats();
        assert!(after.hits + after.misses - before.hits - before.misses <= 8, "crop reread its surviving middle");
        for ch in 0..2 {
            assert_eq!(next.peaks[ch], WavePeaks::build(&next.state, ch).unwrap());
        }
    }
}

#[test]
fn piece_diff_identical_insert_delete_and_one_channel() {
    let doc = Document::from_buffer(spiky(100_003)).unwrap();
    assert_eq!(EditSpan::between(&doc.state, &doc.state), EditSpan { at: 100_003, removed: 0, inserted: 0 });
    let inserted = edit::insert_silence(&doc.state, 37, 101, NONE).unwrap().state;
    assert_eq!(EditSpan::between(&doc.state, &inserted), EditSpan { at: 37, removed: 0, inserted: 101 });
    let deleted = edit::delete(&doc.state, 37..1_041, ChannelMask::ALL, NONE).unwrap().state;
    assert_eq!(EditSpan::between(&doc.state, &deleted), EditSpan { at: 37, removed: 1_004, inserted: 0 });
    let changed = edit::silence(&doc.state, 9_003..13_007, ChannelMask::only(1), NONE).unwrap().state;
    assert_eq!(EditSpan::between(&doc.state, &changed), EditSpan { at: 9_003, removed: 4_004, inserted: 4_004 });
    // Unrelated stores can reuse numeric block IDs; only shared handles prove identity.
    let other = Document::from_buffer(spiky(100_003)).unwrap();
    assert_eq!(EditSpan::between(&doc.state, &other.state), EditSpan { at: 0, removed: 100_003, inserted: 100_003 });
}

#[test]
fn aligned_fft_suffix_is_reused_and_pending_stale_blocks_survive_later_steps() {
    let doc = Document::from_buffer(spiky(310_007)).unwrap();
    let params = SpectrogramParams { fft_size: 512, ..SpectrogramParams::default() };
    let mut state = doc.state;
    let mut overview = Overview::build_from(&StoreChannel::new(&state, 0), &params);
    for length in [8 * params.fft_size as u64, 1, 8 * params.fft_size as u64] {
        let next = edit::insert_silence(&state, 50_003, length, NONE).unwrap().state;
        let span = EditSpan::between(&state, &next);
        overview.after_step(span, next.len_frames() as usize);
        let source = StoreChannel::new(&next, 0);
        let full = Overview::build_from(&source, &params);
        assert_eq!(overview.blocks().count(), full.blocks().count());
        for ((fresh, got), (_, want)) in overview.blocks().zip(full.blocks()) {
            if fresh {
                assert_eq!(got, want);
            }
        }
        if length > 1 && state.len_frames() == 310_007 {
            assert!(overview.blocks().last().unwrap().0, "an aligned shift reuses the last whole FFT block");
            assert!(overview.blocks().any(|(fresh, _)| !fresh), "the seam still needs new FFTs");
        } else {
            assert!(!overview.blocks().last().unwrap().0, "previously stale suffix stays stale across another edit");
        }
        // Render only a small view, leaving most stale blocks pending for the next edit.
        let columns = Columns::covering(45_000.0, 60_000.0, 2_000.0, &params);
        overview.refresh_visible(&source, &params, columns);
        assert_eq!(
            spectrogram::render_columns_from(&source, 48_000, columns, 32, &params, Some(&overview)),
            spectrogram::render_columns_from(&source, 48_000, columns, 32, &params, Some(&full))
        );
        state = next;
    }
}
