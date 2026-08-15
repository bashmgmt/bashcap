//! What a client ships: the words, the guard that decides whether they do
//! anything, and the contract the instrument keeps towards a shell it reaches.

use crate::bash;
use crate::bash::stack;
use crate::bashcap::instrument::{EFFECT, TRACE, WORDS};

use super::{script, ENTRY};

#[test]
fn the_vendored_words_carry_a_call_site_without_the_tool() {
    let scripts = script(
        r#"
        step() { echo "ran $*"; return 7; }
        BASHCAP -BCV:absent -BCS:"a note"
        WITH_BASHCAP -BCV:absent -BCS:"a note" step one two
        "#,
    );

    let ran = std::process::Command::new("bash").arg(scripts.at(ENTRY)).output().unwrap();

    assert_eq!(String::from_utf8(ran.stderr).unwrap(), "", "no flag was run as a command");
    assert_eq!(String::from_utf8(ran.stdout).unwrap(), "ran one two\n");
    assert_eq!(ran.status.code(), Some(7), "the continuation's own status");
}

/// The words are one file, shipped both ways, so a client's copy cannot drift
/// from the injected one. What makes that possible is that they name nothing
/// that only exists once the tool has been sourced.
#[test]
fn the_words_name_nothing_a_client_would_not_have() {
    for line in WORDS.lines().filter(|line| !line.trim_start().starts_with('#')) {
        for name in bash::INJECTED_NAMES {
            assert!(!line.contains(name), "{name} in a file a client vendors: {line}");
        }
    }
}

#[test]
fn no_shipped_bash_exports_a_name() {
    let walk = stack::with_walk(&[]);
    let shipped =
        [("stack.bash", walk.as_str()), ("bashcap.bash", WORDS), ("effect.bash", EFFECT),
         ("trace.bash", TRACE)];

    for (whose, bash) in shipped {
        for line in bash.lines().filter(|line| !line.trim_start().starts_with('#')) {
            assert!(!line.contains("export "), "{whose}: {line}");
        }
    }
}
