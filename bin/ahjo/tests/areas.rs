//! The node's real list of areas holds together: no two share a name, a
//! frame kind or a command, none takes a reserved kind or one of the
//! binary's own verbs, and `ahjo kinds` prints who owns which kinds. All
//! of it is read from the listings; no area is built.

use std::process::Command;

use ahjo_node_kit::listing;

#[test]
fn the_listed_areas_do_not_conflict() {
    listing::check(ahjo::AREAS).unwrap();
    for area in ahjo::AREAS {
        for command in area.commands {
            let noun = command.split('_').next().unwrap();
            assert!(
                !ahjo::VERBS.contains(&noun),
                "{}'s command {command} takes the binary's verb {noun}",
                area.name
            );
        }
    }
}

#[test]
fn ahjo_kinds_prints_the_table_of_the_listed_areas() {
    let out = Command::new(env!("CARGO_BIN_EXE_ahjo"))
        .arg("kinds")
        .output()
        .unwrap();
    assert!(out.status.success());
    let printed = String::from_utf8(out.stdout).unwrap();
    assert_eq!(printed, listing::kinds_table(ahjo::AREAS));
    for area in ahjo::AREAS.iter().filter(|a| !a.kinds.is_empty()) {
        assert!(
            printed
                .lines()
                .any(|l| l.ends_with(&format!("  {}", area.name))),
            "{} has no row in\n{printed}",
            area.name
        );
    }
    assert!(printed.contains("the node's hello"), "{printed}");
}
