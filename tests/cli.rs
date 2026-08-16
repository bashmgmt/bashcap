//! The program's own surface: what `bashcap` does with a command line.
//!
//! Spawning the built binary is the only way to cover argv parsing, the exit
//! code it hands back, where the capture is written, and one subcommand
//! reading what another wrote. What the tool *finds* is tested in `src/tests`.
//!
//! `cargo test --test cli`

use std::process::Command;

use bash_interop::scratch::Scripts;

const BASHCAP: &str = env!("CARGO_BIN_EXE_bashcap");

/// `--trace-calls` asks the subject's shells to record what each call was
/// passed, and the subject is none the wiser: its own status comes back
/// unchanged, and it never runs `shopt` itself.
#[test]
fn trace_calls_reaches_the_subject_and_the_status_comes_back() {
    // Line 2 of the script is where `BASHCAP` fires, line 4 where `step` was called.
    let scripts = Scripts::of(&[(
        "build.bash",
        r#"
        step() { BASHCAP -BCS:"one step"; }

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
        text.contains("step@build.bash:2 ('a target' '--flag')")
            && text.contains("main@build.bash:4 ()"),
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
        r#"
        step() { BASHCAP; }

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

    assert!(text.contains("step@build.bash:2\n"), "the call site alone: {text}");
    assert!(!text.contains("a target"), "no arguments were recorded to report: {text}");
}

/// `--reach by-hand` provisions a definitions file and the workspace: every
/// shell has the words, and none is a shell of the run until it initiates —
/// the script where it says `BASHCAP_INIT "$BASHCAP_SESSION"`; a child that
/// never does is only ever complained at.
#[test]
fn reach_by_hand_leaves_joining_to_the_script() {
    let scripts = Scripts::of(&[(
        "build.bash",
        r#"
        declare -- workspace="${BASHCAP_SESSION:?the workspace, from the tool}"

        bash -c 'BASHCAP -BCS:"never joined" 2>/dev/null || true'
        BASHCAP_INIT "$workspace"
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
    assert!(!text.contains("never joined"), "the child had the words, not the channel: {text}");
}

/// Both session-opening verbs tell a script how to join, under `--help`.
#[test]
fn help_says_how_a_script_joins() {
    for verb in ["run", "serve"] {
        let help = Command::new(BASHCAP).args([verb, "--help"]).output().expect("--help");
        let text = String::from_utf8(help.stdout).unwrap();

        assert!(text.contains(r#"BASHCAP_INIT "$BASHCAP_SESSION""#), "{verb} --help:\n{text}");
        assert!(text.contains("coproc SERVER"), "{verb} --help:\n{text}");
        assert!(text.contains(r#"source "$workspace/prelude.bash""#), "{verb} --help:\n{text}");
    }
}

/// A served session end to end, over the shipped binary: a script starts
/// bashcap for itself, and `BASHCAP` is defined by the laid files.
#[test]
fn a_script_starts_bashcap_for_itself_and_keeps_the_capture() {
    let scripts = Scripts::of(&[(
        "work.bash",
        r#"
        set -euo pipefail
        declare -- workspace="${1:?the session workspace}"; shift
        mkdir -p "$workspace"
        coproc SERVER { "$@"; }
        until [[ -p "$workspace/join" ]]; do sleep 0.01; done
        source "$workspace/prelude.bash"
        source "$workspace/rig.bash"
        BASHCAP_INIT "$workspace"

        step() { BASHCAP -BCS:"in a served shell"; }
        step 'a target'
        ( BASHCAP -BCS:"from a subshell" )

        declare -- handle="${SERVER[1]}"
        exec {handle}>&-
        wait "$SERVER_PID"
        "#,
    )]);
    let into = scripts.at("capture.jsonl");

    let ran = Command::new("bash")
        .arg(scripts.at("work.bash"))
        .arg(scripts.at("session.d"))
        .args([BASHCAP, "serve", "--at"])
        .arg(scripts.at("session.d"))
        .args(["--verbose", "--trace-calls", "--into"])
        .arg(&into)
        .output()
        .expect("bash");

    let complaints = String::from_utf8(ran.stderr).unwrap();
    assert_eq!(ran.status.code(), Some(0), "{complaints}");
    assert!(complaints.contains("bashcap: 2 snapshots"), "the tally is on stderr: {complaints}");

    let shown = Command::new(BASHCAP).arg("show").arg(&into).output().expect("bashcap show");
    let text = String::from_utf8(shown.stdout).unwrap();

    assert!(text.contains("2 snapshots from 2 shells"), "the subshell is one of its own: {text}");
    assert!(text.contains("step@work.bash:11 ('a target')"), "--trace-calls reached it: {text}");
    assert!(text.contains("note  from a subshell"), "{text}");
}
