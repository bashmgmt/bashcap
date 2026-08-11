//! bashcap's bash-level tests: each spawns a real shell, which is the only
//! way to cover what the instrument harvests from one. What decodes without a
//! shell is tested beside the decoder.

use std::path::{Path, PathBuf};

use super::instrument::{BASH, TRACE};
use super::{captures, instrument, BashCap, Capture, Tracing, Value, POLYFILL};
use crate::bash::rig::{run, Doing, ExitStatus, Failure, Line, Rig, Startup};

/// A script that vendors the polyfill, as a shipped one would.
fn script(temp: &Path, body: &str) -> PathBuf {
    let polyfill = temp.join("polyfill.bash");
    std::fs::write(&polyfill, POLYFILL).unwrap();

    let entry = temp.join("main.bash");
    std::fs::write(&entry, format!("source {}\n{body}", polyfill.display())).unwrap();
    entry
}

/// bashcap's bash, decoded but not written, so assertions read typed
/// captures rather than JSON. Every snapshot must decode.
struct Decoding;

impl Rig for Decoding {
    type Session = Vec<Capture>;

    fn startup(&self) -> Startup {
        Startup { bash: instrument(Tracing::Off), ..Default::default() }
    }

    fn open(&self) -> Result<Self::Session, Failure> {
        Ok(Vec::new())
    }

    fn hear(&self, seen: &mut Self::Session, said: Line) -> Result<(), Failure> {
        let Some(decoded) = Capture::of(&said) else { return Ok(()) };

        seen.push(decoded.doing(|| format!("a snapshot from pid {}", said.pid))?);

        Ok(())
    }
}

fn capture(body: &str) -> Vec<Capture> {
    let temp = tempfile::tempdir().unwrap();
    let ran = run(&Decoding, &["bash".into(), script(temp.path(), body).into_os_string()]).unwrap();

    ran.whole().unwrap().0
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

    let deep = &snaps[0].snapshot;
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

    let wrapped = &snaps[1].snapshot;
    assert_eq!(wrapped.frames[0].funcname, "WITH_BASHCAP");
    assert_eq!(wrapped.notes, ["before step"]);
}

