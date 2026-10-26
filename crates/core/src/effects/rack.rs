//! Length-preserving Effects Rack processing (F1).

use std::io;

use super::{Effect, Run, RunContext};

pub struct RackSlot<'a> {
    pub effect: &'a dyn Effect,
    pub params: Vec<f64>,
    pub enabled: bool,
}

pub struct RackConfig<'a> {
    pub slots: Vec<RackSlot<'a>>,
    pub input_db: f64,
    pub output_db: f64,
    /// Linear wet share, 0 through 1.
    pub mix: f64,
}

pub struct RackRun<'a> {
    _marker: std::marker::PhantomData<&'a ()>,
}

/// Validate and prepare enabled slots once. Disabled slots are never inspected or started.
#[allow(unused_variables, clippy::todo)]
pub fn begin<'a>(config: &'a RackConfig<'a>, ctx: &RunContext<'_>) -> io::Result<RackRun<'a>> {
    todo!()
}

impl Run for RackRun<'_> {
    #[allow(unused_variables, clippy::todo)]
    fn process(&mut self, first_frame: u64, input: &[&[f32]], output: &mut [&mut [f32]]) -> io::Result<()> {
        todo!()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod locked_129a {
    use super::*;
    use crate::effects::card129_tests::*;

    fn run(config: &RackConfig<'_>, input: &[f32]) -> Vec<f32> {
        let f = Fixture::new("rack", &[input.to_vec()]);
        let ctx = context(&f.doc, 0..input.len() as u64, vec![0]);
        let mut processor = begin(config, &ctx).unwrap();
        let mut output = vec![0.0; input.len()];
        processor.process(0, &[input], &mut [&mut output]).unwrap();
        output
    }

    #[test]
    fn three_slots_run_in_order() {
        let config = config(vec![slot(&Bias), slot(&Double), slot(&super::super::Invert)]);
        assert_eq!(run(&config, &[1.0, -2.0, 0.0]), vec![-4.0, 2.0, -2.0]);
    }

    #[test]
    fn disabled_slot_never_begins_or_processes_or_validates() {
        let config = config(vec![RackSlot { effect: &Bomb, params: vec![f64::NAN], enabled: false }, slot(&Double)]);
        assert_eq!(run(&config, &[1.0, -2.0]), vec![2.0, -4.0]);
    }

    #[test]
    fn gains_surround_chain_and_mix_uses_original_dry() {
        let config = RackConfig { slots: vec![slot(&Bias)], input_db: 6.020599913279624, output_db: -6.020599913279624, mix: 0.25 };
        let out = run(&config, &[1.0, -2.0]);
        assert!((out[0] - 1.125).abs() < 0.000001);
        assert!((out[1] - -1.875).abs() < 0.000001);
    }

    #[test]
    fn half_wet_invert_cancels_and_dry_endpoint_is_exact() {
        let mut config = config(vec![slot(&super::super::Invert)]);
        config.mix = 0.5;
        assert_eq!(run(&config, &[0.5, -2.0]), vec![0.0, 0.0]);
        config.mix = 0.0;
        config.input_db = 12.0;
        assert_eq!(run(&config, &[0.5, -2.0]), vec![0.5, -2.0]);
    }

    #[test]
    fn state_is_retained_between_chunks_and_channels_are_independent() {
        let f = Fixture::new("rack-state", &[vec![1.0; 4], vec![2.0; 4]]);
        let config = config(vec![slot(&Accumulator)]);
        let mut p = begin(&config, &context(&f.doc, 0..4, vec![0, 1])).unwrap();
        let (mut l, mut r) = ([0.0; 2], [0.0; 2]);
        p.process(0, &[&[1.0, 1.0], &[2.0, 2.0]], &mut [&mut l, &mut r]).unwrap();
        assert_eq!(l, [1.0, 2.0]);
        assert_eq!(r, [2.0, 4.0]);
        p.process(2, &[&[1.0, 1.0], &[2.0, 2.0]], &mut [&mut l, &mut r]).unwrap();
        assert_eq!(l, [3.0, 4.0]);
        assert_eq!(r, [6.0, 8.0]);
    }

    #[test]
    fn empty_chain_keeps_float_headroom() {
        assert_eq!(run(&config(vec![]), &[2.0, -3.0]), vec![2.0, -3.0]);
    }

    #[test]
    fn invalid_controls_and_enabled_parameters_are_refused() {
        let f = Fixture::new("rack-invalid", &[vec![1.0]]);
        let ctx = context(&f.doc, 0..1, vec![0]);
        for mix in [-0.01, 1.01, f64::NAN, f64::INFINITY] {
            let mut c = config(vec![]);
            c.mix = mix;
            assert_eq!(begin(&c, &ctx).err().unwrap().kind(), io::ErrorKind::InvalidInput);
        }
        for (input_db, output_db) in [(f64::NAN, 0.0), (0.0, f64::INFINITY)] {
            let c = RackConfig { slots: vec![], input_db, output_db, mix: 1.0 };
            assert_eq!(begin(&c, &ctx).err().unwrap().kind(), io::ErrorKind::InvalidInput);
        }
        let c = config(vec![RackSlot { effect: &super::super::Gain, params: vec![999.0], enabled: true }]);
        assert_eq!(begin(&c, &ctx).err().unwrap().kind(), io::ErrorKind::InvalidInput);
    }

    #[test]
    fn malformed_chunk_is_rejected_without_touching_output() {
        let f = Fixture::new("rack-shape", &[vec![1.0; 2]]);
        let c = config(vec![]);
        let mut p = begin(&c, &context(&f.doc, 0..2, vec![0])).unwrap();
        let mut out = [9.0];
        assert_eq!(p.process(0, &[&[1.0, 2.0]], &mut [&mut out]).unwrap_err().kind(), io::ErrorKind::InvalidInput);
        assert_eq!(out, [9.0]);
    }
}
