// SPDX-License-Identifier: MPL-2.0
//! Provision screens against a local upstream repository (no network).

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use berry_blocks_wizard::{handle, parse_urlencoded, App, Request, Response};

/// Runs git in `dir`; returns trimmed stdout.
fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}");
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// A unique scratch directory.
fn scratch(tag: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let d = std::env::temp_dir().join(format!(
        "bb-provr-{tag}-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&d).unwrap();
    d
}

/// An upstream repo with a licence and `src/a.js`; returns (url, sha).
fn upstream(licence: &str) -> (String, String) {
    let d = scratch("up");
    git(&d, &["init", "-q"]);
    fs::write(d.join("LICENSE"), licence).unwrap();
    fs::create_dir_all(d.join("src")).unwrap();
    fs::write(d.join("src/a.js"), "export {};\n").unwrap();
    git(&d, &["add", "-A"]);
    git(
        &d,
        &[
            "-c",
            "user.email=t@t",
            "-c",
            "user.name=t",
            "commit",
            "-qm",
            "first",
        ],
    );
    (
        format!("file://{}", d.display()),
        git(&d, &["rev-parse", "HEAD"]),
    )
}

/// A root with pins.kyaml, a wrapped plugin `wrap` (has upstream) and an in-repo plugin `local`.
fn app() -> App {
    let r = scratch("root");
    fs::write(r.join("pins.kyaml"), "# SPDX-License-Identifier: MPL-2.0\n{\n  berrywiki: {\n    repo: \"https://github.com/metadatastician/berrywiki\",\n    commit: \"9bf43190e799573ccc4b08d306e50affeb4228bb\",\n  },\n}\n").unwrap();
    for (name, extra) in [("wrap", "\n  (upstream\n    :repo \"https://example.invalid/x\"\n    :commit \"0000000000000000000000000000000000000000\"\n    :files (\"src/a.js\"))"), ("local", "")] {
        fs::create_dir_all(r.join(format!("plugins/{name}"))).unwrap();
        fs::write(r.join(format!("plugins/{name}/{name}.plugin_praxis.deed")), format!("(praxis-deed\n  :schema-version \"1.0.0\"\n  (plugin\n    :id \"id-{name}\"\n    :display \"{name}\"\n    :crate \"berry-blocks-{name}\"){extra})\n")).unwrap();
    }
    App {
        root: r,
        addr: "127.0.0.1:23880".into(),
    }
}

/// A same-origin request.
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

/// Percent-encodes a form value.
fn enc(s: &str) -> String {
    s.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

/// The digest hidden in a preview page.
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

/// Asserts a page carries no script.
fn script_free(r: &Response) {
    let b = r.body.to_ascii_lowercase();
    assert!(
        !b.contains("<script") && !b.contains("javascript:"),
        "{}",
        r.body
    );
}

#[test]
/// The list marks in-repo plugins as not needed and wrapped ones as not yet.
fn list_shows_each_plugins_state() {
    let a = app();
    let r = handle(&a, &req("GET", "/provision", ""));
    script_free(&r);
    assert!(r
        .body
        .contains("not needed: its code lives in berry-blocks"));
    assert!(r.body.contains("/provision?plugin=wrap"));
    let form = handle(&a, &req("GET", "/provision?plugin=wrap", ""));
    script_free(&form);
    assert!(form.body.contains("value=\"src/a.js\""));
    assert!(form
        .body
        .contains("<button class=\"btn\" type=\"submit\" disabled>Provision</button>"));
}

#[test]
/// A branch name is refused with the reason, and the field is marked invalid.
fn branch_name_is_refused() {
    let a = app();
    let r = handle(
        &a,
        &req(
            "POST",
            "/provision/preview",
            "plugin=wrap&repo=https%3A%2F%2Fexample.org%2Fx&commit=main&files=src%2Fa.js",
        ),
    );
    script_free(&r);
    assert_eq!(r.status, 422);
    assert!(r.body.contains("Nothing was fetched or changed"));
    assert!(r.body.contains("is a branch or tag name"));
    assert!(r.body.contains("<a href=\"#commit\">Commit</a>"));
    assert!(r
        .body
        .contains("id=\"commit\" name=\"commit\" value=\"main\" aria-invalid=\"true\""));
}

#[test]
/// A passing preview offers Provision; only pressing it writes.
fn preview_then_provision() {
    let a = app();
    let (url, sha) = upstream("Mozilla Public License Version 2.0\n");
    let f = format!(
        "plugin=wrap&repo={}&commit={sha}&files=src%2Fa.js",
        enc(&url)
    );
    let pv = handle(&a, &req("POST", "/provision/preview", &f));
    script_free(&pv);
    assert_eq!(pv.status, 200, "{}", pv.body);
    assert_eq!(pv.body.matches("✓ Pass").count(), 3);
    assert!(pv.body.contains("<ins>"));
    assert!(!fs::read_to_string(a.root.join("pins.kyaml"))
        .unwrap()
        .contains("wrap"));
    let done = handle(
        &a,
        &req(
            "POST",
            "/provision",
            &format!("{f}&digest={}", digest_of(&pv)),
        ),
    );
    assert_eq!(
        (done.status, done.location.as_deref()),
        (303, Some("/provision/done?plugin=wrap"))
    );
    let page = handle(&a, &req("GET", "/provision/done?plugin=wrap", ""));
    script_free(&page);
    assert!(page
        .body
        .contains(&format!("wrap is provisioned at {}", &sha[..7])));
    assert!(handle(&a, &req("GET", "/", ""))
        .body
        .contains(&format!("✓ {}", &sha[..7])));
}

#[test]
/// An incompatible licence fails its check; there is no Provision button and nothing changes.
fn incompatible_licence_is_refused() {
    let a = app();
    let (url, sha) = upstream("GNU GENERAL PUBLIC LICENSE\nVersion 3\n");
    let f = format!(
        "plugin=wrap&repo={}&commit={sha}&files=src%2Fa.js",
        enc(&url)
    );
    let pv = handle(&a, &req("POST", "/provision/preview", &f));
    script_free(&pv);
    assert_eq!(pv.status, 422);
    assert!(pv.body.contains("✗ Fail"));
    assert!(pv.body.contains("GPL"));
    assert!(!pv.body.contains("name=\"digest\""));
    let forced = handle(&a, &req("POST", "/provision", &format!("{f}&digest=abc")));
    assert!(forced.status == 409 || forced.status == 422);
    assert!(!fs::read_to_string(a.root.join("pins.kyaml"))
        .unwrap()
        .contains("wrap"));
}
