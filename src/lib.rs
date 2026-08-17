//! bashcap: a transparent bash wrapper that writes the full state of a
//! running shell at every `BASHCAP` call site as one JSON object per line.

mod instrument;
mod show;
mod snapshot;

use std::cell::RefCell;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use bash_interop::rig::{Answer, Doing, Driving, Failure, Layout, Message, Reacting, Rig, Serving, Shell};

pub use instrument::{Tracing, instrument, joining};
pub use show::captures;
pub use snapshot::{Capture, Snapshot, Value, Variable};

#[cfg(test)]
mod tests;

/// The one file a run's captures go to. Every shell writes to it, so it is the
/// rig's and each reaction holds a share.
type Sink = Rc<RefCell<BufWriter<File>>>;

/// Where the capture goes, and whether to ask the shell to record the
/// arguments each call was made with.
pub struct BashCap {
    into: PathBuf,
    sink: Sink,
    tracing: Tracing,
}

impl BashCap {
    /// Opens the file, truncating it: an unwritable path is a failure of the
    /// caller's own before any shell has run.
    pub fn writing(into: impl Into<PathBuf>) -> Result<Self, Failure> {
        let into = into.into();
        let file = File::create(&into).doing(|| format!("writing {}", into.display()))?;
        let sink = Rc::new(RefCell::new(BufWriter::new(file)));

        Ok(Self {
            into,
            sink,
            tracing: Tracing::Off,
        })
    }

    /// Ask the subject's shells to record what each call was passed. This
    /// changes the subject: `extdebug` makes `ERR`, `DEBUG` and `RETURN`
    /// traps inherited by functions and subshells.
    #[must_use]
    pub fn tracing_calls(mut self) -> Self {
        self.tracing = Tracing::Calls;
        self
    }
}

impl Rig for BashCap {
    type Reaction = Capturing;

    /// The instrument reaches every shell that joins, which is why tracing
    /// lives here and not on the command line: what a provisioned file or a
    /// client's init call brings must be the same everywhere.
    fn bash(&self, _at: &Layout) -> String {
        instrument(self.tracing)
    }

    async fn joined(&self, _at: &Layout, shell: Arc<Shell>) -> Result<Capturing, Failure> {
        Ok(Capturing {
            shell,
            into: self.into.clone(),
            sink: Rc::clone(&self.sink),
            written: 0,
        })
    }
}

/// One shell's captures, written as they arrive so resident memory does not
/// track the run.
///
/// The shell is a member: a walk is read against the shell it was taken in, and
/// this one was handed the shell before its first message could arrive.
pub struct Capturing {
    shell: Arc<Shell>,
    into: PathBuf,
    sink: Sink,
    written: usize,
}

impl Capturing {
    fn writing(&self) -> String {
        format!("writing {}", self.into.display())
    }
}

impl Reacting for Capturing {
    /// How many snapshots this shell wrote. What they said went to the file.
    type Kept = usize;

    /// One JSON object per line, as they arrive. Each carries both clocks, so
    /// ordering downstream is `sort`'s job.
    async fn hear(&mut self, said: Message) -> Result<(), Failure> {
        let at = || format!("a snapshot from pid {}", self.shell.pid);

        let Some(decoded) = Capture::of(&said, &self.shell) else {
            return Ok(());
        };
        let json = serde_json::to_string(&decoded.doing(at)?).doing(at)?;

        writeln!(self.sink.borrow_mut(), "{json}").doing(|| self.writing())?;
        self.written += 1;

        Ok(())
    }

    /// bashcap only listens, so a shell that asks it something is told the word
    /// is unknown.
    async fn answer(&mut self, asked: Message) -> Result<Answer, Failure> {
        self.hear(asked).await?;

        Ok(Answer::unknown())
    }

    /// A failed flush ends the run rather than being lost in a `Drop`. The sink
    /// outlives every reaction, so the last shell to finish is what puts the
    /// run's tail on disk.
    async fn finish(self) -> Result<usize, Failure> {
        self.sink.borrow_mut().flush().doing(|| self.writing())?;

        Ok(self.written)
    }
}

/// Either orchestration: the instrument is the same text, and what it
/// harvests is the same either way.
///
/// [`Tracing::Calls`] is the exception in degree: sourced through `BASH_ENV`
/// it arms itself before the subject's first line, sourced into a running
/// shell it installs a `DEBUG` trap there.
impl Driving for BashCap {}

impl Serving for BashCap {}
