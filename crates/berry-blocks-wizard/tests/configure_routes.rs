// SPDX-License-Identifier: MPL-2.0
//! Configure screens against a copy of the lab wiki.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use berry_blocks_wizard::{handle, parse_urlencoded, App, Request, Response};

/// A root with a copy of the lab wiki and a minted `progblocks`.
fn app() -> App {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let r = std::env::temp_dir().join(format!(
        "bb-confr-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    let lab = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/lab-wiki");
    fs::create_dir_all(r.join("wiki")).unwrap();
    for e in fs::read_dir(lab).unwrap() {
        let p = e.unwrap().path();
        fs::copy(&p, r.join("wiki").join(p.file_name().unwrap())).unwrap();
    }
    fs::create_dir_all(r.join("plugins/progblocks")).unwrap();
    fs::write(
        r.join("plugins/progblocks/progblocks.plugin_praxis.deed"),
        "(praxis-deed\n  (plugin\n    :id \"x\"))\n",
    )
    .unwrap();
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

/// Asserts a page carries no script.
fn script_free(r: &Response) {
    let b = r.body.to_ascii_lowercase();
    assert!(
        !b.contains("<script") && !b.contains("javascript:"),
        "{}",
        r.body
    );
}

const FORM: &str =
    "config_name=lab&wiki=wiki&profile=enhanced&use_progblocks=on&opt_progblocks_persist=on";

#[test]
/// The form offers registered, minted plugins and their options.
fn form_offers_available_plugins() {
    let a = app();
    let r = handle(&a, &req("GET", "/configure?new", ""));
    script_free(&r);
    assert!(r.body.contains("name=\"use_progblocks\""));
    assert!(r.body.contains("name=\"opt_progblocks_persist\""));
    assert!(r
        .body
        .contains("type=\"submit\" disabled>Save configuration"));
}

#[test]
/// Preview shows the file and the per-page effects; saving writes it; the list shows it.
fn preview_then_save() {
    let a = app();
    let pv = handle(&a, &req("POST", "/configure/preview", FORM));
    script_free(&pv);
    assert_eq!(pv.status, 200, "{}", pv.body);
    assert!(
        pv.body
            .contains("Of the wiki&#39;s 4 pages, 2 render differently")
            || pv
                .body
                .contains("Of the wiki's 4 pages, 2 render differently")
    );
    assert!(pv.body.contains("persist: true,"));
    assert!(!a.root.join("wikis").exists());
    let d = pv
        .body
        .split("name=\"digest\" value=\"")
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap();
    let done = handle(
        &a,
        &req("POST", "/configure", &format!("{FORM}&digest={d}")),
    );
    assert_eq!(
        (done.status, done.location.as_deref()),
        (303, Some("/configure/done?config=lab"))
    );
    let page = handle(&a, &req("GET", "/configure/done?config=lab", ""));
    script_free(&page);
    assert!(page.body.contains("lab is configured"));
    let list = handle(&a, &req("GET", "/configure", ""));
    assert!(list.body.contains("/configure?config=lab"));
    let edit = handle(&a, &req("GET", "/configure?config=lab", ""));
    assert!(edit
        .body
        .contains("id=\"use-progblocks\" name=\"use_progblocks\" checked"));
}

#[test]
/// Missing plugins and a bad name are reported and linked; nothing is saved.
fn invalid_configuration_is_refused() {
    let a = app();
    let r = handle(
        &a,
        &req(
            "POST",
            "/configure/preview",
            "config_name=Bad+Name&wiki=nowhere&profile=enhanced",
        ),
    );
    script_free(&r);
    assert_eq!(r.status, 422);
    assert!(r
        .body
        .contains("<a href=\"#config_name\">Configuration name</a>"));
    assert!(r.body.contains("<a href=\"#wiki\">Wiki folder</a>"));
    assert!(r.body.contains("<a href=\"#plugins\">Plugins</a>"));
    assert!(!a.root.join("wikis").exists());
}
