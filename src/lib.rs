//! bashcap: a transparent bash wrapper that writes the full state of a
//! running shell at every `BASHCAP` call site as one JSON object per line.

pub mod snapshot;

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::PathBuf;

use serde::Serialize;

use crate::bash::rig::{Doing, ExitStatus, Failure, Line, Rig};

pub use snapshot::{Captured, Frame, Snapshot, Value, BASH, POLYFILL};

#[cfg(test)]
mod tests;

/// Where the capture goes. A description: it opens nothing.
pub struct BashCap {
    into: PathBuf,
}

/// bashcap's session: a sink and a tally. Written as each snapshot arrives,
/// so resident memory does not track the run.
pub struct Capturing {
    pub written: usize,
    sink: BufWriter<File>,
}

/// bashcap's output format: the snapshot's own fields under the provenance
/// the wire supplied. Lines are written in arrival order, and each carries
/// both clocks, so ordering downstream is `sort`'s job.
#[derive(Serialize)]
struct Row<'a> {
    sent_at: u64,
    heard_at: u64,
    pid: u32,
    seq: u32,

    #[serde(flatten)]
    snapshot: &'a Snapshot,
}

impl BashCap {
    pub fn writing(into: impl Into<PathBuf>) -> Self {
        Self { into: into.into() }
    }

    fn writing_to(&self) -> String {
        format!("writing {}", self.into.display())
    }
}

impl Rig for BashCap {
    type Session = Capturing;

    fn bash(&self) -> String {
        BASH.to_string()
    }

    fn open(&self) -> Result<Capturing, Failure> {
        let sink = File::create(&self.into).doing(|| self.writing_to())?;

        Ok(Capturing { written: 0, sink: BufWriter::new(sink) })
    }

    fn hear(&self, session: &mut Capturing, said: Line) -> Result<(), Failure> {
        let Some(decoded) = Snapshot::of(&said) else { return Ok(()) };
        let at = || format!("a snapshot from pid {}", said.pid);

        let snapshot = decoded.doing(at)?;
        let row = Row {
            sent_at: said.sent_at.0,
            heard_at: said.heard_at.0,
            pid: said.pid.0,
            seq: said.seq,
            snapshot: &snapshot,
        };

        let json = serde_json::to_string(&row).doing(at)?;
        writeln!(session.sink, "{json}").doing(|| self.writing_to())?;
        session.written += 1;

        Ok(())
    }

    /// A failed flush ends the run rather than being lost in a `Drop`.
    fn end(&self, session: &mut Capturing, _status: ExitStatus) -> Result<(), Failure> {
        session.sink.flush().doing(|| self.writing_to())
    }
}
