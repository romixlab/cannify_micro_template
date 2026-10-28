//! The firmware's `pins` points (`firmware/src/pins.rs`) are the pins the board map marks free
//! (`docs/board/b129a_cannify-pins.md`, rendered from the board TOML): a pin the TOML takes is never driven, a free
//! pin is never left out. The firmware builds for another target, so its list is read as text.

use std::collections::BTreeSet;

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/..");

/// `NAME: CAPS` entries of the firmware's `with_points!` list
fn firmware_points() -> Vec<(String, String)> {
    let src = std::fs::read_to_string(format!("{ROOT}/firmware/src/pins.rs")).unwrap();
    let start = src
        .find("macro_rules! with_points")
        .expect("with_points! in pins.rs");
    let list = &src[start..];
    let list = &list[list.find("$($args)*").unwrap() + "$($args)*".len()..];
    let list = &list[..list.find('}').unwrap()];
    list.split(',')
        .map(str::trim)
        .filter(|e| !e.is_empty())
        .map(|e| {
            let (name, caps) = e.split_once(':').expect("NAME: CAPS");
            (name.trim().to_string(), caps.trim().to_string())
        })
        .collect()
}

/// Pin and role of every row of the board map's pin table
fn board_pins() -> Vec<(String, String)> {
    let md = std::fs::read_to_string(format!("{ROOT}/docs/board/b129a_cannify-pins.md"))
        .expect("docs/board/b129a_cannify-pins.md: render it with `just board` (boardmap)");
    fn cells(l: &str) -> Vec<&str> {
        l.split('|').map(str::trim).collect()
    }
    // the Role column moves: boardmap adds an alias column (a Nucleo's Arduino names) only where the TOML has any
    let header = md
        .lines()
        .find(|l| l.starts_with("| Pin |"))
        .expect("the pin table's header");
    let role_col = cells(header)
        .iter()
        .position(|c| *c == "Role")
        .expect("a Role column");
    md.lines()
        .filter(|l| l.starts_with("| `P"))
        .map(|l| {
            let cells = cells(l);
            // "`PA11` USB_N": the pin, then the name KiCad's symbol gives it
            let pin = cells[1]
                .split_whitespace()
                .next()
                .unwrap()
                .trim_matches('`')
                .to_string();
            let role = cells[role_col]
                .split_whitespace()
                .last()
                .unwrap()
                .to_string();
            (pin, role)
        })
        .collect()
}

#[test]
fn points_are_the_free_pins() {
    let points = firmware_points();
    let board = board_pins();
    assert!(board.len() > 27, "pin table not parsed: {board:?}");

    let free: BTreeSet<_> = board
        .iter()
        .filter(|(_, r)| r == "free")
        .map(|(p, _)| p.clone())
        .collect();
    let driven: BTreeSet<_> = points
        .iter()
        .filter(|(_, c)| c == "FREE")
        .map(|(p, _)| p.clone())
        .collect();
    assert_eq!(
        driven, free,
        "FREE points of firmware/src/pins.rs vs free pins of the board map"
    );

    for (pin, caps) in &points {
        let role = board
            .iter()
            .find(|(p, _)| p == pin)
            .map(|(_, r)| r.as_str());
        match caps.as_str() {
            "FREE" => {}
            // read only: a pin a part of the board drives or pulls, never the debug port, reset or a crystal
            "READ_ONLY" => assert_eq!(role, Some("shared"), "{pin}"),
            other => panic!("{pin}: caps {other}"),
        }
    }
    let unique: BTreeSet<_> = points.iter().map(|(p, _)| p).collect();
    assert_eq!(unique.len(), points.len(), "a pin listed twice");
}
