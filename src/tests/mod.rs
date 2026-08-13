//! bashcap against a real shell, which is the only way to see what the
//! instrument harvests from one. What decodes without a shell is tested beside
//! the decoder.
//!
//! | | |
//! |---|---|
//! | [`snapshot`] | what one `BASHCAP` call reports, and what survives the wire |
//! | [`tracing`] | call arguments: where the shell was recording, and the tool's own switch |
//! | [`writing`] | the JSON line a run writes, and reading it back |
//! | [`vendoring`] | the stub a client ships, and what the instrument may not do to a shell |

mod snapshot;
mod tracing;
mod vendoring;
mod writing;

use std::path::{Path, PathBuf};

use crate::bash::rig::{Doing, Failure, Halt, Line, Master, Rig};
use crate::bashcap::instrument::WORDS;
use crate::bashcap::{instrument, Capture, Tracing};

/// The one line a client writes. It names the hook rather than the words, so a
/// client cannot displace the real ones whichever order the two arrive in.
const GUARD: &str = "declare -F __bc_capture >/dev/null || __bc_capture() { :; }";

/// A script that vendors the words and guards them, as a shipped one would.
/// What it sources is the file the tool injects, byte for byte.
fn script(temp: &Path, body: &str) -> PathBuf {
    let words = temp.join("bashcap.bash");
    std::fs::write(&words, WORDS).unwrap();

    let entry = temp.join("main.bash");
    let vendoring = format!("source {}\n{GUARD}\n", words.display());
    std::fs::write(&entry, format!("{vendoring}{body}")).unwrap();
    entry
}

/// bashcap's bash, decoded but not written, so assertions read typed captures
/// rather than JSON. Every snapshot must decode.
struct Decoding;

impl Rig for Decoding {
    type Session = Vec<Capture>;

    fn bash(&self) -> String {
        instrument(Tracing::Off)
    }

    fn open(&self) -> Result<Self::Session, Failure> {
        Ok(Vec::new())
    }

    fn hear(&self, seen: &mut Self::Session, said: Line) -> Result<(), Halt> {
        let Some(decoded) = Capture::of(&said) else { return Ok(()) };

        seen.push(decoded.doing(|| format!("a snapshot from pid {}", said.sent.pid))?);

        Ok(())
    }
}

impl Master for Decoding {}

fn capture(body: &str) -> Vec<Capture> {
    let temp = tempfile::tempdir().unwrap();
    let ran = Decoding.run(&["bash".into(), script(temp.path(), body).into_os_string()]).unwrap();

    ran.whole().unwrap().0
}