/// The written line carries the snapshot's own fields alongside bare
/// provenance, and the file is opened by the run rather than by the caller.
#[test]
fn each_snapshot_is_written_as_it_arrives() {
    let temp = tempfile::tempdir().unwrap();
    let into = temp.path().join("out.jsonl");
    let entry = script(
        temp.path(),
        r#"
        BASHCAP -BCS:one
        BASHCAP -BCS:two
        "#,
    );

    let (capturing, status) =
        run(&BashCap::writing(&into), &["bash".into(), entry.into_os_string()])
            .unwrap()
            .whole()
            .unwrap();
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

/// Every piece rides into a shell — two through the prelude, the polyfill
/// through the client's own script. None may put a name in the environment,
/// where it would reach every process the subject starts.
#[test]
fn no_shipped_bash_exports_a_name() {
    let shipped = [("bashcap.bash", BASH), ("trace.bash", TRACE), ("polyfill.bash", POLYFILL)];

    for (whose, bash) in shipped {
        for line in bash.lines().filter(|line| !line.trim_start().starts_with('#')) {
            assert!(!line.contains("export "), "{whose}: {line}");
        }
    }
}

/// A subject that traces its own calls gets each frame's arguments, and one
/// that does not gets `None` rather than an empty list. Neither was asked to
/// trace, so nothing here turns `extdebug` on but the subject itself.
#[test]
fn call_arguments_arrive_where_the_shell_was_recording_them() {
    let body = r#"
        deep() { BASHCAP -BCS:"at the bottom"; }
        mid()  { deep "d one" $'d\ntwo'; }
        mid "m one"
        "#;

    let bare = capture(body);
    assert!(
        bare[0].snapshot.frames.iter().all(|frame| frame.args.is_none()),
        "an ordinary shell records none, and says so"
    );

    let traced = capture(&format!("shopt -s extdebug\n{body}"));
    let called: Vec<&[String]> = traced[0]
        .snapshot
        .frames
        .iter()
        .map(|frame| frame.args.as_deref().expect("the shell was recording"))
        .collect();

    // `BASHCAP`'s own frame is not reported, but its flags still sit in the
    // flat `BASH_ARGV` stack; miscounting them would shift every frame.
    assert_eq!(called, [["d one", "d\ntwo"].as_slice(), ["m one"].as_slice(), [].as_slice()]);

    let shown = traced[0].snapshot.frames[0].to_string();
    assert!(
        shown.ends_with(" ('d one' $'d\\ntwo')"),
        "rendered as the bash that would pass them, newline and all: {shown}"
    );
}

/// What `run` writes, `show` reads: the rendering a library caller gets is
/// the one the command line prints.
#[test]
fn a_written_capture_reads_back_whole() {
    let temp = tempfile::tempdir().unwrap();
    let into = temp.path().join("out.jsonl");
    let entry = script(
        temp.path(),
        r#"
        shopt -s extdebug
        f() { BASHCAP -BCS:one; }
        f arg
        "#,
    );

    run(&BashCap::writing(&into), &["bash".into(), entry.into_os_string()])
        .unwrap()
        .whole()
        .unwrap();

    let read = captures(&std::fs::read_to_string(&into).unwrap()).unwrap();

    assert_eq!(read.len(), 1);
    assert_eq!(read[0].snapshot.frames[0].args.as_deref(), Some(["arg".to_string()].as_slice()));
    assert!(read[0].to_string().contains("f@main.bash"), "{}", read[0]);
}

/// A byte bash cannot show travels as an octal escape, not as itself, so the
/// frame stays valid UTF-8 and the value arrives whole. `\377` is the widest
/// escape bash prints — three octal digits reach 511, which is where reading
/// one back as a byte gives up.
#[test]
fn a_variable_holding_a_byte_bash_cannot_show_survives_the_wire() {
    let snaps = capture(
        r#"
        high=$'\377'
        low=$'a\001b'
        BASHCAP -BCV:high -BCV:low
        "#,
    );

    let vars = &snaps[0].snapshot.vars;
    assert_eq!(vars["high"].value, Value::Scalar("\u{ff}".into()));
    assert_eq!(vars["low"].value, Value::Scalar("a\u{1}b".into()));
}

/// The frame walk runs inside the subject's shell, under whatever options it
/// set. A bash arithmetic *command* succeeds only for a non-zero value, so a
/// running total that reached zero would end an `errexit` script part-way
/// through a snapshot — which is why the cursor moves by assignment. Reaching
/// zero needs nothing to count: no flags on `BASHCAP`, and a frame called
/// with no arguments.
#[test]
fn the_walk_survives_the_subjects_own_shell_options() {
    let snaps = capture(
        r#"
        set -euo pipefail
        shopt -s extdebug
        f() { BASHCAP; }
        f
        BASHCAP -BCS:after
        "#,
    );

    assert_eq!(snaps.len(), 2, "the script ran on past the first snapshot");
    assert_eq!(snaps[0].snapshot.frames[0].args, Some(Vec::new()), "f was called with none");
    assert_eq!(snaps[1].snapshot.notes, ["after"]);
}

/// The tool's own switch, over a subject that traces nothing: the same script
/// yields no arguments by default and every frame's by request, in the child
/// process as well as the top-level shell. `BASH_ENV` is what reaches that
/// child; a command line would have reached only the first shell.
#[test]
fn the_tools_switch_traces_a_subject_that_never_asked_for_it() {
    let temp = tempfile::tempdir().unwrap();
    let child = temp.path().join("child.bash");
    std::fs::write(
        &child,
        r#"
        deep() { BASHCAP -BCS:child; }
        deep 'in a child'
        "#,
    )
    .unwrap();

    let entry = script(
        temp.path(),
        &format!(
            r#"
            outer() {{ BASHCAP -BCS:top; }}
            outer 'at the top'
            bash {}
            "#,
            child.display()
        ),
    );

    let ran = |tool: BashCap, into: &Path| {
        run(&tool, &["bash".into(), entry.clone().into_os_string()]).unwrap().whole().unwrap();

        captures(&std::fs::read_to_string(into).unwrap()).unwrap()
    };

    let plain = temp.path().join("plain.jsonl");
    let bare = ran(BashCap::writing(&plain), &plain);
    assert_eq!(bare.len(), 2, "one snapshot from each shell");
    assert!(
        bare.iter().flat_map(|seen| &seen.snapshot.frames).all(|frame| frame.args.is_none()),
        "nothing records call arguments unless the tool is asked to"
    );

    let full = temp.path().join("traced.jsonl");
    let traced = ran(BashCap::writing(&full).tracing_calls(), &full);
    let called: Vec<Vec<&[String]>> = traced
        .iter()
        .map(|seen| {
            seen.snapshot
                .frames
                .iter()
                .map(|frame| frame.args.as_deref().expect("asked for, so recorded"))
                .collect()
        })
        .collect();

    assert_eq!(
        called,
        [
            [["at the top"].as_slice(), [].as_slice()],
            [["in a child"].as_slice(), [].as_slice()],
        ],
        "the switch reached the child process too"
    );
    assert_ne!(traced[0].pid, traced[1].pid, "two shells, not one");
}
