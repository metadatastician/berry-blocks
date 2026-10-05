// SPDX-License-Identifier: MPL-2.0
//! Harness screens: listing, refusing bad report paths, reading reports back.

use std::collections::BTreeMap;
use std::fs;

use berry_blocks_wizard::{handle, parse_urlencoded, App, Request};

/// A root with one configuration and one saved report.
fn app() -> App {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let r = std::env::temp_dir().join(format!(
        "bb-harr-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&r);
    fs::create_dir_all(r.join("wikis")).unwrap();
    fs::create_dir_all(r.join("reports")).unwrap();
    fs::write(r.join("wikis/lab.kyaml"), "{\n}\n").unwrap();
    fs::write(r.join("reports/lab-2026-10-05.kyaml"), "# SPDX-License-Identifier: MPL-2.0\n{\n  config: \"lab\",\n  date: \"2026-10-05\",\n  checks: [\n    {\n      key: \"escaping\",\n      title: \"Untrusted text stays text\",\n      outcome: \"fail\",\n      evidence: \"markup reached the output\",\n    },\n    {\n      key: \"axe-light\",\n      title: \"Accessible in light mode\",\n      outcome: \"not-run\",\n      evidence: \"no Chromium found\",\n    },\n  ],\n}\n").unwrap();
    App {
        root: r,
        addr: "127.0.0.1:23880".into(),
    }
}

/// A same-origin GET.
fn get(path: &str) -> berry_blocks_wizard::Response {
    let mut headers = BTreeMap::new();
    headers.insert("host".into(), "127.0.0.1:23880".into());
    let (p, q) = path.split_once('?').unwrap_or((path, ""));
    handle(
        &app(),
        &Request {
            method: "GET".into(),
            path: p.into(),
            query: parse_urlencoded(q),
            headers,
            form: BTreeMap::new(),
        },
    )
}

#[test]
/// Results show failures with advice and say plainly what did not run.
fn results_page_reports_fail_and_not_run() {
    let r = get("/harness/results?config=lab&date=2026-10-05");
    assert_eq!(r.status, 200);
    assert!(r.body.contains("1 of 2 checks fail"));
    assert!(r.body.contains("<a href=\"#check-escaping\">"));
    assert!(r.body.contains("1 check(s) were not run"));
    assert!(r.body.contains("A check that did not run is not a pass"));
    assert!(
        r.body.contains("escape_html"),
        "advice for the failing check"
    );
    assert!(!r.body.to_ascii_lowercase().contains("<script"));
    let list = get("/harness");
    assert!(list.body.contains("1 of 2 fail") && list.body.contains("1 not run"));
}

#[test]
/// Report paths cannot be steered outside reports/.
fn bad_report_paths_are_refused() {
    for q in [
        "config=../wikis/lab&date=2026-10-05",
        "config=lab&date=../../x",
        "config=Lab&date=2026-10-05",
    ] {
        assert_eq!(get(&format!("/harness/results?{q}")).status, 404, "{q}");
    }
}
