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

use crate::bash::rig::{run, Doing, Failure, Line, Rig, Startup};
use crate::bashcap::{instrument, Capture, Tracing};

/// The stub a client vendors. bashcap ships it as an asset rather than as a
/// value of its own: whether it is installed is the client's decision, made by
/// the guard below.
const POLYFILL: &str = include_str!("../../../assets/bashcap_polyfill.bash");

const GUARD: &str = "declare -F BASHCAP >/dev/null || __define_bashcap_polyfill";

/// A script that vendors the polyfill and guards it, as a shipped one would.
fn script(temp: &Path, body: &str) -> PathBuf {
    let polyfill = temp.join("polyfill.bash");
    std::fs::write(&polyfill, POLYFILL).unwrap();

    let entry = temp.join("main.bash");
    let vendoring = format!("source {}\n{GUARD}\n", polyfill.display());
    std::fs::write(&entry, format!("{vendoring}{body}")).unwrap();
    entry
}

/// bashcap's bash, decoded but not written, so assertions read typed captures
/// rather than JSON. Every snapshot must decode.
struct Decoding;

impl Rig for Decoding {
    type Session = Vec<Capture>;

    fn startup(&self) -> Startup {
        Startup { bash: instrument(Tracing::Off), ..Default::default() }
    }

    fn open(&self) -> Result<Self::Session, Failure> {
        Ok(Vec::new())
    }

    fn hear(&self, seen: &mut Self::Session, said: Line) -> Result<(), Failure> {
        let Some(decoded) = Capture::of(&said) else { return Ok(()) };

        seen.push(decoded.doing(|| format!("a snapshot from pid {}", said.sent.pid))?);

        Ok(())
    }
}

fn capture(body: &str) -> Vec<Capture> {
    let temp = tempfile::tempdir().unwrap();
    let ran = run(&Decoding, &["bash".into(), script(temp.path(), body).into_os_string()]).unwrap();

    ran.whole().unwrap().0
}
