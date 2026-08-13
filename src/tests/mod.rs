//! bashcap against a real shell, which is the only way to see what the
//! instrument harvests from one. What decodes without a shell is tested beside
//! the decoder.
//!
//! | | |
//! |---|---|
//! | [`snapshot`] | what one `BASHCAP` call reports, and what survives the wire |
//! | [`tracing`] | call arguments: where the shell was recording, and the tool's own switch |
//! | [`writing`] | the JSON line a run writes, and reading it back |
//! | [`vendoring`] | the words a client ships, and what the instrument may not do to a shell |

mod snapshot;
mod tracing;
mod vendoring;
mod writing;

use crate::bash::rig::{Doing, Failure, Line, Master, Rig};
use crate::bashcap::instrument::WORDS;
use crate::bashcap::{instrument, Capture, Tracing};
use crate::tests::scripts::{bash, Scripts};

/// What a shipped script writes: the words beside it, and one line naming the
/// hook rather than the words — so a client cannot displace the real ones
/// whichever order the two arrive in.
const VENDORING: &str = "source \"$(dirname \"${BASH_SOURCE[0]}\")/bashcap.bash\"\n\
                         declare -F __bc_capture >/dev/null || __bc_capture() { :; }\n";

/// A script vendoring the words as a shipped one would. What it sources is the
/// file the tool injects, byte for byte.
pub(super) fn script(body: &str) -> Scripts {
    Scripts::of(&[("bashcap.bash", WORDS), (ENTRY, &format!("{VENDORING}{body}"))])
}

/// Every script these tests build starts here.
pub(super) const ENTRY: &str = "main.bash";

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

    fn hear(&self, seen: &mut Self::Session, said: Line) -> Result<(), Failure> {
        let Some(decoded) = Capture::of(&said) else { return Ok(()) };

        seen.push(decoded.doing(|| format!("a snapshot from pid {}", said.sent.pid))?);

        Ok(())
    }
}

impl Master for Decoding {}

fn capture(body: &str) -> Vec<Capture> {
    let scripts = script(body);

    Decoding.run(&bash(scripts.at(ENTRY))).unwrap().whole().unwrap().0
}
