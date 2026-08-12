//! The programs' own surface: what `bashcap` and `bashprof` do with a command
//! line.
//!
//! Spawning the built binary is the only way to cover argv parsing, the exit
//! code it hands back, and one subcommand reading what another wrote.
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

/// A run that died inside a measured call still measured what completed, and
/// the two halves of `Profile::of`'s result go to the two streams: the
/// measurements to stdout, what prevented a whole profile to stderr. The
/// subject's own status comes back either way.
#[test]
fn a_run_that_died_mid_call_reports_both_halves() {
    let scripts = Scripts::of(&[(
        "build.bash",
        r#"set -e
        step()   { :; }
        broken() { false; }

        BASHPROF_TIME_CPS ok step
        BASHPROF_TIME_CPS doomed broken
        "#,
    )]);

    let ran = Command::new(BASHPROF)
        .args(["run", "--", "bash"])
        .arg(scripts.at("build.bash"))
        .output()
        .expect("the built bashprof");

    assert_eq!(ran.status.code(), Some(1), "the subject's own code, not the wrapper's");

    let measured = String::from_utf8(ran.stdout).unwrap();
    let why = String::from_utf8(ran.stderr).unwrap();

    assert!(measured.contains("ok ") && measured.contains("µs of its own"), "{measured}");
    assert!(!measured.contains("doomed"), "a call that never ended is no measurement: {measured}");
    assert!(why.contains(r#"calls that never ended: ["doomed"]"#), "{why}");
}

/// The stub makes an instrumented script safe to run without the tool: the
/// call sites stay where they are, and the calls they wrap still happen.
#[test]
fn the_polyfill_runs_an_instrumented_script_unprofiled() {
    let stub = Command::new(BASHPROF).arg("polyfill").output().expect("bashprof polyfill");
    let build = format!(
        "{}\nstep() {{ echo \"ran $1\"; }}\nBASHPROF_TIME_CPS build step target\n",
        String::from_utf8(stub.stdout).unwrap()
    );

    let scripts = Scripts::of(&[("build.bash", &build)]);
    let ran = Command::new("bash").arg(scripts.at("build.bash")).output().expect("bash");

    assert_eq!(String::from_utf8(ran.stdout).unwrap(), "ran target\n");
    assert_eq!(ran.status.code(), Some(0));
}
