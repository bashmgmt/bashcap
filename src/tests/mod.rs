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

use crate::bash::rig::{Doing, Failure, Line, Master, Rig, Shells};
use crate::bashcap::instrument::WORDS;
use crate::bashcap::{instrument, Capture, Tracing};
use crate::tests::scripts::{bash, Scripts};

/// What a shipped script writes: strict options, the words beside it, and one
/// line naming the hook rather than the words — so a client cannot displace the
/// real ones whichever order the two arrive in.
///
/// `set -u` is the option that reaches furthest into the tool, every name the
/// instrument reads having to be one it set. It leads because a client that
/// joins a session of its own has it on before anything of the tool's is
/// sourced at all.
const VENDORING: &str = "set -euo pipefail\n\
                         source \"$(dirname \"${BASH_SOURCE[0]}\")/bashcap.bash\"\n\
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

#[derive(Default)]
struct Decoded {
    shells: Shells,
    seen: Vec<Capture>,
}

impl Rig for Decoding {
    type Session = Decoded;

    fn bash(&self) -> String {
        instrument(Tracing::Off)
    }

    fn open(&self) -> Result<Self::Session, Failure> {
        Ok(Decoded::default())
    }

    fn hear(&self, decoded: &mut Self::Session, said: Line) -> Result<(), Failure> {
        let shell = decoded.shells.hear(&said)?;

        let Some(capture) = Capture::of(&said, &decoded.shells.at(shell).bash) else {
            return Ok(());
        };

        decoded.seen.push(capture.doing(|| format!("a snapshot from pid {}", said.sent.pid))?);

        Ok(())
    }
}

impl Master for Decoding {}

fn capture(body: &str) -> Vec<Capture> {
    let scripts = script(body);

    Decoding.run(&bash(scripts.at(ENTRY))).unwrap().whole().unwrap().0.seen
}
