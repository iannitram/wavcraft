//! C171: the master output mode (mono / stereo / 5.1), core only. The mode enum and its channel
//! counts, the model-format mapping, parsing of the verb words, and the folds down to mono.

use wavcraft_core::multitrack::master_mode::{MODE_LIST, MasterMode, buffers_51_to_mono, downmix_51_to_mono, stereo_buffers_to_mono, stereo_to_mono};
use wavcraft_core::multitrack::soundcraft_model::ChannelFormat;
use wavcraft_core::multitrack::{MtError, downmix_51_to_stereo};

const K: f32 = std::f32::consts::FRAC_1_SQRT_2;

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-6
}

fn msg(e: Result<impl std::fmt::Debug, MtError>) -> String {
    match e {
        Ok(v) => format!("OK {v:?}"),
        Err(e) => e.0,
    }
}

const ALL: [MasterMode; 3] = [MasterMode::Mono, MasterMode::Stereo, MasterMode::Surround51];

#[test]
fn channel_counts_are_one_two_and_six() {
    assert_eq!(MasterMode::Mono.channels(), 1);
    assert_eq!(MasterMode::Stereo.channels(), 2);
    assert_eq!(MasterMode::Surround51.channels(), 6);
}

#[test]
fn each_mode_maps_to_the_models_format_and_back() {
    assert_eq!(MasterMode::Mono.format(), ChannelFormat::Mono);
    assert_eq!(MasterMode::Stereo.format(), ChannelFormat::Stereo);
    assert_eq!(MasterMode::Surround51.format(), ChannelFormat::Surround51);
    for m in ALL {
        assert_eq!(MasterMode::from_format(m.format()), Ok(m));
        assert_eq!(m.format().channels(), m.channels(), "the model agrees on the width");
    }
}

#[test]
fn formats_that_are_not_a_master_mode_are_refused() {
    for f in [
        ChannelFormat::Lcr,
        ChannelFormat::Quad,
        ChannelFormat::Lcrs,
        ChannelFormat::Surround50,
        ChannelFormat::Surround71,
        ChannelFormat::Ambisonics(1),
    ] {
        let m = msg(MasterMode::from_format(f));
        assert!(m.contains(MODE_LIST), "{f:?}: {m}");
    }
    assert_eq!(MODE_LIST, "mono, stereo or 5.1");
}

#[test]
fn names_are_the_verb_words() {
    assert_eq!(MasterMode::Mono.name(), "mono");
    assert_eq!(MasterMode::Stereo.name(), "stereo");
    assert_eq!(MasterMode::Surround51.name(), "5.1");
    for m in ALL {
        assert_eq!(MasterMode::parse(m.name()), Ok(m), "a name parses back");
    }
}

#[test]
fn parse_takes_the_three_words_in_any_case() {
    assert_eq!(MasterMode::parse("mono"), Ok(MasterMode::Mono));
    assert_eq!(MasterMode::parse("stereo"), Ok(MasterMode::Stereo));
    assert_eq!(MasterMode::parse("5.1"), Ok(MasterMode::Surround51));
    assert_eq!(MasterMode::parse("MONO"), Ok(MasterMode::Mono));
    assert_eq!(MasterMode::parse("Stereo"), Ok(MasterMode::Stereo));
    assert_eq!(MasterMode::parse("sTeReO"), Ok(MasterMode::Stereo));
    assert_eq!(MasterMode::parse("mOnO"), Ok(MasterMode::Mono));
}

#[test]
fn parse_refuses_everything_else_with_the_list_and_the_input() {
    for bad in ["", " ", "5", "51", "5.1.", "5,1", "5.0", "7.1", "surround", "quad", " mono", "mono ", "stereo\n", "mono,stereo", "monoo", "stere"] {
        let m = msg(MasterMode::parse(bad));
        assert!(m.contains(MODE_LIST), "{bad:?}: {m}");
        assert!(m.contains(&format!("{bad:?}")), "the input is quoted: {m}");
    }
}

#[test]
fn stereo_to_mono_is_the_mean() {
    assert_eq!(stereo_to_mono(1.0, 0.0), 0.5);
    assert_eq!(stereo_to_mono(0.0, 1.0), 0.5);
    assert_eq!(stereo_to_mono(1.0, 1.0), 1.0, "full scale stays full scale");
    assert_eq!(stereo_to_mono(-1.0, -1.0), -1.0);
    assert_eq!(stereo_to_mono(0.5, -0.5), 0.0, "opposite phase cancels");
    assert_eq!(stereo_to_mono(0.25, 0.75), 0.5);
    assert_eq!(stereo_to_mono(-1.0, 0.5), -0.25);
    assert!(stereo_to_mono(f32::NAN, 1.0).is_nan(), "samples are not checked");
}

#[test]
fn stereo_buffers_fold_frame_by_frame() {
    let out = stereo_buffers_to_mono(&[vec![1.0, 0.5, -1.0, 0.0], vec![0.0, 0.5, 1.0, 0.25]]);
    assert_eq!(out, Ok(vec![0.5, 0.5, 0.0, 0.125]));
}

#[test]
fn stereo_buffers_with_no_frames_give_no_frames() {
    assert_eq!(stereo_buffers_to_mono(&[vec![], vec![]]), Ok(Vec::new()));
}

