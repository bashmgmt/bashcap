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

fn bashprof(scripts: &Scripts, output: &[&str]) -> std::process::Output {
    Command::new(BASHPROF)
        .args(output)
        .args(["--", "bash"])
        .arg(scripts.at("build.bash"))
        .output()
        .expect("the built bashprof")
}

/// A run the shell dies inside: one call measured, one that never ended.
fn dying() -> Scripts {
    Scripts::of(&[(
        "build.bash",
        r#"set -e
        step()   { :; }
        broken() { false; }

        BASHPROF_TIME_CPS ok step
        BASHPROF_TIME_CPS doomed broken
        "#,
    )])
}

/// Every entry of the default output claims a duration, and a call the shell
/// died inside has none to give. So nothing goes to stdout, the calls that
/// prevented it go to stderr, and the subject's own status comes back.
#[test]
fn a_run_that_died_mid_call_refuses_to_report_measurements() {
    let ran = bashprof(&dying(), &[]);

    assert_eq!(ran.status.code(), Some(1), "the subject's own code, not the wrapper's");
    assert_eq!(String::from_utf8(ran.stdout).unwrap().trim(), "", "no tree claiming time it lacks");
    assert!(
        String::from_utf8(ran.stderr).unwrap().contains(r#"calls that never ended: ["doomed"]"#),
        "the diagnosis names them"
    );
}

/// The same run as recorded, where an end is a fact that may be absent rather
/// than one the output depends on. Every call that began is there.
#[test]
fn the_recorded_output_keeps_the_call_that_never_ended() {
    let ran = bashprof(&dying(), &["--output=tree-with-err"]);
    assert_eq!(ran.status.code(), Some(1), "still the subject's own code");

    let tree: serde_json::Value = serde_json::from_slice(&ran.stdout).expect("a JSON tree");
    let ends: Vec<(&str, bool)> = tree
        .as_array()
        .expect("an array of roots")
        .iter()
        .map(|node| (node["label"].as_str().unwrap(), node["ended"].is_null()))
        .collect();

    assert_eq!(ends, [("ok", false), ("doomed", true)], "{tree:#}");
}

/// Before any of it is read as calls: one JSON object per line, each the
/// arglist one shell sent with the provenance the protocol put in front. The
/// END that never came is the whole difference from the recorded tree.
#[test]
fn the_raw_output_is_one_message_per_line() {
    let ran = bashprof(&dying(), &["--output=raw"]);
    let text = String::from_utf8(ran.stdout).unwrap();

    let heard: Vec<serde_json::Value> =
        text.lines().map(|line| serde_json::from_str(line).expect("one message per line")).collect();

    let said: Vec<String> =
        heard.iter().map(|line| line["words"][1].as_str().unwrap().to_string()).collect();

    assert_eq!(said, ["BEGIN", "END", "BEGIN"], "{text}");
    assert!(
        heard.iter().all(|line| line["kind"] == "SAY" && line["words"][0] == "TIME_CPS"),
        "{text}"
    );
}

/// The stub a client vendors, and the guard that decides whether it is
/// installed. bashprof ships it as an asset, so the test reads the same file a
/// client would copy.
const POLYFILL: &str = include_str!("../assets/bashprof_polyfill.bash");

const GUARD: &str = "declare -F BASHPROF_TIME_CPS >/dev/null || __define_bashprof_polyfill";

/// A script whose call sites are guarded the way a shipped one's would be.
fn vendoring() -> Scripts {
    Scripts::of(&[
        ("polyfill.bash", POLYFILL),
        (
            "build.bash",
            &format!(
                "source \"$(dirname \"${{BASH_SOURCE[0]}}\")/polyfill.bash\"\n{GUARD}\n\
                 step() {{ echo \"ran $1\" >&2; }}\n\
                 BASHPROF_TIME_CPS build step target\n"
            ),
        ),
    ])
}

/// Without the tool the guard installs the stub, so the call sites stay where
/// they are and the calls they wrap still happen.
#[test]
fn the_vendored_stub_runs_an_instrumented_script_unprofiled() {
    let scripts = vendoring();
    let ran = Command::new("bash").arg(scripts.at("build.bash")).output().expect("bash");

    assert_eq!(String::from_utf8(ran.stderr).unwrap(), "ran target\n");
    assert_eq!(ran.status.code(), Some(0));
}

/// The same script under the tool. `BASH_ENV` defines the real word before the
/// script's first line, so the guard must find it and leave it alone — a stub
/// that installed itself unconditionally would measure nothing, silently.
#[test]
fn the_guard_leaves_the_real_word_in_place_under_the_tool() {
    let ran = bashprof(&vendoring(), &[]);
    let text = String::from_utf8(ran.stdout).unwrap();

    assert_eq!(ran.status.code(), Some(0));
    assert!(String::from_utf8(ran.stderr).unwrap().contains("ran target"), "the subject still ran");

    let tree: serde_json::Value = serde_json::from_str(&text).expect("a JSON tree");

    assert_eq!(tree[0]["label"], "build", "the stub did not displace the real word: {tree:#}");
    assert!(tree[0]["inclusive"].as_u64().is_some(), "{tree:#}");
}
