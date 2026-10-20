//! Shared plumbing for effects that wrap a `soundcraft-dsp` plugin (f124): one prepared plugin
//! per run, fed the chunks of the range in order so its memory (delay lines, reverb tank)
//! carries across `replace_range` chunks.

use std::io;

use soundcraft_dsp::Plugin;

use super::{Run, RunContext};

/// A `soundcraft-dsp` plugin as a [`Run`]. The effect's tail beyond the end of the range is not
/// written: the seam keeps the length, so the tail is cut at the selection's end.
pub struct PluginRun {
    plugin: Box<dyn Plugin>,
    scratch: Vec<Vec<f32>>,
}

impl PluginRun {
    /// Create plugin `id`, prepared for this run's rate and channel count, with `settings`
    /// (`(plugin parameter id, value)`) applied.
    pub fn new(id: &str, ctx: &RunContext, settings: &[(&str, f32)]) -> io::Result<PluginRun> {
        let mut plugin = soundcraft_dsp::create(id).ok_or_else(|| io::Error::other(format!("soundcraft-dsp has no plugin {id:?}")))?;
        plugin.prepare(ctx.sample_rate as f32, super::CHUNK_HINT, ctx.channels.len().max(1));
        for (key, value) in settings {
            if !plugin.set_param(key, *value) {
                return Err(io::Error::other(format!("plugin {id} has no setting {key:?}")));
            }
        }
        plugin.reset();
        Ok(PluginRun { plugin, scratch: Vec::new() })
    }
}

impl Run for PluginRun {
    fn process(&mut self, _first_frame: u64, input: &[&[f32]], output: &mut [&mut [f32]]) -> io::Result<()> {
        self.scratch.resize_with(input.len(), Vec::new);
        for (buf, src) in self.scratch.iter_mut().zip(input) {
            buf.clear();
            buf.extend_from_slice(src);
        }
        let frames = input.first().map_or(0, |c| c.len());
        self.plugin.process(&mut self.scratch, frames);
        for (o, buf) in output.iter_mut().zip(&self.scratch) {
            o.copy_from_slice(&buf[..o.len()]);
        }
        Ok(())
    }
}
