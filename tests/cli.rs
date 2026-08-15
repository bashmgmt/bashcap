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
    // Line 1 of the script is where `BASHCAP` fires, line 3 where `step` was called.
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

/// `--reach by-hand` exports the address and nothing else: the script joins
/// where it says `source "$BC_SESSION"`, and a shell it started before that is
/// not a shell of the run.
#[test]
fn reach_by_hand_leaves_joining_to_the_script() {
    let scripts = Scripts::of(&[(
        "build.bash",
        r#"bash -c 'type BASHCAP >/dev/null 2>&1 && echo "joined without asking" >&2'
        source "$BC_SESSION"
        BASHCAP -BCS:"by hand"
        "#,
    )]);
    let into = scripts.at("capture.jsonl");

    let ran = Command::new(BASHCAP)
        .args(["run", "--reach", "by-hand", "--into"])
        .arg(&into)
        .args(["--", "bash"])
        .arg(scripts.at("build.bash"))
        .output()
        .expect("the built bashcap");
    assert_eq!(ran.status.code(), Some(0));
    assert!(ran.stderr.is_empty(), "{}", String::from_utf8_lossy(&ran.stderr));

    let shown = Command::new(BASHCAP).arg("show").arg(&into).output().expect("bashcap show");
    let text = String::from_utf8(shown.stdout).unwrap();

    assert!(text.contains("1 snapshots from 1 shells"), "{text}");
    assert!(text.contains("note  by hand"), "{text}");
}

/// Both binaries tell a script how to join, under `--help` of the two verbs
/// that open a session.
#[test]
fn help_says_how_a_script_joins() {
    for (binary, verb) in [(BASHCAP, "run"), (BASHCAP, "serve"), (BASHPROF, "run"), (BASHPROF, "serve")] {
        let help = Command::new(binary).args([verb, "--help"]).output().expect("--help");
        let text = String::from_utf8(help.stdout).unwrap();

        assert!(text.contains(r#"source "$BC_SESSION""#), "{binary} {verb} --help:\n{text}");
        assert!(text.contains("BC_START"), "{binary} {verb} --help:\n{text}");
    }
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
        .args(["run", "--into"])
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

        BASHPROF_TIMETHIS ok step
        BASHPROF_TIMETHIS doomed broken
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
        r#"BASHPROF_TIMETHIS before true
        BC_INSTR BASHPROF say TIMETHIS BEGIN id 1.99 inside "" label mangled
        BASHPROF_TIMETHIS after true
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
/// arglist one shell sent beside the shell that sent it. The END that never
/// came is the whole difference from the recorded tree.
#[test]
fn the_raw_reading_is_one_message_per_line() {
    let ran = bashprof(&dying(), &["--output=raw"]);

    let heard: Vec<serde_json::Value> = ran
        .into
        .lines()
        .map(|line| serde_json::from_str(line).expect("one message per line"))
        .collect();

    // A shell's account of itself is not among these: it is what makes the
    // shell, and what every line then carries whole.
    let kinds: Vec<&str> =
        heard.iter().map(|said| said["message"]["verb"].as_str().unwrap()).collect();
    assert_eq!(kinds, ["SAY", "SAY", "SAY"], "{}", ran.into);
    assert!(
        heard.iter().all(|said| said["shell"]["pid"].is_u64()),
        "and says which shell: {}",
        ran.into
    );

    let said: Vec<&str> =
        heard.iter().map(|said| said["message"]["words"][1].as_str().unwrap()).collect();

    assert_eq!(said, ["BEGIN", "END", "BEGIN"], "{}", ran.into);
    assert!(
        heard.iter().all(|said| said["message"]["words"][0] == "TIMETHIS"),
        "{}",
        ran.into
    );
}

// ── the other role: a bash script starts the tool ────────────────────

/// A file under `__fixtures/`, by path from the crate root.
fn fixture(relative: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("__fixtures").join(relative)
}

/// `assets/joining.bash`, where a client would have vendored it. Sourcing it
/// from where it lives is what keeps the test and the shipped file the same
/// bytes.
fn joining() -> String {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("assets/joining.bash")
        .to_string_lossy()
        .into_owned()
}

/// The vendoring contract end to end, over the shipped binary: the same script
/// builds on its own with the empty hooks, and measures itself when it is
/// given a server to start. Nothing about the script changes between the two.
#[test]
fn a_script_starts_bashprof_for_itself_and_keeps_the_reading() {
    let build = fixture("joined/build.bash");
    let scripts = Scripts::of(&[]);
    let into = scripts.at("build.times");

    let alone = Command::new("bash").arg(&build).output().expect("bash");
    assert_eq!(String::from_utf8(alone.stdout).unwrap(), "built\n", "it runs on its own");
    assert_eq!(alone.status.code(), Some(0));

    let joined = Command::new("bash")
        .arg(&build)
        .args([BASHPROF, "serve", "--into"])
        .arg(&into)
        .output()
        .expect("bash");

    assert_eq!(String::from_utf8(joined.stdout).unwrap(), "built\n", "and the same output");
    assert_eq!(joined.status.code(), Some(0), "{}", String::from_utf8_lossy(&joined.stderr));

    // The script waited for the server it started, so the reading is on disk by
    // the time it exits. Two spaces of indent per level, label first.
    let reading = std::fs::read_to_string(&into).expect("the reading the server wrote");
    let shape: Vec<(usize, &str)> = reading
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let depth = (line.len() - line.trim_start().len()) / 2;

            (depth, line.trim_start().split(' ').next().expect("a label"))
        })
        .collect();

    assert_eq!(
        shape,
        [(0, "build"), (1, "compile"), (2, "link"), (1, "package")],
        "the tree the calls made, indented by how deep they nested:\n{reading}"
    );
}

/// The same for bashcap, and the same script shape: join, say things, let go.
/// `BASHCAP` is defined by joining — a client that only ever runs under a
/// server vendors nothing at all.
#[test]
fn a_script_starts_bashcap_for_itself_and_keeps_the_capture() {
    let scripts = Scripts::of(&[(
        "work.bash",
        &format!(
            r#"set -euo pipefail
            source {:?}
            BC_START "$@"

            step() {{ BASHCAP -BCS:"in a served shell"; }}
            step 'a target'
            ( BASHCAP -BCS:"from a subshell" )

            BC_LEAVE
            "#,
            joining()
        ),
    )]);
    let into = scripts.at("capture.jsonl");

    let ran = Command::new("bash")
        .arg(scripts.at("work.bash"))
        .args([BASHCAP, "serve", "--verbose", "--trace-calls", "--into"])
        .arg(&into)
        .output()
        .expect("bash");

    let complaints = String::from_utf8(ran.stderr).unwrap();
    assert_eq!(ran.status.code(), Some(0), "{complaints}");
    assert!(complaints.contains("bashcap: 2 snapshots"), "the tally is on stderr: {complaints}");

    let shown = Command::new(BASHCAP).arg("show").arg(&into).output().expect("bashcap show");
    let text = String::from_utf8(shown.stdout).unwrap();

    assert!(text.contains("2 snapshots from 2 shells"), "the subshell is one of its own: {text}");
    assert!(text.contains("step@work.bash:5 ('a target')"), "--trace-calls reached it: {text}");
    assert!(text.contains("note  from a subshell"), "{text}");
}
