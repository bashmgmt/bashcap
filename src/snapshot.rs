//! The record one shell sends back, and the decoder that reads one off the
//! wire.

use std::sync::Arc;

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use crate::bash::rig::{field, Doing, Failure, Message, Shell, Stamp};
use crate::bash::stack::{Columns, Stack};
use crate::bash::value::{parse_array, parse_assoc, parse_indexed, parse_scalar};

/// The word every snapshot message begins with, and the one thing that tells
/// bashcap's messages from any other tool's on the same wire.
const TAG: &str = "__BASHCAP__";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Variable {
    pub attrs: String,
    pub value: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub stack: Stack,
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub state: IndexMap<String, String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rematch: Vec<String>,
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub vars: IndexMap<String, Variable>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

/// One snapshot, the shell that took it, and when. This is bashcap's output
/// format — one per message — and what `bashcap show` reads back.
///
/// The shell rides on every record rather than once per run: a message of JSONL
/// that has to be read against something else is not one, and what a walk means
/// depends on which shell took it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Capture {
    pub shell: Arc<Shell>,
    pub stamp: Stamp,
    pub snapshot: Snapshot,
}

impl Capture {
    /// `None` for a message that is not one of ours; `Some(Err)` for one that is
    /// and will not decode. Several tools may share the wire, and only the
    /// second of those is anyone's failure.
    pub fn of(message: &Message, shell: &Arc<Shell>) -> Option<Result<Self, Failure>> {
        let sections = message.behind(TAG)?;

        Some(Snapshot::decode(sections, shell).map(|snapshot| Self {
            shell: Arc::clone(shell),
            stamp: message.stamp,
            snapshot,
        }))
    }
}

impl Snapshot {
    fn decode(sections: &[String], shell: &Shell) -> Result<Self, Failure> {
        let stack = Columns::of(sections)?.frames(shell)?;

        let state = flat(sections, "state")?
            .chunks_exact(2)
            .map(|pair| (pair[0].clone(), pair[1].clone()))
            .collect();

        let vars = flat(sections, "vars")?
            .iter()
            .map(|declaration| variable(declaration))
            .collect::<Result<IndexMap<_, _>, _>>()?;

        Ok(Self {
            stack,
            state,
            rematch: flat(sections, "rematch")?,
            vars,
            notes: flat(sections, "notes")?,
        })
    }
}

// ── the sections a message carries ───────────────────────────────────

fn reading(key: &str, cause: impl Into<Box<dyn std::error::Error + Send + Sync>>) -> Failure {
    Failure::new(format!("reading the {key:?} section"), cause)
}

/// One section, as the array literal it is written as.
fn flat(sections: &[String], key: &str) -> Result<Vec<String>, Failure> {
    let text = field(sections, key).ok_or_else(|| reading(key, "it is missing"))?;

    parse_array(text).map_err(|cause| reading(key, cause))
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
fn variable(text: &str) -> Result<(String, Variable), Failure> {
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

    Ok((name.to_string(), Variable { attrs, value }))
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
            let (got_name, got) = variable(text).unwrap();
            assert_eq!(got_name, name);
            assert_eq!(got.attrs, attrs, "{text}");
            assert_eq!(got.value, value, "{text}");
        }
    }

}
