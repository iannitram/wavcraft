//! Plate (b086): measured behaviour of the plate reverb through the effect seam and the stateful Run.

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use wavcraft_core::doc::edit::{ChannelMask, Crossfades};
use wavcraft_core::doc::{Channel, DocState, Piece, SourceFormat};
use wavcraft_core::effects;
use wavcraft_core::store::{BLOCK_FRAMES, Store, StoreOptions};

const RATE: u32 = 48_000;

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("wavcraft-plate-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn doc(dir: &std::path::Path, data: &[Vec<f32>]) -> DocState {
    let s = Store::create_with(dir, StoreOptions { cache_bytes: 64 << 20, ..StoreOptions::default() }).unwrap();
    let chans = data.iter().map(|c| Channel::from_pieces(c.chunks(BLOCK_FRAMES).map(|b| Piece::whole(s.append(b).unwrap()).unwrap()))).collect();
    DocState::new(s, chans, RATE, SourceFormat::synthetic(RATE, data.len() as u16)).unwrap()
}

fn run(effect: &str, args: &str, d: &DocState, range: std::ops::Range<u64>) -> DocState {
    let e = effects::find(effect).unwrap();
    let p = effects::parse_params(e, args).unwrap();
    let cancel = AtomicBool::new(false);
    effects::apply(e, &p, d, range, ChannelMask::ALL, Crossfades::NONE, &cancel).unwrap().state
}

fn read(d: &DocState, ch: usize) -> Vec<f32> {
    let mut v = vec![0.0; d.len_frames() as usize];
    assert_eq!(d.read(ch, 0, &mut v).unwrap(), v.len());
    v
}

fn energy(v: &[f32], from: f64, to: f64) -> f64 {
    v[(from * RATE as f64) as usize..(to * RATE as f64) as usize].iter().map(|x| (*x as f64).powi(2)).sum::<f64>()
}

fn noise(n: usize, seed: u32) -> Vec<f32> {
    let mut s = seed;
    (0..n)
        .map(|_| {
            s = s.wrapping_mul(1664525).wrapping_add(1013904223);
            (s >> 8) as f32 / (1u32 << 23) as f32 - 1.0
        })
        .collect()
}

/// A short noise burst followed by silence, `secs` long in all.
fn burst(secs: usize) -> Vec<f32> {
    let mut x = noise(RATE as usize / 10, 7);
    x.resize(RATE as usize * secs, 0.0);
    x
}

fn max_diff(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| (x - y).abs()).fold(0.0f32, f32::max)
}

#[test]
fn plate_is_registered_with_its_six_settings() {
    let e = effects::find("plate").unwrap();
    assert_eq!(e.name(), "Plate");
    let ids: Vec<_> = e.parameters().iter().map(|p| p.id).collect();
    assert_eq!(ids, ["size", "decay", "damping", "pre_delay", "diffusion", "wet"]);
    let defaults = effects::Params::defaults(e);
    assert_eq!(defaults, [50.0, 2.5, 10000.0, 20.0, 85.0, 30.0]);
    effects::validate(e, &defaults).unwrap();
    assert!(effects::all().iter().any(|x| x.id() == "plate"));
    assert!(effects::all().iter().any(|x| x.id() == "reverb"), "the room reverb is still there");
}

#[test]
fn settings_outside_their_range_are_refused_with_the_range() {
    let e = effects::find("plate").unwrap();
    let ok = effects::Params::defaults(e);
    let with = |i: usize, v: f64| {
        let mut p = ok.clone();
        p[i] = v;
        effects::validate(e, &p)
    };
    let cases: [(usize, f64, f64, &str, &str); 5] = [
        (0, -1.0, 101.0, "size", "0 to 100"),
        (1, 0.05, 25.0, "decay", "0.1 to 20 s"),
        (2, 500.0, 25000.0, "damping", "1000 to 20000 Hz"),
        (3, -1.0, 300.0, "pre_delay", "0 to 250 ms"),
        (4, -1.0, 101.0, "diffusion", "0 to 100"),
    ];
    for (i, low, high, id, range) in cases {
        for v in [low, high] {
            let msg = with(i, v).unwrap_err();
            assert!(msg.contains(id) && msg.contains(range), "{id} = {v}: {msg}");
        }
    }
    let msg = with(5, 101.0).unwrap_err();
    assert!(msg.contains("wet") && msg.contains("0 to 100"), "{msg}");
    assert!(with(5, f64::NAN).is_err());
    assert!(with(1, f64::INFINITY).is_err());
    let msg = effects::validate(e, &ok[..5]).unwrap_err();
    assert!(msg.contains("takes 6 setting"), "{msg}");
    // Text settings go through the same ranges and names.
    assert!(effects::parse_params(e, "decay=25").is_err());
    assert!(effects::parse_params(e, "room_size=50").unwrap_err().contains("no setting"));
    assert_eq!(effects::parse_params(e, "decay=4s pre_delay=0ms wet=100%").unwrap(), [50.0, 4.0, 10000.0, 0.0, 85.0, 100.0]);
}

