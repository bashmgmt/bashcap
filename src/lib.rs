//! bashcap: a transparent bash wrapper that writes the full state of a
//! running shell at every `BASHCAP` call site as one JSON object per line.

mod instrument;
mod show;
mod snapshot;

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::PathBuf;

use crate::bash::rig::{Doing, ExitStatus, Failure, Line, Rig, Startup};

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

/// bashcap's session: a sink and a tally. Written as each snapshot arrives,
/// so resident memory does not track the run.
pub struct Capturing {
    pub written: usize,
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
    fn startup(&self) -> Startup {
        Startup { bash: instrument(self.tracing), ..Default::default() }
    }

    fn open(&self) -> Result<Capturing, Failure> {
        let sink = File::create(&self.into).doing(|| self.writing_to())?;

        Ok(Capturing { written: 0, sink: BufWriter::new(sink) })
    }

    /// One JSON object per line, in arrival order. Each carries both clocks,
    /// so ordering downstream is exact and is `sort`'s job.
    fn hear(&self, session: &mut Capturing, said: Line) -> Result<(), Failure> {
        let Some(decoded) = Capture::of(&said) else { return Ok(()) };
        let at = || format!("a snapshot from pid {}", said.sent.pid);

        let json = serde_json::to_string(&decoded.doing(at)?).doing(at)?;
        writeln!(session.sink, "{json}").doing(|| self.writing_to())?;
        session.written += 1;

        Ok(())
    }

    /// A failed flush ends the run rather than being lost in a `Drop`.
    fn end(&self, session: &mut Capturing, _status: ExitStatus) -> Result<(), Failure> {
        session.sink.flush().doing(|| self.writing_to())
    }
}
