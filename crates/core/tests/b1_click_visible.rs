use wavcraft_core::AudioBuffer;
use wavcraft_core::document::Document;

#[test]
fn a_one_sample_click_shows_at_full_zoom_out() {
    let mut ch = vec![0.0f32; 4_800_000];
    ch[2_345_678] = 1.0;
    let doc = Document::from_buffer(AudioBuffer { sample_rate: 48_000, channels: vec![ch] }).unwrap();
    for (cols, hit) in [(1000usize, 488usize), (100, 48)] {
        let out = doc.waveform_columns(0, 0.0, 4_800_000.0, cols);
        assert_eq!(out.len(), cols);
        for (i, c) in out.iter().enumerate() {
            let want = if i == hit { (0.0, 1.0) } else { (0.0, 0.0) };
            assert_eq!(*c, want, "cols {cols} column {i}");
        }
    }
}
