//! The programs' own surface: what `bashcap` and `bashprof` do with a command
//! line.
//!
//! Spawning the built binary is the only way to cover argv parsing, the exit
//! code it hands back, where the reading is written, and one subcommand
//! reading what another wrote. What the tools *find* is tested where they are
//! — `src/bashcap/tests/`, `src/bashprof/tests/`.
//!
//! `cargo test --test cli`

use std::process::Command;

#[path = "support/mod.rs"]
#[allow(dead_code)]
mod support;

use support::Scripts;

const BASHCAP: &str = env!("CARGO_BIN_EXE_bashcap");
const BASHPROF: &str = env!("CARGO_BIN_EXE_bashprof");

/// `--trace-calls` asks the subject's shells to record what each call was
/// passed, and the subject is none the wiser: its own status comes back
/// unchanged, and it never runs `shopt` itself.
#[test]
fn trace_calls_reaches_the_subject_and_the_status_comes_back() {
    // Line 1 is where `BASHCAP` fires, line 3 where `step` was called.
    let scripts = Scripts::of(&[(
        "build.bash",
        r#"step() { BASHCAP -BCS:"one step"; }

        step 'a target' --flag
        exit 7
        "#,
    )]);
    let into = scripts.at("capture.jsonl");

    let ran = Command::new(BASHCAP)
        .args(["run", "--into"])
        .arg(&into)
        .args(["--trace-calls", "--", "bash"])
        .arg(scripts.at("build.bash"))
        .output()
        .expect("the built bashcap");

    assert_eq!(ran.status.code(), Some(7), "the subject's own code, not the wrapper's");

    let shown = Command::new(BASHCAP).arg("show").arg(&into).output().expect("bashcap show");
    let text = String::from_utf8(shown.stdout).unwrap();

    assert!(text.contains("1 snapshots from 1 shells"), "{text}");
    assert!(
        text.contains("step@build.bash:1 ('a target' '--flag')")
            && text.contains("main@build.bash:3 ()"),
        "each frame carries its own call site and the arguments it was passed: {text}"
    );
    assert!(text.contains("note  one step"), "{text}");
}

/// Without it, a frame says its arguments were never recorded rather than
/// claiming it was called with none.
#[test]
fn without_the_switch_nothing_is_traced() {
    let scripts = Scripts::of(&[(
        "build.bash",
        r#"step() { BASHCAP; }

        step 'a target'
        "#,
    )]);
    let into = scripts.at("capture.jsonl");

    let ran = Command::new(BASHCAP)
        .args(["run", "--into"])
        .arg(&into)
        .args(["--", "bash"])
        .arg(scripts.at("build.bash"))
        .output()
        .expect("the built bashcap");
    assert_eq!(ran.status.code(), Some(0));

    let shown = Command::new(BASHCAP).arg("show").arg(&into).output().expect("bashcap show");
    let text = String::from_utf8(shown.stdout).unwrap();

    assert!(text.contains("step@build.bash:1\n"), "the call site alone: {text}");
    assert!(!text.contains("a target"), "no arguments were recorded to report: {text}");
}

/// One run, and what it wrote where it was told to.
struct Ran {
    status: Option<i32>,
    stdout: String,
    stderr: String,
    into: String,
}

fn bashprof(scripts: &Scripts, output: &[&str]) -> Ran {
    let into = scripts.at("reading.json");
    let ran = Command::new(BASHPROF)
        .arg("--into")
        .arg(&into)
        .args(output)
        .args(["--", "bash"])
        .arg(scripts.at("build.bash"))
        .output()
        .expect("the built bashprof");

    Ran {
        status: ran.status.code(),
        stdout: String::from_utf8(ran.stdout).unwrap(),
        stderr: String::from_utf8(ran.stderr).unwrap(),
        into: std::fs::read_to_string(&into).expect("the file bashprof was pointed at"),
    }
}

/// A run the shell dies inside: one call measured, one that never ended.
fn dying() -> Scripts {
    Scripts::of(&[(
        "build.bash",
        r#"set -e
        step()   { echo "the subject's own stdout"; }
        broken() { false; }

        BASHPROF_TIME_CPS ok step
        BASHPROF_TIME_CPS doomed broken
        "#,
    )])
}

