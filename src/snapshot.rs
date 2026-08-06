//! The instrument: the bash that harvests a shell's stack, variables and
//! regex state, the decoder that reads one back, and the one rendering of it.

use std::fmt;

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use crate::bash::rig::{field, Line};
use crate::bash::value::{self, emit_assoc, emit_indexed, emit_q_words, emit_scalar};
use crate::bash::value::{BashCodec, QuotedNest};

/// `BASHCAP` and `WITH_BASHCAP`, in every shell. Reached through
/// [`instrument`], which is the one way to compose what gets injected.
pub(crate) const BASH: &str = include_str!("bashcap.bash");

/// The no-op stubs a script vendors, so instrumented call sites stay safe to
/// ship. Under the tool the real definitions are already in place and its
/// `if` is false.
pub const POLYFILL: &str = include_str!("polyfill.bash");

/// Turns on the shell's own recording of call arguments, in every shell.
/// Opt-in, because `extdebug` also makes `ERR`, `DEBUG` and `RETURN` traps
/// inherited by functions and subshells — a change in the subject.
pub(crate) const TRACE: &str = include_str!("trace.bash");

/// The bash to put in a [`Startup`](crate::bash::rig::Startup), for any rig
/// that wants what bashcap harvests. With `tracing_calls`, every frame comes
/// back with the arguments its call was made with — see [`Frame::args`].
pub fn instrument(tracing_calls: bool) -> String {
    match tracing_calls {
        true => format!("{BASH}\n{TRACE}"),
        false => BASH.to_string(),
    }
}

