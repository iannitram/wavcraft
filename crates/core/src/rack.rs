//! The Effects Rack (f31): an ordered list of effect slots, each with a power switch, plus input
//! gain, output gain and a dry/wet Mix for the whole rack.
//!
//! [`RackConfig`] is plain data (what the panel edits, what a rack preset stores). A
//! [`RackProcessor`] is built from it with all its memory allocated, then run block by block with
//! no allocation and no locks, so the player can use it in the audio callback; [`apply`] runs the
//! same processor over a range of a document as one undoable edit named "Effects Rack".

use std::io;
use std::ops::Range;
use std::sync::atomic::AtomicBool;

use crate::doc::DocState;
use crate::doc::edit::{self, ChannelMask, Crossfades, Edit, EditError};
use crate::effects::{self, EffectStream};

/// Slots a new rack starts with.
pub const DEFAULT_SLOTS: usize = 10;
/// The most channels a rack processes (more are passed through).
pub const MAX_CHANNELS: usize = 8;
/// Frames the processor handles at a time (its buffers are this long).
pub const MAX_BLOCK: usize = 1024;
/// Range of the input and output gain, dB.
pub const GAIN_DB: (f64, f64) = (-60.0, 24.0);
/// The undo step's name for an applied rack.
pub const LABEL: &str = "Effects Rack";

/// One slot: an effect (by [`effects::Effect::id`]) with its settings, and its power switch.
#[derive(Debug, Clone, PartialEq)]
pub struct SlotConfig {
    /// `None` for an empty slot.
    pub effect: Option<String>,
    pub params: Vec<f64>,
    /// Power: off means bypassed (costs nothing).
    pub enabled: bool,
}

impl SlotConfig {
    pub fn empty() -> SlotConfig {
        SlotConfig { effect: None, params: Vec::new(), enabled: false }
    }
}

/// The whole rack's settings.
#[derive(Debug, Clone, PartialEq)]
pub struct RackConfig {
    /// Master power: off bypasses the whole rack.
    pub power: bool,
    pub input_db: f64,
    pub output_db: f64,
    /// Share of the processed signal in the output, 0 to 100 (100 is fully wet).
    pub mix_pct: f64,
    pub slots: Vec<SlotConfig>,
}

impl Default for RackConfig {
    fn default() -> RackConfig {
        RackConfig::new()
    }
}

impl RackConfig {
    /// A rack of [`DEFAULT_SLOTS`] empty slots, power on, unity gains, fully wet.
    pub fn new() -> RackConfig {
        RackConfig { power: true, input_db: 0.0, output_db: 0.0, mix_pct: 100.0, slots: vec![SlotConfig::empty(); DEFAULT_SLOTS] }
    }

    /// Put `effect` (an id) with `params` (`None`: its defaults) in `slot`, powered on.
    pub fn set_slot(&mut self, slot: usize, effect: &str, params: Option<Vec<f64>>) -> Result<(), String> {
        let n = self.slots.len();
        let s = self.slots.get_mut(slot).ok_or_else(|| format!("slot {} does not exist (the rack has {n})", slot + 1))?;
        let Some(e) = effects::find(effect) else { return Err(format!("no effect named {effect:?}")) };
        let params = params.unwrap_or_else(|| effects::Params::defaults(e));
        effects::validate(e, &params)?;
        *s = SlotConfig { effect: Some(e.id().to_string()), params, enabled: true };
        Ok(())
    }

    /// Empty `slot`.
    pub fn clear_slot(&mut self, slot: usize) -> Result<(), String> {
        let n = self.slots.len();
        *self.slots.get_mut(slot).ok_or_else(|| format!("slot {} does not exist (the rack has {n})", slot + 1))? = SlotConfig::empty();
        Ok(())
    }

    /// Switch `slot` on or off.
    pub fn set_slot_power(&mut self, slot: usize, on: bool) -> Result<(), String> {
        let n = self.slots.len();
        self.slots.get_mut(slot).ok_or_else(|| format!("slot {} does not exist (the rack has {n})", slot + 1))?.enabled = on;
        Ok(())
    }

    pub fn set_input_db(&mut self, db: f64) -> Result<(), String> {
        check_gain("input gain", db)?;
        self.input_db = db;
        Ok(())
    }

    pub fn set_output_db(&mut self, db: f64) -> Result<(), String> {
        check_gain("output gain", db)?;
        self.output_db = db;
        Ok(())
    }

