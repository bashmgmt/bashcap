//! bashcap against a real shell, which is the only way to see what the
//! instrument harvests from one. What decodes without a shell is tested beside
//! the decoder.
//!
//! | | |
//! |---|---|
//! | [`snapshot`] | what one `BASHCAP` call reports, and what survives the wire |
//! | [`tracing`] | call arguments: where the shell was recording, and the tool's own switch |
//! | [`writing`] | the JSON line a run writes, and reading it back |
//! | [`shipped`] | what the injected bash may not do to a shell |

mod shipped;
mod snapshot;
mod tracing;
mod writing;

use std::sync::Arc;

use bash_interop::rig::{Answer, Doing, Driving, Failure, Layout, Message, Provision, Reacting, Rig, Shell};

use crate::{Capture, Tracing, instrument};
use bash_interop::scratch::{Scripts, bash};

/// A subject script under the strict options a shipped one has — `set -u`
/// is the option that reaches furthest into the tool, every name the
/// instrument reads having to be one it set. The words arrive through the
/// session, as they do for any subject.
pub(super) fn script(body: &str) -> Scripts {
    Scripts::of(&[(
        ENTRY,
        &format!("set -euo pipefail\n{body}"),
    )])
}

/// Every script these tests build starts here.
pub(super) const ENTRY: &str = "main.bash";

/// bashcap's bash, decoded but not written, so assertions read typed captures
/// rather than JSON. Every snapshot must decode.
struct Decoding;

struct Decoded {
    shell: Arc<Shell>,
    seen: Vec<Capture>,
}

impl Rig for Decoding {
    type Reaction = Decoded;

    fn bash(&self, _at: &Layout) -> String {
        instrument(Tracing::Off)
    }

    async fn joined(&self, _at: &Layout, shell: Arc<Shell>) -> Result<Decoded, Failure> {
        Ok(Decoded {
            shell,
            seen: Vec::new(),
        })
    }
}

impl Reacting for Decoded {
    type Kept = Vec<Capture>;

    async fn hear(&mut self, said: Message) -> Result<(), Failure> {
        let Some(capture) = Capture::of(&said, &self.shell) else {
            return Ok(());
        };

        self.seen
            .push(capture.doing(|| format!("a snapshot from pid {}", self.shell.pid))?);

        Ok(())
    }

    async fn answer(&mut self, asked: Message) -> Result<Answer, Failure> {
        self.hear(asked).await?;

        Ok(Answer::unknown())
    }

    async fn finish(self) -> Result<Vec<Capture>, Failure> {
        Ok(self.seen)
    }
}

impl Driving for Decoding {}

/// Every snapshot a script produced, shell by shell in the order they joined.
async fn capture(body: &str) -> Vec<Capture> {
    let scripts = script(body);
    let ran = Decoding
        .run(&bash(scripts.at(ENTRY)), |at| {
            Ok(vec![at.bash_env(
                Provision::Joining(&crate::joining(at)),
            )?])
        })
        .await
        .unwrap()
        .whole()
        .unwrap();

    ran.shells.into_iter().flat_map(|at| at.kept).collect()
}
