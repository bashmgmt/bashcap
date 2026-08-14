//! bashcap: a transparent bash wrapper that writes the full state of a
//! running shell at every `BASHCAP` call site as one JSON object per line.

mod instrument;
mod show;
mod snapshot;

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::PathBuf;

use crate::bash::rig::{Doing, Failure, Line, Master, Rig, Shells, Slave};

pub use instrument::{instrument, Tracing};
pub use show::captures;
pub use snapshot::{Capture, Captured, Snapshot, Value};

#[cfg(test)]
mod tests;

/// Where the capture goes, and whether to ask the shell to record the
/// arguments each call was made with. A description: it opens nothing.
pub struct BashCap {
    into: PathBuf,
    tracing: Tracing,
}

/// bashcap's session: a sink, a tally, and the shells that have joined so far.
/// Written as each snapshot arrives, so resident memory does not track the run.
///
/// The register is the one thing kept: a walk is read against the shell it was
/// taken in, and a decoder reading a run as it arrives has to know that shell
/// by the time the walk turns up. It grows by one entry per shell, not per
/// message.
pub struct Capturing {
    pub written: usize,
    shells: Shells,
    sink: BufWriter<File>,
}

impl BashCap {
    pub fn writing(into: impl Into<PathBuf>) -> Self {
        Self { into: into.into(), tracing: Tracing::Off }
    }

    /// Ask the subject's shells to record what each call was passed. This
    /// changes the subject: `extdebug` makes `ERR`, `DEBUG` and `RETURN`
    /// traps inherited by functions and subshells.
    pub fn tracing_calls(mut self) -> Self {
        self.tracing = Tracing::Calls;
        self
    }

    fn writing_to(&self) -> String {
        format!("writing {}", self.into.display())
    }
}

impl Rig for BashCap {
    type Session = Capturing;

    /// bashcap's instrument reaches every shell through the prelude, which
    /// is why tracing lives here and not in the command line: `BASH_ENV`
    /// reaches a subject's children, its argv does not.
    fn bash(&self) -> String {
        instrument(self.tracing)
    }

    fn open(&self) -> Result<Capturing, Failure> {
        let sink = File::create(&self.into).doing(|| self.writing_to())?;

        Ok(Capturing { written: 0, shells: Shells::default(), sink: BufWriter::new(sink) })
    }

    /// One JSON object per line, in arrival order. Each carries both clocks,
    /// so ordering downstream is exact and is `sort`'s job.
    ///
    /// Every message goes through the register first, whether or not it is one
    /// of ours: that is what opens a shell and what places the rest under it.
    fn hear(&self, session: &mut Capturing, said: Line) -> Result<(), Failure> {
        let at = || format!("a snapshot from pid {}", said.sent.pid);
        let shell = session.shells.hear(&said)?;

        let Some(decoded) = Capture::of(&said, &session.shells.at(shell).bash) else {
            return Ok(());
        };

        let json = serde_json::to_string(&decoded.doing(at)?).doing(at)?;
        writeln!(session.sink, "{json}").doing(|| self.writing_to())?;
        session.written += 1;

        Ok(())
    }

    /// A failed flush ends the run rather than being lost in a `Drop`.
    fn end(&self, session: &mut Capturing) -> Result<(), Failure> {
        session.sink.flush().doing(|| self.writing_to())
    }
}

/// Either orchestration: the instrument is the same text, and what it harvests
/// is the same either way.
///
/// [`Tracing::Calls`] is the exception in degree: reached as a `Master` it arms
/// itself before the subject's first line, reached as a `Slave` it installs a
/// `DEBUG` trap in a shell that is already running.
impl Master for BashCap {}
impl Slave for BashCap {}