/// The word every snapshot message begins with.
pub const TAG: &str = "__BASHCAP__";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Value {
    Scalar(String),
    Indexed(#[serde(with = "sparse")] IndexMap<usize, String>),
    Assoc(IndexMap<String, String>),
}

/// A bash indexed array is sparse, so its indices are data. They travel as
/// `[index, value]` pairs rather than as object keys, which JSON can only
/// spell as strings and `serde(flatten)` cannot then read back as numbers.
mod sparse {
    use indexmap::IndexMap;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer>(
        items: &IndexMap<usize, String>,
        into: S,
    ) -> Result<S::Ok, S::Error> {
        items.iter().collect::<Vec<_>>().serialize(into)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        from: D,
    ) -> Result<IndexMap<usize, String>, D::Error> {
        Ok(Vec::<(usize, String)>::deserialize(from)?.into_iter().collect())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Captured {
    pub attrs: String,
    pub value: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Frame {
    pub funcname: String,
    pub source: String,
    pub lineno: u32,

    /// The call's arguments, when the shell was recording them. `None` is
    /// "not recorded", never "called with none": bash keeps these only under
    /// `extdebug`, which this crate does not turn on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub frames: Vec<Frame>,
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub state: IndexMap<String, String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rematch: Vec<String>,
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub vars: IndexMap<String, Captured>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

/// One snapshot under the provenance the wire gave it. This is bashcap's
/// output format — one per line — and what `bashcap show` reads back.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Capture {
    pub sent_at: u64,
    pub heard_at: u64,
    pub pid: u32,
    pub seq: u32,

    #[serde(flatten)]
    pub snapshot: Snapshot,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SnapshotError(pub String);

impl fmt::Display for SnapshotError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for SnapshotError {}

impl Capture {
    /// `None` for a line that is not one of ours.
    pub fn of(line: &Line) -> Option<Result<Self, SnapshotError>> {
        let sections = line.behind(TAG)?;

        Some(Snapshot::decode(sections).map(|snapshot| Self {
            sent_at: line.sent_at.0,
            heard_at: line.heard_at.0,
            pid: line.pid.0,
            seq: line.seq,
            snapshot,
        }))
    }
}

impl Snapshot {
    fn decode(sections: &[String]) -> Result<Self, SnapshotError> {
        let traced = match section(sections, "traced")? {
            "yes" => true,
            "no" => false,
            other => return Err(SnapshotError(format!("traced is {other:?}"))),
        };

        let frames = nested(sections, "frames")?
            .into_iter()
            .map(|row| frame(&row, traced))
            .collect::<Result<_, _>>()?;

        let state = flat(sections, "state")?
            .chunks_exact(2)
            .map(|pair| (pair[0].clone(), pair[1].clone()))
            .collect();

        let vars = flat(sections, "vars")?
            .iter()
            .map(|declaration| captured(declaration))
            .collect::<Result<IndexMap<_, _>, _>>()?;

        Ok(Self {
            frames,
            state,
            rematch: flat(sections, "rematch")?,
            vars,
            notes: flat(sections, "notes")?,
        })
    }
}

/// A frame is its call site, then whatever arguments the shell had recorded.
fn frame(row: &[String], traced: bool) -> Result<Frame, SnapshotError> {
    let [funcname, source, lineno, args @ ..] = row else {
        return Err(SnapshotError(format!("a frame is at least three words, got {row:?}")));
    };
    if !traced && !args.is_empty() {
        return Err(SnapshotError(format!("arguments on an untraced frame: {row:?}")));
    }

    Ok(Frame {
        funcname: funcname.clone(),
        source: source.clone(),
        lineno: lineno.parse().map_err(|_| SnapshotError(format!("frame line {lineno:?}")))?,
        args: traced.then(|| args.to_vec()),
    })
}

// ── rendering ────────────────────────────────────────────────────────
//
// One way, and it is the types': a value prints as the bash that would
// declare it, which is what `bash::value` already emits.

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

impl fmt::Display for Frame {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let file = self.source.rsplit('/').next().unwrap_or(&self.source);
        write!(f, "{}@{file}:{}", self.funcname, self.lineno)?;

        match &self.args {
            Some(args) => write!(f, " ({})", emit_q_words(args)),
            None => Ok(()),
        }
    }
}

impl fmt::Display for Capture {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let state = |key| self.snapshot.state.get(key).map_or("?", String::as_str);

        writeln!(
            f,
            "pid {} seq {} shlvl {} subshell {}",
            self.pid,
            self.seq,
            state("shlvl"),
            state("subshell")
        )?;

        for frame in &self.snapshot.frames {
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

// ── the sections a message carries ───────────────────────────────────

fn section<'a>(sections: &'a [String], key: &str) -> Result<&'a str, SnapshotError> {
    field(sections, key).ok_or_else(|| SnapshotError(format!("no {key:?} section")))
}

fn flat(sections: &[String], key: &str) -> Result<Vec<String>, SnapshotError> {
    QuotedNest
        .words(section(sections, key)?)
        .map_err(|cause| SnapshotError(format!("{key}: {cause}")))
}

fn nested(sections: &[String], key: &str) -> Result<Vec<Vec<String>>, SnapshotError> {
    QuotedNest
        .rows(section(sections, key)?)
        .map_err(|cause| SnapshotError(format!("{key}: {cause}")))
}

/// What `${ref[*]@A}` yields: `declare -aX name=rhs`, in its three parts.
struct Declaration<'a> {
    name: &'a str,
    attrs: String,
    rhs: &'a str,
}

impl<'a> Declaration<'a> {
    /// `None` for text that is not one.
    fn read(text: &'a str) -> Option<Self> {
        let mut cursor = text.strip_prefix("declare ").unwrap_or(text).trim_start();
        let mut attrs = String::new();

        while let Some(rest) = cursor.strip_prefix('-') {
            let end = rest.find(' ')?;
            attrs.push_str(&rest[..end]);
            cursor = rest[end + 1..].trim_start();
        }

        Some(match cursor.split_once('=') {
            Some((name, rhs)) => Self { name, attrs, rhs },
            None => Self { name: cursor.trim(), attrs, rhs: "" },
        })
    }
}

/// The attribute letters say which form the right-hand side is in.
fn captured(text: &str) -> Result<(String, Captured), SnapshotError> {
    let Declaration { name, attrs, rhs } = Declaration::read(text)
        .ok_or_else(|| SnapshotError(format!("not a declaration: {text:?}")))?;
    let fail = |cause: value::ParseError| SnapshotError(format!("{name}: {cause}"));

    let value = if attrs.contains('A') {
        Value::Assoc(value::parse_assoc(rhs).map_err(fail)?)
    } else if attrs.contains('a') {
        Value::Indexed(value::parse_indexed(rhs).map_err(fail)?)
    } else {
        Value::Scalar(value::parse_scalar(rhs).map_err(fail)?)
    };

    Ok((name.to_string(), Captured { attrs, value }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn declarations_decode_with_their_attributes() {
        let cases = [
            ("s='hi there'", "s", "", Value::Scalar("hi there".into())),
            (
                r#"declare -a a=([0]="x" [1]="y z")"#,
                "a",
                "a",
                Value::Indexed(IndexMap::from([(0, "x".into()), (1, "y z".into())])),
            ),
            (
                r#"declare -A m=([k]="v" [k2]="v 2" )"#,
                "m",
                "A",
                Value::Assoc(IndexMap::from([("k".into(), "v".into()), ("k2".into(), "v 2".into())])),
            ),
            ("declare -i n='7'", "n", "i", Value::Scalar("7".into())),
        ];
        for (text, name, attrs, value) in cases {
            let (got_name, got) = captured(text).unwrap();
            assert_eq!(got_name, name);
            assert_eq!(got.attrs, attrs, "{text}");
            assert_eq!(got.value, value, "{text}");
        }
    }

    /// A frame carries arguments only where the shell recorded them, and the
    /// difference is a type rather than an empty list.
    #[test]
    fn a_frame_says_whether_its_arguments_are_known() {
        let row = ["f".to_string(), "/x.bash".into(), "12".into(), "one".into(), "two".into()];

        let traced = frame(&row, true).unwrap();
        assert_eq!(traced.args.as_deref(), Some(["one".to_string(), "two".into()].as_slice()));
        assert_eq!(traced.to_string(), "f@x.bash:12 ('one' 'two')");

        let bare = frame(&row[..3], false).unwrap();
        assert_eq!(bare.args, None, "not recorded is not the same as none passed");
        assert_eq!(bare.to_string(), "f@x.bash:12");

        assert_eq!(frame(&row[..3], true).unwrap().args, Some(Vec::new()), "called with none");
        assert!(frame(&row, false).is_err(), "arguments the shell could not have recorded");
        assert!(frame(&row[..2], true).is_err(), "a frame is at least three words");
    }
}
