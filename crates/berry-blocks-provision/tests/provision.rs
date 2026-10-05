// SPDX-License-Identifier: MPL-2.0
//! Provision against real local git repositories: no network.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use berry_blocks_provision::{apply, plan, Pins, ProvisionError, ProvisionRequest};

/// Runs git in `dir`, panicking on failure; returns trimmed stdout.
fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// A unique scratch directory.
fn scratch(tag: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let d = std::env::temp_dir().join(format!(
        "bb-prov-{tag}-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&d).unwrap();
    d
}

/// An upstream repository with the given licence text and files; returns (path, sha).
fn upstream(licence: Option<&str>, files: &[&str]) -> (PathBuf, String) {
    let d = scratch("up");
    git(&d, &["init", "-q"]);
    if let Some(l) = licence {
        fs::write(d.join("LICENSE"), l).unwrap();
    }
    for f in files {
        let p = d.join(f);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, "export {};\n").unwrap();
    }
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
            "upstream commit",
        ],
    );
    let sha = git(&d, &["rev-parse", "HEAD"]);
    (d, sha)
}

/// A berry-blocks root with pins.kyaml and one minted plugin, `demo`.
fn root() -> PathBuf {
    let d = scratch("root");
    fs::write(d.join("pins.kyaml"), "# SPDX-License-Identifier: MPL-2.0\n{\n  berrywiki: {\n    repo: \"https://github.com/metadatastician/berrywiki\",\n    commit: \"9bf43190e799573ccc4b08d306e50affeb4228bb\",\n  },\n}\n").unwrap();
    fs::create_dir_all(d.join("plugins/demo")).unwrap();
    fs::write(d.join("plugins/demo/demo.plugin_praxis.deed"), ";; SPDX-License-Identifier: MPL-2.0\n(praxis-deed\n  :schema-version \"1.0.0\"\n  (plugin\n    :id \"x\"))\n").unwrap();
    d
}

/// A request for `demo` against a local upstream.
fn req(up: &Path, sha: &str, files: &[&str]) -> ProvisionRequest {
    ProvisionRequest {
        plugin: "demo".into(),
        repo: format!("file://{}", up.display()),
        commit: sha.into(),
        files: files.iter().map(|s| s.to_string()).collect(),
    }
}

#[test]
/// Preview fetches and checks but changes no repository file; apply pins it.
fn provisions_a_good_commit() {
    let (up, sha) = upstream(Some("Mozilla Public License Version 2.0\n"), &["src/a.js"]);
    let r = root();
    let pins_before = fs::read_to_string(r.join("pins.kyaml")).unwrap();
    let p = plan(&req(&up, &sha, &["src/a.js"]), &r).unwrap();
    assert!(p.ok(), "{:?}", p.checks);
    assert_eq!(p.subject, "upstream commit");
    assert_eq!(p.changes.len(), 2);
    assert_eq!(
        fs::read_to_string(r.join("pins.kyaml")).unwrap(),
        pins_before,
        "preview must not write"
    );
    apply(&req(&up, &sha, &["src/a.js"]), &r, &p.digest).unwrap();
    let pins = Pins::parse(&fs::read_to_string(r.join("pins.kyaml")).unwrap()).unwrap();
    assert_eq!(pins.get("demo").unwrap().commit, sha);
    assert!(
        fs::read_to_string(r.join("plugins/demo/demo.plugin_praxis.deed"))
            .unwrap()
            .contains(&format!(":commit \"{sha}\""))
    );
    assert_eq!(git(&r.join("vendor/demo"), &["rev-parse", "HEAD"]), sha);
    assert!(r.join("vendor/demo/src/a.js").is_file());
}

#[test]
/// An incompatible licence fails the check and apply refuses without writing.
fn refuses_an_incompatible_licence() {
    let (up, sha) = upstream(Some("GNU GENERAL PUBLIC LICENSE\nVersion 3\n"), &["a.js"]);
    let r = root();
    let p = plan(&req(&up, &sha, &["a.js"]), &r).unwrap();
    assert!(!p.ok());
    assert!(p.checks[1].evidence.contains("GPL"));
    assert!(matches!(
        apply(&req(&up, &sha, &["a.js"]), &r, &p.digest),
        Err(ProvisionError::ChecksFailed(_))
    ));
    assert!(!fs::read_to_string(r.join("pins.kyaml"))
        .unwrap()
        .contains("demo"));
    assert!(!r.join("vendor/demo").exists());
}

#[test]
/// A missing file, a missing licence and an unknown commit all fail their checks.
fn reports_missing_files_licence_and_commit() {
    let (up, sha) = upstream(None, &["a.js"]);
    let r = root();
    let p = plan(&req(&up, &sha, &["a.js", "b.css"]), &r).unwrap();
    assert!(!p.checks[1].passed && p.checks[1].evidence.contains("no LICENSE"));
    assert!(!p.checks[2].passed && p.checks[2].evidence.contains("b.css"));
    let p = plan(&req(&up, &"0".repeat(40), &["a.js"]), &r).unwrap();
    assert_eq!(p.checks.len(), 1);
    assert!(!p.checks[0].passed);
}

#[test]
/// Changing the request after the preview is refused, and nothing is written.
fn stale_preview_is_refused() {
    let (up, sha) = upstream(Some("MIT License\n\nPermission is hereby granted, free of charge, to any person obtaining a copy\n"), &["a.js", "b.js"]);
    let r = root();
    let p = plan(&req(&up, &sha, &["a.js"]), &r).unwrap();
    assert_eq!(
        apply(&req(&up, &sha, &["a.js", "b.js"]), &r, &p.digest),
        Err(ProvisionError::Stale)
    );
    assert!(!fs::read_to_string(r.join("pins.kyaml"))
        .unwrap()
        .contains("demo"));
}