#[test]
fn stereo_buffers_need_exactly_two_channels() {
    for n in [0usize, 1, 3, 6] {
        let chans: Vec<Vec<f32>> = (0..n).map(|_| vec![0.0; 4]).collect();
        let m = msg(stereo_buffers_to_mono(&chans));
        assert!(m.contains("needs 2 channels"), "{n}: {m}");
        assert!(m.contains(&format!("got {n}")), "{n}: {m}");
    }
    assert!(stereo_buffers_to_mono(&[vec![0.0; 4], vec![0.0; 4]]).is_ok());
}

#[test]
fn stereo_buffers_of_different_lengths_are_refused() {
    for (a, b) in [(2usize, 1usize), (1, 2), (0, 1), (1, 0)] {
        let m = msg(stereo_buffers_to_mono(&[vec![0.5; a], vec![0.5; b]]));
        assert!(m.contains("differ in length"), "{a}/{b}: {m}");
    }
}

#[test]
fn the_51_fold_to_mono_is_the_stereo_downmix_then_the_mean() {
    let frames: [[f32; 6]; 5] = [
        [1.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        [0.3, -0.2, 0.9, 0.5, -0.4, 0.1],
        [0.0, 0.0, 1.0, 0.0, 0.0, 0.0],
        [1.0; 6],
        [-0.5, 0.25, -1.0, 1.0, 0.75, -0.125],
    ];
    for f in frames {
        let d = downmix_51_to_stereo(f);
        assert!(close(downmix_51_to_mono(f), (d[0] + d[1]) / 2.0), "{f:?}");
    }
}

#[test]
fn the_51_fold_to_mono_has_the_expected_levels() {
    assert!(close(downmix_51_to_mono([1.0, 0.0, 0.0, 0.0, 0.0, 0.0]), 0.5), "L");
    assert!(close(downmix_51_to_mono([0.0, 1.0, 0.0, 0.0, 0.0, 0.0]), 0.5), "R");
    assert!(close(downmix_51_to_mono([0.0, 0.0, 1.0, 0.0, 0.0, 0.0]), K), "centre at -3 dB");
    assert!(close(downmix_51_to_mono([0.0, 0.0, 0.0, 1.0, 0.0, 0.0]), 0.0), "LFE dropped");
    assert!(close(downmix_51_to_mono([0.0, 0.0, 0.0, 0.0, 1.0, 0.0]), K / 2.0), "Ls");
    assert!(close(downmix_51_to_mono([0.0, 0.0, 0.0, 0.0, 0.0, 1.0]), K / 2.0), "Rs");
    assert!(close(downmix_51_to_mono([1.0; 6]), 1.0 + 2.0 * K), "no normalisation: 2.4142135");
}

#[test]
fn the_51_buffers_fold_frame_by_frame() {
    // frame 0 is only L = 1; frame 1 is only C = 1; frame 2 is only LFE = 1.
    let chans = vec![vec![1.0, 0.0, 0.0], vec![0.0; 3], vec![0.0, 1.0, 0.0], vec![0.0, 0.0, 1.0], vec![0.0; 3], vec![0.0; 3]];
    let out = buffers_51_to_mono(&chans).unwrap();
    assert_eq!(out.len(), 3);
    assert!(close(out[0], 0.5) && close(out[1], K) && close(out[2], 0.0), "{out:?}");
}

#[test]
fn the_51_buffers_match_the_frame_function() {
    let chans: Vec<Vec<f32>> = (0..6).map(|c| (0..5).map(|i| ((c * 5 + i) as f32 * 0.37).sin()).collect()).collect();
    let out = buffers_51_to_mono(&chans).unwrap();
    assert_eq!(out.len(), 5);
    for i in 0..5 {
        let f = [chans[0][i], chans[1][i], chans[2][i], chans[3][i], chans[4][i], chans[5][i]];
        assert!(close(out[i], downmix_51_to_mono(f)), "frame {i}");
    }
}

#[test]
fn the_51_buffers_with_no_frames_give_no_frames() {
    let chans: Vec<Vec<f32>> = vec![Vec::new(); 6];
    assert_eq!(buffers_51_to_mono(&chans), Ok(Vec::new()));
}

#[test]
fn the_51_buffers_need_exactly_six_channels() {
    for n in [0usize, 1, 2, 5, 7] {
        let chans: Vec<Vec<f32>> = (0..n).map(|_| vec![0.0; 4]).collect();
        let m = msg(buffers_51_to_mono(&chans));
        assert!(m.contains("needs 6 channels"), "{n}: {m}");
        assert!(m.contains(&format!("got {n}")), "{n}: {m}");
    }
    let six: Vec<Vec<f32>> = (0..6).map(|_| vec![0.0; 4]).collect();
    assert!(buffers_51_to_mono(&six).is_ok());
}

#[test]
fn the_51_buffers_of_different_lengths_are_refused() {
    for odd in 0..6usize {
        for len in [3usize, 5] {
            let mut chans: Vec<Vec<f32>> = (0..6).map(|_| vec![0.1; 4]).collect();
            chans[odd] = vec![0.1; len];
            let m = msg(buffers_51_to_mono(&chans));
            assert!(m.contains("differ in length"), "channel {odd} len {len}: {m}");
        }
    }
}