/// Every entry of the default reading claims a duration, and a call the shell
/// died inside has none to give. So the file is left empty rather than holding
/// a tree that is quietly missing time, and the calls that prevented one are
/// named on stderr.
#[test]
fn a_run_that_died_mid_call_refuses_to_report_measurements() {
    let ran = bashprof(&dying(), &[]);

    assert_eq!(ran.status, Some(1), "the subject's own code, not the wrapper's");
    assert_eq!(ran.into, "", "no tree claiming time it lacks");
    assert!(ran.stderr.contains(r#"calls that never ended: ["doomed"]"#), "{}", ran.stderr);
}

/// Both streams are the subject's alone. Nothing of bashprof's reaches them,
/// so a profiled run is pipeable exactly as an unprofiled one is.
#[test]
fn the_subject_owns_both_streams() {
    let ran = bashprof(&dying(), &["--output=tree-with-err"]);

    assert_eq!(ran.stdout, "the subject's own stdout\n");
    assert_eq!(ran.stderr, "", "and nothing was wrong to report");
}

/// The same run as recorded. Whether a call ended is the node's tag, and an
/// unended one carries no end rather than an empty one.
#[test]
fn the_recorded_reading_keeps_the_call_that_never_ended() {
    let ran = bashprof(&dying(), &["--output=tree-with-err"]);
    assert_eq!(ran.status, Some(1), "still the subject's own code");

    let tree: serde_json::Value = serde_json::from_str(&ran.into).expect("a JSON tree");
    let states: Vec<(String, String)> = tree
        .as_array()
        .expect("an array of roots")
        .iter()
        .map(|node| {
            let (state, body) = node["record"].as_object().unwrap().iter().next().unwrap();
            let call = body.get("call").unwrap_or(body);

            (call["label"].as_str().unwrap().to_string(), state.clone())
        })
        .collect();

    assert_eq!(
        states,
        [("ok".to_string(), "ended".to_string()), ("doomed".to_string(), "unended".to_string())],
        "{tree:#}"
    );
    assert_eq!(tree[0]["record"]["ended"]["status"], 0, "what the measured command returned");
    assert!(tree[1]["record"]["unended"].get("ended_at").is_none(), "no END, so no end");
}

/// A run holding a message the instrument wrote and cannot read back, beside
/// two calls it can. Only a shell saying it directly can stage this: the word
/// a client writes cannot produce one.
fn mangled() -> Scripts {
    Scripts::of(&[(
        "build.bash",
        r#"BASHPROF_TIME_CPS before true
        BC_INSTR say TIME_CPS BEGIN id 1.99 inside "" label mangled
        BASHPROF_TIME_CPS after true
        "#,
    )])
}

/// The two readings answer that differently. Measurements refuse, every entry
/// of them claiming a duration. What the run recorded is written anyway, with
/// a word on stderr, because it is what the run said.
#[test]
fn a_message_that_will_not_read_refuses_a_profile_and_not_a_record() {
    let complaint = r#"a BEGIN with no "argv""#;

    let refused = bashprof(&mangled(), &[]);
    assert_eq!(refused.status, Some(1), "bashprof's own code: the subject was fine");
    assert_eq!(refused.into, "", "no tree claiming time it cannot account for");
    assert!(refused.stderr.contains(complaint), "{}", refused.stderr);

    let kept = bashprof(&mangled(), &["--output=tree-with-err"]);
    assert_eq!(kept.status, Some(0), "the subject's own code");
    assert!(kept.stderr.contains(complaint), "{}", kept.stderr);

    let tree: serde_json::Value = serde_json::from_str(&kept.into).expect("a JSON tree");
    let labels: Vec<&str> = tree
        .as_array()
        .expect("an array of roots")
        .iter()
        .map(|node| node["record"]["ended"]["call"]["label"].as_str().expect("a label"))
        .collect();

    assert_eq!(labels, ["before", "after"], "the calls around it are no less true: {tree:#}");
}

/// Before any of it is read as calls: one JSON object per line, each the
/// arglist one shell sent with the provenance the protocol put in front. The
/// END that never came is the whole difference from the recorded tree.
#[test]
fn the_raw_reading_is_one_message_per_line() {
    let ran = bashprof(&dying(), &["--output=raw"]);

    let heard: Vec<serde_json::Value> = ran
        .into
        .lines()
        .map(|line| serde_json::from_str(line).expect("one message per line"))
        .collect();

    // The shell opens with its own account of itself, then the instrument's
    // messages. Both are on the wire, and this output is the wire.
    let kinds: Vec<&str> = heard.iter().map(|line| line["kind"].as_str().unwrap()).collect();
    assert_eq!(kinds, ["JOIN", "SAY", "SAY", "SAY"], "{}", ran.into);

    let instrument = &heard[1..];
    let said: Vec<&str> =
        instrument.iter().map(|line| line["words"][1].as_str().unwrap()).collect();

    assert_eq!(said, ["BEGIN", "END", "BEGIN"], "{}", ran.into);
    assert!(
        instrument.iter().all(|line| line["words"][0] == "TIME_CPS"),
        "{}",
        ran.into
    );
}
