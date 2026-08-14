//! What one `BASHCAP` call reports: the frames it was made on, the
//! variables it named, and what a value survives on the way here.

use crate::bashcap::Value;

use super::capture;

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
    let names: Vec<String> = deep.stack.frames().map(|frame| frame.site.to_string()).collect();
    assert_eq!(names, ["inner", "outer", "main"]);
    assert_eq!(deep.stack.at().source.to_string(), "main.bash", "the file, absolute underneath");
    assert!(deep.stack.at().lineno > 0);

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

    // Three homes and no overlap: what only this moment can say, what every
    // message carries already, and what the shell said once when it joined.
    assert!(deep.state.contains_key("seconds"));
    assert!(snaps[0].sent.shlvl > 0);
    assert!(snaps[0].shell.version.at_least(5, 0, 0), "$EPOCHREALTIME is bash 5");
    assert!(snaps[0].shell.started.from_a_file(), "a script bash was handed to read");
    assert!(!snaps[0].shell.started.interactive);

    let wrapped = &snaps[1].snapshot;
    assert_eq!(wrapped.stack.at().site.to_string(), "WITH_BASHCAP");
    assert_eq!(wrapped.notes, ["before step"]);
}

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
    assert_eq!(snaps[0].snapshot.stack.at().args, Some(Vec::new()), "f was called with none");
    assert_eq!(snaps[1].snapshot.notes, ["after"]);
}
