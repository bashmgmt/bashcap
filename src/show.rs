//! Reading a written capture back, and the one rendering of one.
//!
//! A value prints as the bash that would declare it, which is what
//! [`bash::value`](crate::bash::value) already emits — so `bashcap show`, a
//! test and a library caller all print the same text.

use std::fmt;

use super::{Capture, Captured, Value};
use crate::bash::rig::{Doing, Failure};
use crate::bash::value::{emit_assoc, emit_indexed, emit_scalar};

/// Every capture in a file written by [`BashCap`](super::BashCap): one JSON
/// object per line, in the order they were heard.
pub fn captures(text: &str) -> Result<Vec<Capture>, Failure> {
    text.lines()
        .enumerate()
        .map(|(at, line)| serde_json::from_str(line).doing(|| format!("reading line {}", at + 1)))
        .collect()
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Scalar(text) => write!(f, "{}", emit_scalar(text)),
            Self::Indexed(items) => write!(f, "{}", emit_indexed(items)),
            Self::Assoc(items) => write!(f, "{}", emit_assoc(items)),
        }
    }
}

impl fmt::Display for Captured {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let attrs = if self.attrs.is_empty() { "--" } else { &self.attrs };

        write!(f, "[{attrs}] {}", self.value)
    }
}

impl fmt::Display for Capture {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let state = |key| self.snapshot.state.get(key).map_or("?", String::as_str);

        writeln!(
            f,
            "pid {} seq {} shlvl {} subshell {}",
            self.sent.pid,
            self.sent.seq,
            state("shlvl"),
            state("subshell")
        )?;

        for frame in self.snapshot.stack.frames() {
            writeln!(f, "    at    {frame}")?;
        }
        for note in &self.snapshot.notes {
            writeln!(f, "    note  {note}")?;
        }
        for (name, var) in &self.snapshot.vars {
            writeln!(f, "    var   {name} {var}")?;
        }
        if !self.snapshot.rematch.is_empty() {
            writeln!(f, "    regex {}", self.snapshot.rematch.join(" | "))?;
        }
        Ok(())
    }
}