#[test]
fn wet_zero_returns_the_input_and_wet_full_changes_it() {
    let tmp = TempDir::new("mix");
    let n = RATE as usize;
    let x = noise(n, 3);
    let d = doc(&tmp.0, &[x.clone(), noise(n, 4)]);
    let dry = read(&run("plate", "wet=0%", &d, 0..0), 0);
    assert!(max_diff(&dry, &x) < 1e-6, "wet 0 must pass the input through: {}", max_diff(&dry, &x));
    let wet = read(&run("plate", "wet=100%", &d, 0..0), 0);
    assert!(max_diff(&wet, &x) > 0.05, "wet 100 must differ from the input: {}", max_diff(&wet, &x));
    let half = read(&run("plate", "wet=50%", &d, 0..0), 0);
    assert!(max_diff(&half, &x) > 0.01 && max_diff(&half, &x) < max_diff(&wet, &x) + 0.5, "half wet sits between");
    assert!(max_diff(&half, &wet) > 0.01, "half wet differs from full wet");
}

#[test]
fn tail_decays_over_the_set_time_and_longer_decay_rings_longer() {
    let tmp = TempDir::new("tail");
    let d = doc(&tmp.0, &[burst(6)]);
    let short = read(&run("plate", "decay=1s wet=100%", &d, 0..0), 0);
    let long = read(&run("plate", "decay=4s wet=100%", &d, 0..0), 0);
    let early = energy(&short, 0.3, 0.4);
    assert!(early > 1e-3, "there is a tail right after the burst: {early}");
    // RT60 = 1 s means -60 dB one second later; require at least 30 dB of the 60.
    let drop_db = 10.0 * (early / energy(&short, 1.3, 1.4).max(1e-30)).log10();
    assert!(drop_db > 30.0, "1 s decay fell only {drop_db} dB in one second");
    assert!(energy(&long, 2.3, 2.4) > 50.0 * energy(&short, 2.3, 2.4), "a 4 s decay still rings when a 1 s one is gone");
    // The long tail decays too: later is quieter than earlier.
    assert!(energy(&long, 1.0, 1.1) > energy(&long, 3.0, 3.1), "the 4 s tail is falling");
}

#[test]
fn the_tail_past_the_selection_end_is_cut_and_the_length_kept() {
    let tmp = TempDir::new("cut");
    let d = doc(&tmp.0, &[burst(3)]);
    let whole = read(&run("plate", "decay=4s wet=100%", &d, 0..0), 0);
    let part = read(&run("plate", "decay=4s wet=100%", &d, 0..(RATE as u64 / 2)), 0);
    assert_eq!(part.len(), whole.len(), "length kept");
    assert!(part[RATE as usize / 2..].iter().all(|v| *v == 0.0), "nothing written past the selection");
    assert!(part[..RATE as usize / 2].iter().any(|v| v.abs() > 1e-3), "something was written inside it");
}

#[test]
fn a_fixed_impulse_gives_the_same_output_every_time() {
    let tmp = TempDir::new("det");
    let mut x = vec![0.0f32; RATE as usize];
    x[100] = 1.0;
    let d = doc(&tmp.0, &[x.clone(), x]);
    let a = read(&run("plate", "wet=100%", &d, 0..0), 0);
    let b = read(&run("plate", "wet=100%", &d, 0..0), 0);
    assert_eq!(a, b, "same input and settings, same samples");
    // The impulse leaves a tail, and the first sound is not before the shortest delay line.
    assert!(energy(&a, 0.1, 0.5) > 1e-6, "the impulse rings");
    let first = a.iter().position(|v| v.abs() > 1e-6).unwrap();
    assert!(first > 100, "the output starts after the impulse (pre-delay and lines): {first}");
}

