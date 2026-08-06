use std::path::{Path, PathBuf};

use super::{BashCap, Snapshot, Value, BASH, POLYFILL};
use crate::bash::rig::{run, Doing, ExitStatus, Failure, Line, Pid, Rig};

/// A script that vendors the polyfill, as a shipped one would.
fn script(temp: &Path, body: &str) -> PathBuf {
    let polyfill = temp.join("polyfill.bash");
    std::fs::write(&polyfill, POLYFILL).unwrap();

    let entry = temp.join("main.bash");
    std::fs::write(&entry, format!("source {}\n{body}", polyfill.display())).unwrap();
    entry
}

/// bashcap's bash, decoded but not written, so assertions read typed
/// snapshots rather than JSON. Every snapshot must decode.
struct Decoding;

impl Rig for Decoding {
    type Session = Vec<(Pid, Snapshot)>;

    fn bash(&self) -> String {
        BASH.to_string()
    }

    fn open(&self) -> Result<Self::Session, Failure> {
        Ok(Vec::new())
    }

    fn hear(&self, snaps: &mut Self::Session, said: Line) -> Result<(), Failure> {
        let Some(decoded) = Snapshot::of(&said) else { return Ok(()) };

        snaps.push((said.pid, decoded.doing(|| format!("a snapshot from pid {}", said.pid))?));

        Ok(())
    }
}

fn capture(body: &str) -> Vec<(Pid, Snapshot)> {
    let temp = tempfile::tempdir().unwrap();
    let (snaps, _) = run(&Decoding, &[script(temp.path(), body)]).unwrap();

    snaps
}

/// One run covering every section: frames with their call sites, typed
/// variable capture through `@A`, ambient context, regex state, annotations,
/// and the CPS wrapper running its continuation.
#[test]
fn a_snapshot_carries_the_whole_shell_state() {
    let snaps = capture(
        r#"
        declare -- greeting="hello world"
        declare -a items=(alpha "beta gamma")
        declare -A conf=([host]=localhost)
        declare -i tries=3
        BASHCAP__CTX__phase="setup"

        [[ "build-2026" =~ ^([a-z]+)-([0-9]+)$ ]]

        outer() { inner; }
        inner() {
            BASHCAP -BCV:greeting -BCV:items -BCV:conf -BCV:tries -BCV:absent -BCS:"deep"
        }
        outer

        step() { echo "ran $*"; }
        WITH_BASHCAP -BCV:greeting -BCS:"before step" step one
        "#,
    );

    assert_eq!(snaps.len(), 2);

    let deep = &snaps[0].1;
    let names: Vec<&str> = deep.frames.iter().map(|frame| frame.funcname.as_str()).collect();
    assert_eq!(names, ["inner", "outer", "main"]);
    assert!(deep.frames[0].source.ends_with("main.bash"));
    assert!(deep.frames[0].lineno > 0);

    assert_eq!(deep.vars["greeting"].value, Value::Scalar("hello world".into()));
    assert_eq!(deep.vars["items"].attrs, "a");
    assert_eq!(
        deep.vars["items"].value,
        Value::Indexed([(0, "alpha".to_string()), (1, "beta gamma".to_string())].into())
    );
    assert_eq!(deep.vars["conf"].attrs, "A");
    assert_eq!(deep.vars["tries"].attrs, "i", "attributes survive, unlike a hand-rolled capture");
    assert!(!deep.vars.contains_key("absent"), "a missing variable is skipped");
    assert_eq!(deep.vars["BASHCAP__CTX__phase"].value, Value::Scalar("setup".into()));

    assert_eq!(deep.rematch, ["build-2026", "build", "2026"]);
    assert_eq!(deep.notes, ["deep"]);
    assert!(deep.state.contains_key("shlvl") && deep.state.contains_key("flags"));

    let wrapped = &snaps[1].1;
    assert_eq!(wrapped.frames[0].funcname, "WITH_BASHCAP");
    assert_eq!(wrapped.notes, ["before step"]);
}

/// The written line carries the snapshot's own fields alongside bare
/// provenance, and the file is opened by the run rather than by the caller.
#[test]
fn each_snapshot_is_written_as_it_arrives() {
    let temp = tempfile::tempdir().unwrap();
    let into = temp.path().join("out.jsonl");
    let entry = script(temp.path(), "BASHCAP -BCS:one\nBASHCAP -BCS:two");

    let (capturing, status) = run(&BashCap::writing(&into), &[entry]).unwrap();
    assert_eq!(capturing.written, 2);
    assert_eq!(status, ExitStatus::Code(0));

    let rows: Vec<serde_json::Value> = std::fs::read_to_string(&into)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();

    assert_eq!(rows.len(), 2);
    for row in &rows {
        for key in ["sent_at", "heard_at", "pid", "seq", "frames", "state"] {
            assert!(row.get(key).is_some(), "{key} missing from {row}");
        }
        assert!(row["pid"].is_u64(), "provenance is bare, not nested");
    }
    assert_eq!(rows[0]["notes"][0], "one");
    assert_eq!(rows[1]["notes"][0], "two");
}

/// bashcap rides into every shell through the same prelude the protocol
/// uses, and its polyfill is sourced by the client's own script. Neither may
/// put a name in the environment, where it would reach every process the
/// subject starts.
#[test]
fn neither_half_exports_a_name() {
    for (whose, bash) in [("bashcap.bash", BASH), ("polyfill.bash", POLYFILL)] {
        for line in bash.lines().filter(|line| !line.trim_start().starts_with('#')) {
            assert!(!line.contains("export "), "{whose}: {line}");
        }
    }
}
