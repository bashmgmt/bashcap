//! What the injected bash may not do to a shell.

use bash_interop::stack;
use crate::instrument::{TRACE, WORDS};

#[test]
fn no_shipped_bash_exports_a_name() {
    let walk = stack::with_walk(&[]);
    let shipped =
        [("stack.bash", walk.as_str()), ("words.bash", WORDS), ("trace.bash", TRACE)];

    for (whose, bash) in shipped {
        for line in bash.lines().filter(|line| !line.trim_start().starts_with('#')) {
            assert!(!line.contains("export "), "{whose}: {line}");
        }
    }
}
