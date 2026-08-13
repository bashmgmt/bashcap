//! The JSON line a run writes, and the reading that takes it back.

use crate::bash::rig::{ExitStatus, Master};
use crate::bashcap::{captures, BashCap};
use crate::tests::scripts::bash;

use super::{script, ENTRY};

#[test]
fn each_snapshot_is_written_as_it_arrives() {
    let scripts = script(
        r#"
        BASHCAP -BCS:one
        BASHCAP -BCS:two
        "#,
    );
    let into = scripts.at("out.jsonl");

    let (capturing, status) =
        BashCap::writing(&into).run(&bash(scripts.at(ENTRY))).unwrap().whole().unwrap();
    assert_eq!(capturing.written, 2);
    assert_eq!(status, ExitStatus::Code(0));

    let rows: Vec<serde_json::Value> = std::fs::read_to_string(&into)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();

    assert_eq!(rows.len(), 2);
    for row in &rows {
        assert!(row["sent"]["pid"].is_u64(), "provenance is its own object: {row}");
        for key in ["stack", "state"] {
            assert!(row["snapshot"].get(key).is_some(), "{key} missing from {row}");
        }
    }
    assert_eq!(rows[0]["snapshot"]["notes"][0], "one");
    assert_eq!(rows[1]["snapshot"]["notes"][0], "two");
}

#[test]
fn a_written_capture_reads_back_whole() {
    let scripts = script(
        r#"
        shopt -s extdebug
        f() { BASHCAP -BCS:one; }
        f arg
        "#,
    );
    let into = scripts.at("out.jsonl");

    BashCap::writing(&into).run(&bash(scripts.at(ENTRY))).unwrap().whole().unwrap();

    let read = captures(&std::fs::read_to_string(&into).unwrap()).unwrap();

    assert_eq!(read.len(), 1);
    assert_eq!(read[0].snapshot.stack.at().args.as_deref(), Some(["arg".to_string()].as_slice()));
    assert!(read[0].to_string().contains("f@main.bash"), "{}", read[0]);
}
