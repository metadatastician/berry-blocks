// SPDX-License-Identifier: MPL-2.0
//! Route tests: every screen script-free, preview-gated minting, refusals.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use berry_blocks_wizard::{handle, parse_urlencoded, App, Request, Response};

/// A scratch berry-blocks root holding only a workspace Cargo.toml.
fn app() -> App {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let root = std::env::temp_dir().join(format!(
        "bb-wizard-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("Cargo.toml"), "[workspace]\nmembers = [\n    \"crates/berry-blocks-host\",\n    \"crates/berry-blocks-cli\",\n]\n").unwrap();
    App {
        root,
        addr: "127.0.0.1:23880".into(),
    }
}

/// A request with the right Host header.
fn req(method: &str, path: &str, form: &str) -> Request {
    let mut headers = BTreeMap::new();
    headers.insert("host".into(), "127.0.0.1:23880".into());
    headers.insert("origin".into(), "http://127.0.0.1:23880".into());
    let (p, q) = path.split_once('?').unwrap_or((path, ""));
    Request {
        method: method.into(),
        path: p.into(),
        query: parse_urlencoded(q),
        headers,
        form: parse_urlencoded(form),
    }
}

const VALID: &str =
    "name=progblocks&display=ProgBlocks&claims=variant&run_key=group&enhanced=on&licence=MPL-2.0";

/// Pulls the digest out of a preview page.
fn digest_of(r: &Response) -> String {
    r.body
        .split("name=\"digest\" value=\"")
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap()
        .to_string()
}

#[test]
/// No route ever serves a script, as BerryWiki asserts for its own routes.
fn every_route_is_script_free() {
    let a = app();
    let mut pages = vec![];
    for p in [
        "/",
        "/mint",
        "/provision",
        "/configure",
        "/harness",
        "/nope",
        "/mint/done?name=x",
    ] {
        pages.push(handle(&a, &req("GET", p, "")));
    }
    pages.push(handle(&a, &req("POST", "/mint/preview", VALID)));
    pages.push(handle(&a, &req("POST", "/mint/preview", "name=Bad")));
    for r in &pages {
        let b = r.body.to_ascii_lowercase();
        assert!(
            !b.contains("<script")
                && !b.contains("javascript:")
                && !b.contains(" onclick=")
                && !b.contains(" onerror="),
            "{}",
            r.body
        );
    }
}

#[test]
/// Preview writes nothing and shows every file; minting with its digest writes them.
fn preview_then_mint() {
    let a = app();
    let pv = handle(&a, &req("POST", "/mint/preview", VALID));
    assert_eq!(pv.status, 200);
    assert!(pv.body.contains("Preview: what minting will do"));
    assert!(pv
        .body
        .contains("plugins/progblocks/progblocks.plugin_praxis.deed"));
    assert!(pv.body.contains("c39f8b97-e19b-88cf-bc7f-15c773f72113"));
    assert!(!a.root.join("plugins").exists(), "preview must not write");
    let d = digest_of(&pv);
    let done = handle(&a, &req("POST", "/mint", &format!("{VALID}&digest={d}")));
    assert_eq!(
        (done.status, done.location.as_deref()),
        (303, Some("/mint/done?name=progblocks"))
    );
    assert!(a
        .root
        .join("crates/berry-blocks-progblocks/src/lib.rs")
        .is_file());
    let page = handle(&a, &req("GET", "/mint/done?name=progblocks", ""));
    assert!(page.body.contains("progblocks is minted"));
    assert!(handle(&a, &req("GET", "/", ""))
        .body
        .contains("c39f8b97-e19b-88cf-bc7f-15c773f72113"));
}

#[test]
/// Minting without the previewed digest, or after editing a field, writes nothing.
fn mint_without_matching_preview_is_refused() {
    let a = app();
    let r = handle(&a, &req("POST", "/mint", VALID));
    assert_eq!(r.status, 409);
    let d = digest_of(&handle(&a, &req("POST", "/mint/preview", VALID)));
    let edited = VALID.replace("display=ProgBlocks", "display=Other");
    let r = handle(&a, &req("POST", "/mint", &format!("{edited}&digest={d}")));
    assert_eq!(r.status, 409);
    assert!(r.body.contains("Nothing was created or changed"));
    assert!(!a.root.join("plugins").exists());
}

#[test]
/// Bad fields come back marked invalid, each linked from the error banner.
fn field_errors_are_marked_and_linked() {
    let a = app();
    let r = handle(
        &a,
        &req(
            "POST",
            "/mint/preview",
            "name=Bad+Name&display=&claims=variant&run_key=variant&licence=GPL",
        ),
    );
    assert_eq!(r.status, 422);
    for f in ["name", "display", "run_key"] {
        assert!(
            r.body.contains(&format!("<a href=\"#{f}\">")),
            "{f} not linked"
        );
        assert!(
            r.body
                .contains(&format!("id=\"{f}\" name=\"{f}\" value=\"")),
            "{f} missing"
        );
    }
    assert_eq!(r.body.matches("aria-invalid=\"true\"").count(), 3);
    assert!(r.body.contains("Bad Name"), "the entered value is kept");
}

#[test]
/// Requests for another host, or form posts from another site, are refused.
fn foreign_requests_are_refused() {
    let a = app();
    let mut r = req("POST", "/mint/preview", VALID);
    r.headers
        .insert("origin".into(), "http://evil.example".into());
    assert_eq!(handle(&a, &r).status, 403);
    let mut r = req("GET", "/", "");
    r.headers.insert("host".into(), "evil.example".into());
    assert_eq!(handle(&a, &r).status, 403);
}

#[test]
/// Form decoding handles +, %XX, bad escapes and multibyte text.
fn decodes_forms() {
    let m = parse_urlencoded("a=x+y&b=%3Cb%3E&c=%zz&d=%E2%82%AC");
    assert_eq!(m["a"], "x y");
    assert_eq!(m["b"], "<b>");
    assert_eq!(m["c"], "%zz");
    assert_eq!(m["d"], "€");
}

#[test]
/// Markup typed into a field is shown as text, never as markup.
fn field_values_are_escaped() {
    let a = app();
    let r = handle(
        &a,
        &req(
            "POST",
            "/mint/preview",
            "name=%3Cimg+src%3Dx%3E&display=%3Cb%3E",
        ),
    );
    assert!(!r.body.contains("<img") && !r.body.contains("<b>"));
    let _ = PathBuf::new();
}
