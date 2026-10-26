//! Punch Again (roadmap C041): remember the last completed punch range and re-arm it, so the same
//! punch can be repeated quickly for another take. This is only the memory and the re-arm; the
//! command, the key and the take counter live in the app. It sits beside [`crate::mtrecord`], whose
//! [`Punch`] it arms.

use soundcraft_model::time::{Range, Samples};

use crate::mtrecord::Punch;
use crate::multitrack::MtError;

/// The last completed punch range, kept so it can be armed again.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PunchAgain {
    last: Option<Range>,
}

impl PunchAgain {
    /// A memory with no punch in it.
    pub fn new() -> PunchAgain {
        PunchAgain { last: None }
    }

    /// Remember a completed punch from frame `from` to frame `to` (session frames). The range
    /// must be at least one frame long and start at 0 or later; a refused range leaves the
    /// remembered one as it was.
    pub fn record(&mut self, from: Samples, to: Samples) -> Result<(), MtError> {
        if from < 0 || to <= from {
            return Err(MtError("a punch range needs 0 <= in < out".into()));
        }
        self.last = Some(Range::new(from, to));
        Ok(())
    }

    /// The last remembered punch range, `None` when there is none.
    pub fn last(&self) -> Option<Range> {
        self.last
    }

    /// Arm the remembered range on `punch`: set its range and turn it on, leaving the pre-roll as
    /// it is. `session_end` is the session's content end in frames; a range that ends after it is
    /// outside the session. Returns the range. Nothing changes on an error.
    pub fn rearm(&self, punch: &mut Punch, session_end: Samples) -> Result<Range, MtError> {
        let Some(r) = self.last else {
            return Err(MtError("no punch to repeat yet".into()));
        };
        if r.end > session_end {
            return Err(MtError("the last punch is outside the session".into()));
        }
        punch.set(r.start, r.end)?;
        punch.on = true;
        Ok(r)
    }

    /// Forget the remembered range.
    pub fn clear(&mut self) {
        self.last = None;
    }
}
