// SPDX-License-Identifier: MPL-2.0
//! Every page of BerryWiki's own fixture wiki, rendered through the host with
//! the ProgBlocks block, must be byte-identical to BerryWiki's render: the lab
//! may only change fences an author explicitly marked with `variant=`.

use std::fs;
use std::path::PathBuf;

use berry_blocks_host::{render_page, Profile};
use berry_blocks_progblocks::ProgBlocks;

/// The pinned BerryWiki fixture wiki, fetched by scripts/fetch-pins.sh.
fn fixture_wiki() -> PathBuf {
    let dir =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../vendor/berrywiki/fixtures/test-wiki");
    assert!(
        dir.is_dir(),
        "{} missing: run scripts/fetch-pins.sh first",
        dir.display()
    );
    dir
}

#[test]
/// No BerryWiki fixture page changes under either profile.
fn berrywiki_fixture_pages_are_unchanged() {
    let block = ProgBlocks::default();
    let mut checked = 0;
    for entry in fs::read_dir(fixture_wiki()).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|x| x != "md") {
            continue;
        }
        let md = fs::read_to_string(&path).unwrap();
        let expected = berrywiki_render::render_markdown(&md);
        for profile in [Profile::Static, Profile::Enhanced] {
            let got = render_page(&md, &[&block], profile).unwrap();
            assert_eq!(
                got.html,
                expected,
                "{} changed under {profile:?}",
                path.display()
            );
            assert_eq!(got.runs, 0);
        }
        checked += 1;
    }
    assert!(
        checked >= 10,
        "expected the full fixture wiki, checked only {checked} pages"
    );
}
