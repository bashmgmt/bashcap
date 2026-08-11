//! The record one shell sends back, and the decoder that reads one off the
//! wire.

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use crate::bash::rig::{field, Doing, Failure, Line};
use crate::bash::value::{parse_array, parse_assoc, parse_indexed, parse_rows, parse_scalar};

/// The word every snapshot message begins with, and the one thing that tells
/// bashcap's messages from any other tool's on the same wire.
const TAG: &str = "__BASHCAP__";

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

impl Capture {
    /// `None` for a line that is not one of ours; `Some(Err)` for one that is
    /// and will not decode. Several tools may share the wire, and only the
    /// second of those is anyone's failure.
    pub fn of(line: &Line) -> Option<Result<Self, Failure>> {
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
    fn decode(sections: &[String]) -> Result<Self, Failure> {
        let traced = match section(sections, "traced")? {
            "yes" => true,
            "no" => false,
            other => return Err(reading("traced", format!("it says {other:?}"))),
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
fn frame(row: &[String], traced: bool) -> Result<Frame, Failure> {
    let broken = |what: String| Failure::new("reading a frame", what);

    let [funcname, source, lineno, args @ ..] = row else {
        return Err(broken(format!("a frame is at least three words, got {row:?}")));
    };
    if !traced && !args.is_empty() {
        return Err(broken(format!("arguments on an untraced frame: {row:?}")));
    }

    Ok(Frame {
        funcname: funcname.clone(),
        source: source.clone(),
        lineno: lineno.parse().map_err(|_| broken(format!("line number {lineno:?}")))?,
        args: traced.then(|| args.to_vec()),
    })
}

// ── the sections a message carries ───────────────────────────────────

fn reading(key: &str, cause: impl Into<Box<dyn std::error::Error + Send + Sync>>) -> Failure {
    Failure::new(format!("reading the {key:?} section"), cause)
}

fn section<'a>(sections: &'a [String], key: &str) -> Result<&'a str, Failure> {
    field(sections, key).ok_or_else(|| reading(key, "it is missing"))
}

fn flat(sections: &[String], key: &str) -> Result<Vec<String>, Failure> {
    parse_array(section(sections, key)?).map_err(|cause| reading(key, cause))
}

fn nested(sections: &[String], key: &str) -> Result<Vec<Vec<String>>, Failure> {
    parse_rows(section(sections, key)?).map_err(|cause| reading(key, cause))
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
fn captured(text: &str) -> Result<(String, Captured), Failure> {
    let Declaration { name, attrs, rhs } = Declaration::read(text)
        .ok_or_else(|| Failure::new("reading a variable", format!("not a declaration: {text:?}")))?;
    let at = || format!("reading the variable {name}");

    let value = if attrs.contains('A') {
        Value::Assoc(parse_assoc(rhs).doing(at)?)
    } else if attrs.contains('a') {
        Value::Indexed(parse_indexed(rhs).doing(at)?)
    } else {
        Value::Scalar(parse_scalar(rhs).doing(at)?)
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
