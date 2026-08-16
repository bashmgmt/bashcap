//! Call arguments: bash records them only under `extdebug`, so a frame
//! either has them or says it was never told.

use std::path::Path;

use bash_interop::rig::{Driving};
use crate::{captures, BashCap};
use bash_interop::scratch::bash;

use super::{capture, script, ENTRY};

#[tokio::test]
async fn call_arguments_arrive_where_the_shell_was_recording_them() {
    let body = r#"
        deep() { BASHCAP -BCS:"at the bottom"; }
        mid()  { deep "d one" $'d\ntwo'; }
        mid "m one"
        "#;

    let bare = capture(body).await;
    assert!(
        bare[0].snapshot.stack.frames().all(|frame| frame.args.is_none()),
        "an ordinary shell records none, and says so"
    );

    let traced = capture(&format!("shopt -s extdebug\n{body}")).await;
    let called: Vec<&[String]> = traced[0]
        .snapshot
        .stack
        .frames()
        .map(|frame| frame.args.as_deref().expect("the shell was recording"))
        .collect();

    // The two instrument frames are not reported, but their own arguments
    // still sit in the flat `BASH_ARGV` stack; miscounting them would shift
    // every frame's group.
    assert_eq!(called, [["d one", "d\ntwo"].as_slice(), ["m one"].as_slice(), [].as_slice()]);

    let shown = traced[0].snapshot.stack.top().to_string();
    assert!(
        shown.ends_with(" ('d one' $'d\\ntwo')"),
        "rendered as the bash that would pass them, newline and all: {shown}"
    );
}

#[tokio::test]
async fn the_tools_switch_traces_a_subject_that_never_asked_for_it() {
    let scripts = script(
        r#"
        outer() { BASHCAP -BCS:top; }
        outer 'at the top'
        bash "${BASH_SOURCE[0]%/*}/child.bash"
        "#,
    );
    std::fs::write(
        scripts.at("child.bash"),
        r#"
        deep() { BASHCAP -BCS:child; }
        deep 'in a child'
        "#,
    )
    .unwrap();

    let ran = async |tool: BashCap, into: &Path| {
        tool.run(&bash(scripts.at(ENTRY)), |at| vec![at.bash_env()])
            .await
            .unwrap()
            .whole()
            .unwrap();

        captures(&std::fs::read_to_string(into).unwrap()).unwrap()
    };

    let plain = scripts.at("plain.jsonl");
    let bare = ran(BashCap::writing(&plain).unwrap(), &plain).await;
    assert_eq!(bare.len(), 2, "one snapshot from each shell");
    assert!(
        bare.iter().flat_map(|seen| seen.snapshot.stack.frames()).all(|frame| frame.args.is_none()),
        "nothing records call arguments unless the tool is asked to"
    );

    let full = scripts.at("traced.jsonl");
    let traced = ran(BashCap::writing(&full).unwrap().tracing_calls(), &full).await;
    let called: Vec<Vec<&[String]>> = traced
        .iter()
        .map(|seen| {
            seen.snapshot
                .stack
                .frames()
                .map(|frame| frame.args.as_deref().expect("asked for, so recorded"))
                .collect()
        })
        .collect();

    assert_eq!(
        called,
        [
            [["at the top"].as_slice(), [].as_slice()],
            [["in a child"].as_slice(), [].as_slice()],
        ],
        "the switch reached the child process too"
    );
    assert_ne!(traced[0].shell.pid, traced[1].shell.pid, "two shells, not one");
}
