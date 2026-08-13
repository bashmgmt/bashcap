//! What a client ships: the stub that stands in for the words, and the
//! contract the instrument keeps towards a shell it reaches.

use crate::bash::stack;
use crate::bashcap::instrument::{BASH, TRACE};

use super::{script, POLYFILL};

#[test]
fn the_vendored_stub_carries_a_call_site_without_the_tool() {
    let temp = tempfile::tempdir().unwrap();
    let entry = script(
        temp.path(),
        r#"
        step() { echo "ran $*"; return 7; }
        BASHCAP -BCV:absent -BCS:"a note"
        WITH_BASHCAP -BCV:absent -BCS:"a note" step one two
        "#,
    );

    let ran = std::process::Command::new("bash").arg(entry).output().unwrap();

    assert_eq!(String::from_utf8(ran.stderr).unwrap(), "", "no flag was run as a command");
    assert_eq!(String::from_utf8(ran.stdout).unwrap(), "ran one two\n");
    assert_eq!(ran.status.code(), Some(7), "the continuation's own status");
}

#[test]
fn no_shipped_bash_exports_a_name() {
    let walk = stack::with(&[]);
    let shipped = [("stack.bash", walk.as_str()), ("bashcap.bash", BASH),
        ("trace.bash", TRACE), ("bashcap_polyfill.bash", POLYFILL)];

    for (whose, bash) in shipped {
        for line in bash.lines().filter(|line| !line.trim_start().starts_with('#')) {
            assert!(!line.contains("export "), "{whose}: {line}");
        }
    }
}
