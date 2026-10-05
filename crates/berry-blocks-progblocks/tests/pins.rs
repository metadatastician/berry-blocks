// SPDX-License-Identifier: MPL-2.0
//! The Cargo git rev for berrywiki-render must equal pins.kyaml's commit.

/// Returns the first 40-hex-character run after `needle` in `text`.
fn sha_after(text: &str, needle: &str) -> String {
    let start = text
        .find(needle)
        .unwrap_or_else(|| panic!("{needle} not found"))
        + needle.len();
    text[start..]
        .chars()
        .skip_while(|c| !c.is_ascii_hexdigit())
        .take(40)
        .collect()
}

#[test]
/// One pin, recorded in two places, must agree.
fn cargo_rev_matches_pins_file() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
    let cargo = std::fs::read_to_string(format!("{root}/Cargo.toml")).unwrap();
    let pins = std::fs::read_to_string(format!("{root}/pins.kyaml")).unwrap();
    let rev = sha_after(&cargo, "berrywiki\", rev = ");
    let commit = sha_after(pins.split("berrywiki:").nth(1).unwrap(), "commit:");
    assert_eq!(rev.len(), 40, "Cargo rev is not a full SHA: {rev}");
    assert_eq!(rev, commit);
}