    pub fn set_mix(&mut self, pct: f64) -> Result<(), String> {
        if !pct.is_finite() || !(0.0..=100.0).contains(&pct) {
            return Err(format!("Mix {pct} is not 0 to 100 %"));
        }
        self.mix_pct = pct;
        Ok(())
    }

    /// Reset to bypass, as after Apply: master power and every slot's power off. The slots keep
    /// their effects and settings.
    pub fn bypass(&mut self) {
        self.power = false;
        for s in &mut self.slots {
            s.enabled = false;
        }
    }

    /// The powered slots that hold an effect.
    pub fn active_slots(&self) -> impl Iterator<Item = &SlotConfig> {
        self.slots.iter().filter(|s| s.enabled && s.effect.is_some())
    }

    /// Whether running this rack would change the signal at all.
    pub fn is_active(&self) -> bool {
        self.power && (self.active_slots().next().is_some() || self.input_db != 0.0 || self.output_db != 0.0)
    }

    /// One line for the control channel.
    pub fn describe(&self) -> String {
        let slots: Vec<String> = self
            .slots
            .iter()
            .enumerate()
            .map(|(i, s)| match &s.effect {
                None => format!("{}:-", i + 1),
                Some(id) => {
                    let named =
                        effects::find(id).map(|e| e.parameters().iter().zip(&s.params).map(|(p, v)| format!(" {}={v}", p.id)).collect::<String>());
                    format!("{}:{id}:{}{}", i + 1, if s.enabled { "on" } else { "off" }, named.unwrap_or_default())
                }
            })
            .collect();
        format!(
            "power={} in={} out={} mix={} slots={}",
            if self.power { "on" } else { "off" },
            self.input_db,
            self.output_db,
            self.mix_pct,
            slots.join("; ")
        )
    }
}

fn check_gain(what: &str, db: f64) -> Result<(), String> {
    if db.is_finite() && db >= GAIN_DB.0 && db <= GAIN_DB.1 { Ok(()) } else { Err(format!("{what} {db} is not {} to {} dB", GAIN_DB.0, GAIN_DB.1)) }
}

fn gain(db: f64) -> f32 {
    10f64.powf(db / 20.0) as f32
}

/// A rack ready to run: its effect streams, gains and buffers, all allocated. Not `Clone`; build
/// another from the same [`RackConfig`] when the settings change.
pub struct RackProcessor {
    channels: usize,
    streams: Vec<Box<dyn EffectStream>>,
    input_gain: f32,
    output_gain: f32,
    mix: f32,
    /// The signal as it came in (for Mix).
    dry: Vec<Vec<f32>>,
    a: Vec<Vec<f32>>,
    b: Vec<Vec<f32>>,
}

impl RackProcessor {
    /// Build for `channels` channels (at most [`MAX_CHANNELS`]) at `rate` Hz. Slots that are off
    /// or empty are left out; a rack with master power off has no effects and unity gains.
    pub fn new(config: &RackConfig, channels: usize, rate: u32) -> Result<RackProcessor, String> {
        if channels == 0 || channels > MAX_CHANNELS {
            return Err(format!("the rack handles 1 to {MAX_CHANNELS} channels, not {channels}"));
        }
        let mut streams = Vec::new();
        if config.power {
            for s in config.active_slots() {
                let Some(e) = s.effect.as_deref().and_then(effects::find) else { continue };
                streams.push(effects::open_stream(e, &s.params, channels, rate)?);
            }
        }
        let (input_db, output_db, mix) = if config.power { (config.input_db, config.output_db, config.mix_pct) } else { (0.0, 0.0, 100.0) };
        let buf = || vec![vec![0.0f32; MAX_BLOCK]; channels];
        Ok(RackProcessor {
            channels,
            streams,
            input_gain: gain(input_db),
            output_gain: gain(output_db),
            mix: (mix / 100.0) as f32,
            dry: buf(),
            a: buf(),
            b: buf(),
        })
    }

    pub fn channels(&self) -> usize {
        self.channels
    }

    /// The input buffer of channel `ch` for [`run_chunk`](Self::run_chunk): fill its first `n`
    /// frames (`n` up to [`MAX_BLOCK`]).
    pub fn chunk_in(&mut self, ch: usize) -> &mut [f32] {
        &mut self.dry[ch]
    }

    /// The processed first `n` frames of channel `ch`, after [`run_chunk`](Self::run_chunk).
    pub fn chunk_out(&self, ch: usize) -> &[f32] {
        &self.a[ch]
    }

