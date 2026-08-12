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

    let said: Vec<&str> = heard.iter().map(|line| line["words"][1].as_str().unwrap()).collect();

    assert_eq!(said, ["BEGIN", "END", "BEGIN"], "{}", ran.into);
    assert!(
        heard.iter().all(|line| line["kind"] == "SAY" && line["words"][0] == "TIME_CPS"),
        "{}",
        ran.into
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
                 step() {{ echo \"ran $1\"; }}\n\
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

    assert_eq!(String::from_utf8(ran.stdout).unwrap(), "ran target\n");
    assert_eq!(ran.status.code(), Some(0));
}

/// The same script under the tool. `BASH_ENV` defines the real word before the
/// script's first line, so the guard must find it and leave it alone — a stub
/// that installed itself unconditionally would measure nothing, silently.
#[test]
fn the_guard_leaves_the_real_word_in_place_under_the_tool() {
    let ran = bashprof(&vendoring(), &["--output=tree"]);

    assert_eq!(ran.status, Some(0));
    assert_eq!(ran.stdout, "ran target\n", "the subject still ran, and still owns stdout");

    let tree: serde_json::Value = serde_json::from_str(&ran.into).expect("a JSON tree");
    let top = &tree["roots"][0]["complete"];

    assert_eq!(top["call"]["label"], "build", "the stub did not displace it: {tree:#}");
    assert_eq!(top["call"]["argv"], serde_json::json!(["step", "target"]), "{tree:#}");
    assert!(top["ended_at"].as_u64() > top["call"]["sent"]["sent_at"].as_u64(), "{tree:#}");
}

/// A stack is not the tree and cannot be read off it: an unmeasured function
/// between two measured calls is a frame and not a node. So every measurement
/// carries the walk its own shell took, and its first frame is the call site
/// of that measurement.
#[test]
fn a_measurement_carries_the_whole_stack_it_was_made_on() {
    // Line 2 is where `inner` is measured, 3 where `mid` is called, 5 where
    // `top` is measured.
    let scripts = Scripts::of(&[(
        "build.bash",
        r#"leaf()    { :; }
        mid()     { BASHPROF_TIME_CPS inner leaf; }
        between() { mid; }

        BASHPROF_TIME_CPS top between
        "#,
    )]);
    let ran = bashprof(&scripts, &["--output=tree"]);
    let tree: serde_json::Value = serde_json::from_str(&ran.into).expect("a JSON tree");

    // A frame is a named function, or one of bash's two words for a file's
    // own top level.
    fn site(frame: &serde_json::Value) -> String {
        match &frame["site"] {
            serde_json::Value::String(word) => word.clone(),
            named => named["function"].as_str().unwrap().to_string(),
        }
    }

    let named = |node: &serde_json::Value| -> Vec<String> {
        node["complete"]["call"]["stack"]
            .as_array()
            .expect("one array of frames")
            .iter()
            .map(site)
            .collect()
    };

    // The instrument's own frames are in the walk and are not the subject's,
    // so they are named but carry no line of this script.
    let sited = |node: &serde_json::Value| -> Vec<(String, u64)> {
        node["complete"]["call"]["stack"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|frame| {
                frame["source"]["file"].as_str().unwrap().ends_with("build.bash")
            })
            .map(|frame| (site(frame), frame["lineno"].as_u64().unwrap()))
            .collect()
    };

    let top = &tree["roots"][0];
    let inner = &top["children"][0];
    fn label(node: &serde_json::Value) -> &str {
        node["complete"]["call"]["label"].as_str().unwrap()
    }

    assert_eq!((label(top), label(inner)), ("top", "inner"));

    assert_eq!(sited(top), [("script".to_string(), 5)], "the outermost call site: {tree:#}");
    assert_eq!(
        named(inner),
        ["mid", "between", "BASHPROF_TIME_CPS", "script"],
        "`between` is on the stack and nowhere in the tree: {tree:#}"
    );
    assert_eq!(
        sited(inner),
        [("mid".to_string(), 2), ("between".to_string(), 3), ("script".to_string(), 5)],
        "each frame at the line it is executing, innermost first: {tree:#}"
    );
}

/// The default is the measured tree written for a reader: one call per line,
/// nested by indentation, with what each had to itself.
#[test]
fn the_default_reading_is_the_measured_tree_as_text() {
    let ran = bashprof(&vendoring(), &[]);

    assert_eq!(ran.status, Some(0));
    assert!(
        ran.into.starts_with("build ") && ran.into.contains("µs of its own"),
        "{}",
        ran.into
    );
    assert!(!ran.into.contains('{'), "text, not the JSON of the same reading: {}", ran.into);
}
