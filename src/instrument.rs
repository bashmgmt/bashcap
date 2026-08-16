//! The bash bashcap injects into a subject's shells.
//!
//! Two halves. [`WORDS`] is what a call site says; [`EFFECT`] is what a
//! call does, and needs both the protocol and the frame walk.

use bash_interop::rig::Layout;
use bash_interop::stack;
use bash_strings::emit_scalar;

/// `BASHCAP` and `WITH_BASHCAP`.
pub(crate) const WORDS: &str = include_str!("words.bash");

/// `__bc_capture`, which is what makes those words do anything.
pub(crate) const EFFECT: &str = include_str!("effect.bash");

/// Turns on the shell's own recording of call arguments, in every shell.
pub(crate) const TRACE: &str = include_str!("trace.bash");

/// Whether the subject's shells record what each call was passed. Opt-in,
/// because `extdebug` also makes `ERR`, `DEBUG` and `RETURN` traps inherited
/// by functions and subshells — a change in the subject.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Tracing {
    Off,

    /// Every frame comes back with the arguments its call was made with —
    /// see [`Frame::args`](bash_interop::stack::Frame::args).
    ///
    /// Sourced through `BASH_ENV`, this arms itself before the subject's first
    /// line. Sourced into a shell that is already running — by hand, or under
    /// [`Serving`](bash_interop::rig::Serving) — it installs a `DEBUG` trap
    /// there, replacing one the client had.
    Calls,
}

/// The definitions a rig hands the subject, for any rig that wants what
/// bashcap harvests: the words speak under `BASHCAP`, the frame walk comes
/// with them since a snapshot reports one, and `BASHCAP_INIT <dir>` is the
/// channel setup on offer — defined here, called by nothing here. Under
/// [`Tracing::Calls`] the init also arms the trace, after the join: arming
/// hooks the next command, which must be the subject's.
pub fn instrument(tracing: Tracing) -> String {
    const INIT: &str = r#"
BASHCAP_INIT() {
    BC_JOIN BASHCAP "${1:?the session workspace}" || return
}
"#;
    const INIT_TRACING: &str = r#"
BASHCAP_INIT() {
    BC_JOIN BASHCAP "${1:?the session workspace}" || return
    trap '__bc_arm' DEBUG
}
"#;
    match tracing {
        Tracing::Off => stack::with_walk(&[WORDS, EFFECT, INIT]),
        Tracing::Calls => stack::with_walk(&[WORDS, EFFECT, TRACE, INIT_TRACING]),
    }
}

/// The standard initiation: `BASHCAP_INIT '<dir>'`. Data — written into a
/// provisioned `bash_env.bash`, or said by a client's own line.
pub fn joining(at: &Layout) -> String {
    format!("BASHCAP_INIT {}\n", emit_scalar(at.text()))
}
