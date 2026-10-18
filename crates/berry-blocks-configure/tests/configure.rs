// SPDX-License-Identifier: MPL-2.0
//! Configure against the real lab wiki.

use std::fs;
use std::path::PathBuf;

use berry_blocks_configure::{
    apply, load, plan, ConfigureError, ConfigureRequest, PluginConfig, WikiConfig,
};

/// A scratch root holding a copy of the lab wiki and a minted `progblocks`.
fn root() -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let r = std::env::temp_dir().join(format!(
        "bb-conf-{}-{}",
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
    r
}

/// Enhanced progblocks with persist on.
fn req(name: &str) -> ConfigureRequest {
    ConfigureRequest {
        name: name.into(),
        wiki: "wiki".into(),
        profile: "enhanced".into(),
        plugins: vec![PluginConfig {
            name: "progblocks".into(),
            options: vec![("persist".into(), true)],
        }],
    }
}

#[test]
/// Preview reports per-page effects and writes nothing; apply writes the file.
fn preview_then_apply() {
    let r = root();
    let p = plan(&req("lab"), &r).unwrap();
    let changed: Vec<_> = p
        .pages
        .iter()
        .filter(|e| e.runs > 0)
        .map(|e| (e.page.as_str(), e.runs))
        .collect();
    assert_eq!(
        changed,
        vec![("Install-ripgrep.md", 1), ("Upgrade-ripgrep.md", 1)]
    );
    assert_eq!(p.pages.len(), 4);
    assert!(p.before.is_none());
    assert!(!r.join("wikis").exists(), "preview must not write");
    apply(&req("lab"), &r, &p.digest).unwrap();
    let text = fs::read_to_string(r.join("wikis/lab.kyaml")).unwrap();
    assert_eq!(text, p.after);
    let (cfg, blocks) = load(&r.join("wikis/lab.kyaml")).unwrap();
    assert_eq!((cfg.profile.as_str(), blocks.len()), ("enhanced", 1));
}

#[test]
/// Editing an existing configuration keeps its header and shows the before.
fn reconfigure_keeps_header() {
    let r = root();
    let p = plan(&req("lab"), &r).unwrap();
    apply(&req("lab"), &r, &p.digest).unwrap();
    let mut again = req("lab");
    again.profile = "static".into();
    let p2 = plan(&again, &r).unwrap();
    assert!(p2.before.is_some());
    assert_eq!(
        WikiConfig::parse(&p2.after).unwrap().header,
        WikiConfig::parse(p2.before.as_ref().unwrap())
            .unwrap()
            .header
    );
}

#[test]
/// Unknown plugins and options, unminted plugins and missing wikis are refused.
fn refuses_bad_requests() {
    let r = root();
    let mut bad = req("Bad Name");
    bad.wiki = "nowhere".into();
    bad.plugins.push(PluginConfig {
        name: "ghost".into(),
        options: vec![],
    });
    bad.plugins[0].options.push(("colour".into(), true));
    match plan(&bad, &r) {
        Err(ConfigureError::Invalid(errs)) => {
            let fields: Vec<_> = errs.iter().map(|e| e.field).collect();
            assert!(
                fields.contains(&"name") && fields.contains(&"wiki") && fields.contains(&"plugins")
            );
            assert!(errs
                .iter()
                .any(|e| e.message.contains("ghost is not registered")));
            assert!(errs
                .iter()
                .any(|e| e.message.contains("no option called colour")));
        }
        other => panic!("expected refusal, got {other:?}"),
    }
    fs::remove_dir_all(r.join("plugins/progblocks")).unwrap();
    assert!(matches!(
        plan(&req("lab"), &r),
        Err(ConfigureError::Invalid(_))
    ));
}

#[test]
/// A preview made before the wiki changed is refused.
fn stale_preview_is_refused() {
    let r = root();
    let p = plan(&req("lab"), &r).unwrap();
    fs::write(r.join("wiki/New.md"), "```sh variant=a\n1\n```\n").unwrap();
    assert_eq!(
        apply(&req("lab"), &r, &p.digest),
        Err(ConfigureError::Stale)
    );
    assert!(!r.join("wikis").exists());
}
