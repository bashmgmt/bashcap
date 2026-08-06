//! The instrument: the bash that harvests a shell's stack, variables and
//! regex state, and the decoder that reads one back.

use indexmap::IndexMap;
use serde::Serialize;

use crate::bash::rig::{field, Line};
use crate::bash::value::{self, BashCodec, QuotedNest};

/// `BASHCAP` and `WITH_BASHCAP`, in every shell.
pub const BASH: &str = include_str!("bashcap.bash");

/// The no-op stubs a script vendors, so instrumented call sites stay safe to
/// ship. Under the tool the real definitions are already in place and its
/// `if` is false.
pub const POLYFILL: &str = include_str!("polyfill.bash");

/// The word every snapshot message begins with.
pub const TAG: &str = "__BASHCAP__";

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Value {
    Scalar(String),
    Indexed(IndexMap<usize, String>),
    Assoc(IndexMap<String, String>),
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Captured {
    pub attrs: String,
    pub value: Value,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Frame {
    pub funcname: String,
    pub source: String,
    pub lineno: u32,
}

#[derive(Debug, Clone, Serialize)]
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

#[derive(Debug, Clone, PartialEq)]
pub struct SnapshotError(pub String);

impl std::fmt::Display for SnapshotError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for SnapshotError {}

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

impl Snapshot {
    /// `None` for a line that is not one of ours.
    pub fn of(line: &Line) -> Option<Result<Self, SnapshotError>> {
        Some(Self::decode(line.behind(TAG)?))
    }

    fn decode(sections: &[String]) -> Result<Self, SnapshotError> {
        let frames: Vec<Frame> = nested(sections, "frames")?
            .into_iter()
            .map(|row| match row.as_slice() {
                [funcname, source, lineno] => Ok(Frame {
                    funcname: funcname.clone(),
                    source: source.clone(),
                    lineno: lineno
                        .parse()
                        .map_err(|_| SnapshotError(format!("frame line {lineno:?}")))?,
                }),
                other => Err(SnapshotError(format!("a frame is three words, got {other:?}"))),
            })
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
}