    /// Process the first `n` frames filled with [`chunk_in`](Self::chunk_in). No allocation.
    pub fn run_chunk(&mut self, n: usize) {
        let n = n.min(MAX_BLOCK);
        for (a, dry) in self.a.iter_mut().zip(&self.dry) {
            for (x, d) in a[..n].iter_mut().zip(&dry[..n]) {
                *x = *d * self.input_gain;
            }
        }
        for stream in &mut self.streams {
            {
                let mut ins: [&[f32]; MAX_CHANNELS] = [&[]; MAX_CHANNELS];
                let mut outs: [&mut [f32]; MAX_CHANNELS] = Default::default();
                for (slot, a) in ins.iter_mut().zip(&self.a) {
                    *slot = &a[..n];
                }
                for (slot, b) in outs.iter_mut().zip(self.b.iter_mut()) {
                    *slot = &mut b[..n];
                }
                stream.process_block(&ins[..self.channels], &mut outs[..self.channels]);
            }
            std::mem::swap(&mut self.a, &mut self.b);
        }
        for (a, dry) in self.a.iter_mut().zip(&self.dry) {
            for (x, d) in a[..n].iter_mut().zip(&dry[..n]) {
                *x = (*x * self.output_gain) * self.mix + *d * (1.0 - self.mix);
            }
        }
    }

    /// Process `io` in place: one slice per channel (the first [`channels`](Self::channels) are
    /// used), all the same length, any length.
    pub fn process_block(&mut self, io: &mut [&mut [f32]]) {
        let len = io.first().map_or(0, |c| c.len());
        let mut at = 0;
        while at < len {
            let n = (len - at).min(MAX_BLOCK);
            for (ch, buf) in io.iter().enumerate().take(self.channels) {
                self.dry[ch][..n].copy_from_slice(&buf[at..at + n]);
            }
            self.run_chunk(n);
            for (ch, buf) in io.iter_mut().enumerate().take(self.channels) {
                buf[at..at + n].copy_from_slice(&self.a[ch][..n]);
            }
            at += n;
        }
    }
}

/// Run `config` over `range` of `doc` (the whole file when empty) on `channels`, as one edit
/// named [`LABEL`]. Streams through `replace_range`; the effect streams keep their state from
/// chunk to chunk.
pub fn apply(
    config: &RackConfig,
    doc: &DocState,
    range: Range<u64>,
    channels: ChannelMask,
    xf: Crossfades,
    cancel: &AtomicBool,
) -> Result<Edit, EditError> {
    let invalid = |why: String| EditError::Io(io::Error::new(io::ErrorKind::InvalidInput, why));
    let k = channels.active(doc.channel_count()).len();
    let mut rack = RackProcessor::new(config, k.max(1), doc.sample_rate()).map_err(invalid)?;
    let range = if range.start < range.end { range } else { 0..doc.len_frames() };
    edit::replace_range(doc, range, channels, LABEL, xf, cancel, |_at, input, output| {
        for (i, o) in input.iter().zip(output.iter_mut()) {
            o.copy_from_slice(i);
        }
        rack.process_block(output);
        Ok(())
    })
}

