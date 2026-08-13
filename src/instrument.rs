//! The bash bashcap injects into a subject's shells.

use crate::bash::stack;

/// `BASHCAP` and `WITH_BASHCAP`, in every shell. Reached through
/// [`instrument`], which is the one way to compose what gets injected.
pub(crate) const BASH: &str = include_str!("bashcap.bash");

/// Turns on the shell's own recording of call arguments, in every shell.
pub(crate) const TRACE: &str = include_str!("trace.bash");

/// Whether the subject's shells record what each call was passed. Opt-in,
/// because `extdebug` also makes `ERR`, `DEBUG` and `RETURN` traps inherited
/// by functions and subshells — a change in the subject.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Tracing {
    Off,

    /// Every frame comes back with the arguments its call was made with —
    /// see [`Frame::args`](super::Frame::args).
    Calls,
}

/// The bash to put in a [`Startup`](crate::bash::rig::Startup), for any rig
/// that wants what bashcap harvests. The frame walk comes with it, since a
/// snapshot reports one.
pub fn instrument(tracing: Tracing) -> String {
    match tracing {
        Tracing::Off => stack::with(&[BASH]),
        Tracing::Calls => stack::with(&[BASH, TRACE]),
    }
}
