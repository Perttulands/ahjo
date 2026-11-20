//! Arrangement goldens. The boards are Archon's; the places are Ahjo's
//! since it wraps a mission into rows only when one row would not read on
//! a full-size window, with one card's room for notes between cards rather
//! than two.
use ahjo_mission_engine::arrange;
use ahjo_mission_model::{Layout, LayoutNode, Mission};

#[test]
fn arrangements_match_the_goldens() {
    for name in ["arrange-golden.json", "arrange-long-golden.json"] {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/parity")
            .join(name);
        let mut fixture: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let board = fixture["board"].as_object_mut().unwrap();
        board.remove("etag");
        board.remove("tools");
        for (from, to) in [
            ("input_cards", "input_card"),
            ("steps", "step"),
            ("gates", "gate"),
            ("ends", "end"),
            ("limits", "limit"),
            ("connections", "connection"),
        ] {
            if let Some(value) = board.remove(from) {
                board.insert(to.into(), value);
            }
        }
        if let Some(steps) = board.get_mut("step").and_then(|v| v.as_array_mut()) {
            for step in steps {
                let object = step.as_object_mut().unwrap();
                for (from, to) in [
                    ("inputs", "input"),
                    ("outputs", "output"),
                    ("seats", "seat"),
                ] {
                    if let Some(value) = object.remove(from) {
                        object.insert(to.into(), value);
                    }
                }
            }
        }
        let mission: Mission = serde_json::from_value(fixture["board"].clone()).unwrap();
        let expected: Vec<LayoutNode> =
            serde_json::from_value(fixture["positions"].clone()).unwrap();
        let result = arrange(&mission, &Layout::default());
        assert_eq!(result.nodes, expected, "{name}");
    }
}