/// Like [`apply`], but lets the rack ring out (B019, "Apply with tail"): after `range` (the whole
/// file when empty) it feeds `tail_frames` frames of silence through the same rack and keeps what
/// comes out, so a delay or reverb tail is not cut off at the selection end. The document grows
/// by `tail_frames` on every channel (channels not in `channels` get silence there); everything
/// after the range moves later. One edit named [`LABEL`] whose change is `LengthFrom` the range
/// start. With `tail_frames == 0`, or no active channel, it is exactly [`apply`].
pub fn apply_with_tail(
    config: &RackConfig,
    doc: &DocState,
    range: Range<u64>,
    channels: ChannelMask,
    xf: Crossfades,
    cancel: &AtomicBool,
    tail_frames: u64,
) -> Result<Edit, EditError> {
    let invalid = |why: String| EditError::Io(io::Error::new(io::ErrorKind::InvalidInput, why));
    let k = channels.active(doc.channel_count()).len();
    let mut rack = RackProcessor::new(config, k.max(1), doc.sample_rate()).map_err(invalid)?;
    let range = if range.start < range.end { range } else { 0..doc.len_frames() };

    if tail_frames == 0 || k == 0 {
        return apply(config, doc, range, channels, xf, cancel);
    }

    let widened = edit::insert_silence(doc, range.end, tail_frames, Crossfades::NONE)?;
    let mut done = edit::replace_range(&widened.state, range.start..range.end + tail_frames, channels, LABEL, xf, cancel, |_at, input, output| {
        for (i, o) in input.iter().zip(output.iter_mut()) {
            o.copy_from_slice(i);
        }
        rack.process_block(output);
        Ok(())
    })?;

    done.change = edit::Change::LengthFrom { frame: range.start };
    Ok(done)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rack_with(slots: &[(&str, &str)]) -> RackConfig {
        let mut c = RackConfig::new();
        for (i, (id, args)) in slots.iter().enumerate() {
            let e = effects::find(id).unwrap();
            c.set_slot(i, id, Some(effects::parse_params(e, args).unwrap())).unwrap();
        }
        c
    }

    fn run(config: &RackConfig, input: &[f32]) -> Vec<f32> {
        let mut p = RackProcessor::new(config, 1, 48_000).unwrap();
        let mut buf = input.to_vec();
        p.process_block(&mut [&mut buf[..]]);
        buf
    }

    #[test]
    fn an_empty_rack_passes_audio_through() {
        let x: Vec<f32> = (0..3000).map(|i| (i as f32 * 0.01).sin()).collect();
        assert_eq!(run(&RackConfig::new(), &x), x);
    }

    #[test]
    fn slots_run_in_order_and_bypass_costs_nothing() {
        let x = vec![0.5f32; 2500];
        let c = rack_with(&[("invert", ""), ("gain", "gain_db=-6.0206")]);
        let y = run(&c, &x);
        assert!(y.iter().all(|v| (v + 0.25).abs() < 1e-4), "{:?}", &y[..3]);
        let mut off = c.clone();
        off.set_slot_power(0, false).unwrap();
        let y = run(&off, &x);
        assert!(y.iter().all(|v| (v - 0.25).abs() < 1e-4));
        off.set_slot_power(1, false).unwrap();
        assert_eq!(run(&off, &x), x);
        assert!(!off.is_active());
        let mut master = c;
        master.power = false;
        assert_eq!(run(&master, &x), x, "master power off bypasses everything");
    }

    #[test]
    fn gains_and_mix() {
        let x = vec![0.5f32; 100];
        let mut c = rack_with(&[("invert", "")]);
        c.set_input_db(-6.0206).unwrap();
        c.set_output_db(6.0206).unwrap();
        assert!(run(&c, &x).iter().all(|v| (v + 0.5).abs() < 1e-3));
        c.set_input_db(0.0).unwrap();
        c.set_output_db(0.0).unwrap();
        c.set_mix(50.0).unwrap();
        assert!(run(&c, &x).iter().all(|v| v.abs() < 1e-6), "half of +0.5 and half of -0.5");
        c.set_mix(0.0).unwrap();
        assert_eq!(run(&c, &x), x);
        assert!(c.set_mix(101.0).is_err() && c.set_input_db(99.0).is_err() && c.set_slot(99, "gain", None).is_err());
        assert!(c.set_slot(0, "nonesuch", None).is_err() && c.set_slot(0, "gain", Some(vec![500.0])).is_err());
    }

    #[test]
    fn block_size_does_not_change_the_result() {
        let x: Vec<f32> = (0..5000).map(|i| (i as f32 * 0.02).sin() * 0.5).collect();
        let c = rack_with(&[("gain", "gain_db=-3"), ("invert", "")]);
        let whole = run(&c, &x);
        let mut p = RackProcessor::new(&c, 1, 48_000).unwrap();
        let mut piecewise = x.clone();
        for part in piecewise.chunks_mut(137) {
            p.process_block(&mut [part]);
        }
        assert_eq!(whole, piecewise);
    }

    #[test]
    fn bypass_keeps_the_slots_but_turns_everything_off() {
        let mut c = rack_with(&[("invert", "")]);
        c.bypass();
        assert!(!c.power && !c.slots[0].enabled && c.slots[0].effect.as_deref() == Some("invert"));
        assert!(!c.is_active());
    }

    #[test]
    fn describe_lists_the_slots() {
        let c = rack_with(&[("gain", "gain_db=-6")]);
        let d = c.describe();
        assert!(d.contains("1:gain:on gain_db=-6") && d.contains("2:-") && d.starts_with("power=on in=0 out=0 mix=100"), "{d}");
    }
}