#[test]
fn every_setting_reaches_the_plugin_and_changes_the_sound() {
    let tmp = TempDir::new("each");
    let d = doc(&tmp.0, &[burst(3), burst(3)]);
    let base = read(&run("plate", "wet=100%", &d, 0..0), 0);
    for args in ["size=100", "decay=8s", "damping=1500", "pre_delay=120ms", "diffusion=10"] {
        let out = read(&run("plate", &format!("wet=100% {args}"), &d, 0..0), 0);
        assert!(max_diff(&out, &base) > 1e-3, "{args} changed nothing");
    }
}

#[test]
fn pre_delay_holds_the_reverb_back() {
    let tmp = TempDir::new("pre");
    let mut x = vec![0.0f32; RATE as usize];
    x[0] = 1.0;
    let d = doc(&tmp.0, &[x]);
    let late = read(&run("plate", "pre_delay=100ms wet=100%", &d, 0..0), 0);
    let early = read(&run("plate", "pre_delay=0ms wet=100%", &d, 0..0), 0);
    assert!(energy(&late, 0.0, 0.09) < 1e-9, "nothing before the pre-delay: {}", energy(&late, 0.0, 0.09));
    assert!(energy(&early, 0.0, 0.09) > 1e-6, "with no pre-delay the reverb starts at once");
    assert!(energy(&late, 0.1, 0.5) > 1e-6, "and arrives after it");
}

#[test]
fn plate_matches_one_pass_through_plate_reverb_in_chunks_and_is_not_the_room() {
    let tmp = TempDir::new("chunk");
    let n = BLOCK_FRAMES * 2 + 5000;
    let x = noise(n, 99);
    let d = doc(&tmp.0, &[x.clone(), noise(n, 5)]);
    let out = read(&run("plate", "size=70 decay=3s damping=8000 pre_delay=30ms diffusion=60% wet=60%", &d, 0..0), 0);
    let mut p = soundcraft_dsp::create("plate_reverb").unwrap();
    p.prepare(RATE as f32, 4096, 2);
    for (k, v) in [("size", 70.0), ("decay", 3.0), ("damping", 8000.0), ("pre_delay", 30.0), ("diffusion", 60.0), ("mix", 60.0)] {
        assert!(p.set_param(k, v));
    }
    p.reset();
    let mut io = vec![x.clone(), noise(n, 5)];
    p.process(&mut io, n);
    let worst = max_diff(&out, &io[0]);
    assert!(worst < 1e-5, "chunked run differs from one pass by {worst}: state lost at a chunk join or a setting mis-mapped");
    // The room reverb with the same numbers sounds different.
    let mut room = soundcraft_dsp::create("room_reverb").unwrap();
    room.prepare(RATE as f32, 4096, 2);
    for (k, v) in [("size", 70.0), ("decay", 3.0), ("damping", 8000.0), ("pre_delay", 30.0), ("diffusion", 60.0), ("mix", 60.0)] {
        assert!(room.set_param(k, v));
    }
    room.reset();
    let mut rio = vec![x, noise(n, 5)];
    room.process(&mut rio, n);
    assert!(max_diff(&out, &rio[0]) > 0.01, "plate is not the room");
}

#[test]
fn the_output_stays_finite_at_the_extremes() {
    let tmp = TempDir::new("finite");
    let loud: Vec<f32> = noise(RATE as usize, 11).iter().map(|v| v * 4.0).collect();
    let d = doc(&tmp.0, &[loud.clone(), loud]);
    for args in [
        "size=100 decay=20s diffusion=100% wet=100% damping=20000 pre_delay=250ms",
        "size=0 decay=0.1s diffusion=0% wet=100% damping=1000 pre_delay=0ms",
    ] {
        let out = run("plate", args, &d, 0..0);
        for ch in 0..2 {
            assert!(read(&out, ch).iter().all(|v| v.is_finite()), "{args}: channel {ch} has a non-finite sample");
        }
    }
}
