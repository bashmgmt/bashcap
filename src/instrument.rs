//! The bash bashcap injects into a subject's shells.
//!
//! Two halves. [`WORDS`] is what a call site says — the same file a client
//! vendors, naming nothing of the protocol. [`EFFECT`] is what a call does,
//! and needs both the protocol and the frame walk.

use bash_interop::rig::Layout;
use bash_interop::stack;
use bash_strings::emit_scalar;

/// `BASHCAP` and `WITH_BASHCAP`. Shipped as an asset so a client's copy and
/// the injected one are the same bytes.
pub(crate) const WORDS: &str = include_str!("../assets/bashcap.bash");

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

/// The bash a rig hands the subject, for any rig that wants what bashcap
/// harvests, generated against the settled workspace: the words speak under
/// `BASHCAP`, and the join carries the coordinate baked in, quoted. The
/// frame walk comes with it, since a snapshot reports one. The join comes
/// before the trace: `TRACE` arms itself from the next command, which must
/// be the subject's, not the join.
pub fn instrument(at: &Layout, tracing: Tracing) -> String {
    let join = format!("BC_JOIN BASHCAP {}\n", emit_scalar(at.text()));
    match tracing {
        Tracing::Off => stack::with_walk(&[WORDS, EFFECT, &join]),
        Tracing::Calls => stack::with_walk(&[WORDS, EFFECT, &join, TRACE]),
    }
}
