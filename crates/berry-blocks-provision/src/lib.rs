// SPDX-License-Identifier: MPL-2.0
//! Provision: pin a plugin's upstream code to one exact commit.
//!
//! Like Mint, provisioning is split so the preview is exactly what is written:
//!
//! * [`plan`] validates the request, fetches the commit (shallow, by SHA) into
//!   a cache under `vendor/.cache/`, checks it, and returns a [`Plan`]: the
//!   check results and every repository file to modify, with a digest. Nothing
//!   in the repository changes.
//! * [`apply`] recomputes the plan, refuses unless the digest matches and every
//!   check passed, writes `pins.kyaml` and the plugin's manifest, and checks the
//!   commit out into `vendor/<plugin>/` (gitignored).
//!
//! Checks, all before anything is written: the commit is a full SHA, the
//! repository URL is `https://` or `file://`, the commit exists, its licence is
//! compatible with MPL-2.0 (owner ruling 2026-10-05: refuse otherwise), and
//! every file the plugin needs exists at that commit.

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use sha2::{Digest, Sha256};

/// Licences compatible with the lab's MPL-2.0, as SPDX identifiers.
pub const COMPATIBLE: [&str; 5] = [
    "MPL-2.0",
    "Apache-2.0",
    "MIT",
    "BSD-2-Clause",
    "BSD-3-Clause",
];

/// One pinned project in `pins.kyaml`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pin {
    /// Key in the file: `berrywiki`, or a plugin name.
    pub name: String,
    /// Repository URL.
    pub repo: String,
    /// Full commit SHA.
    pub commit: String,
}

/// The contents of `pins.kyaml`: leading comment lines, then the pins.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pins {
    /// Comment lines before the opening brace, kept verbatim.
    pub header: Vec<String>,
    /// Pins in file order.
    pub pins: Vec<Pin>,
}

impl Pins {
    /// Reads the exact KYAML shape this repo uses; refuses anything else.
    /// Preserves leading comments and pin order; trailing blank lines are allowed.
    /// Returns an error for malformed structure or quoted fields. Pin names,
    /// repository URLs, and commit IDs are not validated here.
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut lines = text.lines().peekable();
        let mut header = Vec::new();
        while let Some(l) = lines.peek() {
            if l.starts_with('#') {
                header.push(l.to_string());
                lines.next();
            } else {
                break;
            }
        }
        if lines.next() != Some("{") {
            return Err("expected `{` after the header comments".into());
        }
        let mut pins = Vec::new();
        loop {
            let line = lines.next().ok_or("unexpected end of file")?;
            if line == "}" {
                break;
            }
            let name = line
                .strip_prefix("  ")
                .and_then(|l| l.strip_suffix(": {"))
                .ok_or_else(|| format!("unexpected line: {line}"))?;
            let repo = quoted_field(lines.next(), "repo")?;
            let commit = quoted_field(lines.next(), "commit")?;
            if lines.next() != Some("  },") {
                return Err(format!("expected `  }},` after {name}"));
            }
            pins.push(Pin {
                name: name.to_string(),
                repo,
                commit,
            });
        }
        if lines.any(|l| !l.trim().is_empty()) {
            return Err("unexpected content after the closing `}`".into());
        }
        Ok(Self { header, pins })
    }

    /// Writes the file in the same shape `parse` reads.
    /// Values are emitted verbatim without validation or escaping; callers must
    /// supply names and field values that fit that shape.
    pub fn render(&self) -> String {
        let mut out = String::new();
        for h in &self.header {
            out.push_str(h);
            out.push('\n');
        }
        out.push_str("{\n");
        for p in &self.pins {
            out.push_str(&format!(
                "  {}: {{\n    repo: \"{}\",\n    commit: \"{}\",\n  }},\n",
                p.name, p.repo, p.commit
            ));
        }
        out.push_str("}\n");
        out
    }

    /// The pin for a name, if any.
    pub fn get(&self, name: &str) -> Option<&Pin> {
        self.pins.iter().find(|p| p.name == name)
    }
}

/// Reads `    key: "value",` from one line.
fn quoted_field(line: Option<&str>, key: &str) -> Result<String, String> {
    let line = line.ok_or("unexpected end of file")?;
    line.strip_prefix(&format!("    {key}: \""))
        .and_then(|l| l.strip_suffix("\","))
        .filter(|v| !v.contains('"') && !v.contains('\\'))
        .map(str::to_string)
        .ok_or_else(|| format!("expected `{key}: \"…\",`, got: {line}"))
}

/// What a person asks to provision.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProvisionRequest {
    /// Plugin name (must already be minted).
    pub plugin: String,
    /// Upstream repository URL.
    pub repo: String,
    /// Full 40-character commit SHA.
    pub commit: String,
    /// Files the plugin needs from upstream, relative paths.
    pub files: Vec<String>,
}

/// A problem with one form field.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FieldError {
    /// Form field name.
    pub field: &'static str,
    /// What is wrong and what to do.
    pub message: String,
}

/// The outcome of one pre-fetch check, shown as a table row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Check {
    /// What was checked, in plain words.
    pub what: String,
    /// Whether it passed.
    pub passed: bool,
    /// What was found.
    pub evidence: String,
}

/// One repository file the plan rewrites.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    /// Path relative to the repository root.
    pub path: String,
    /// The file's full content before.
    pub before: String,
    /// The file's full content after.
    pub after: String,
}

/// Everything provisioning would do, computed without changing the repository.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    /// The checks, in order. Apply refuses unless all passed.
    pub checks: Vec<Check>,
    /// Repository files to rewrite (`pins.kyaml`, the manifest).
    pub changes: Vec<Change>,
    /// Commit subject line, for the preview.
    pub subject: String,
    /// Total bytes of the needed files.
    pub bytes: u64,
    /// SHA-256 over the request, checks and changes, hex.
    pub digest: String,
}

impl Plan {
    /// True when every check passed.
    pub fn ok(&self) -> bool {
        self.checks.iter().all(|c| c.passed)
    }
}

/// Why provisioning was refused.
#[derive(Debug, PartialEq, Eq)]
pub enum ProvisionError {
    /// The request is invalid; nothing was fetched or changed.
    Invalid(Vec<FieldError>),
    /// A check failed; nothing in the repository was changed.
    ChecksFailed(Plan),
    /// The plan no longer matches the preview; nothing was changed.
    Stale,
    /// Something else failed (I/O, git). Says what was already written.
    Failed { written: Vec<String>, error: String },
}

impl fmt::Display for ProvisionError {
    /// Describes the refusal in one sentence.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProvisionError::Invalid(e) => write!(f, "{} field(s) need fixing", e.len()),
            ProvisionError::ChecksFailed(p) => write!(
                f,
                "{} check(s) failed",
                p.checks.iter().filter(|c| !c.passed).count()
            ),
            ProvisionError::Stale => write!(f, "the plan changed since it was previewed"),
            ProvisionError::Failed { error, .. } => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for ProvisionError {}

/// Path of a plugin's manifest.
pub fn manifest_path(root: &Path, plugin: &str) -> PathBuf {
    root.join("plugins")
        .join(plugin)
        .join(format!("{plugin}.plugin_praxis.deed"))
}

/// Splits a whitespace- or newline-separated file list.
pub fn parse_files(s: &str) -> Vec<String> {
    s.split_whitespace().map(str::to_string).collect()
}

/// Checks every field; returns all problems at once.
pub fn validate(req: &ProvisionRequest, root: &Path) -> Vec<FieldError> {
    let mut errs = Vec::new();
    let name_ok = req.plugin.chars().next().is_some_and(|c| c.is_ascii_lowercase())
        && req.plugin.len() <= 40
        && req.plugin.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !req.plugin.ends_with('-')
        && !req.plugin.contains("--");
    if !name_ok || !manifest_path(root, &req.plugin).is_file() {
        errs.push(FieldError {
            field: "plugin",
            message: format!("No minted plugin called {}. Mint it first.", req.plugin),
        });
    }
    let repo_ok = (req.repo.starts_with("https://") || req.repo.starts_with("file:///"))
        && !req
            .repo
            .chars()
            .any(|c| c.is_whitespace() || c == '"' || c == '\\');
    if !repo_ok {
        errs.push(FieldError {
            field: "repo",
            message: "Use an https:// repository URL (or file:/// for a local repository).".into(),
        });
    }
    if !(req.commit.len() == 40
        && req
            .commit
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()))
    {
        let looks_like_ref = !req.commit.is_empty()
            && req.commit.len() < 40
            && !req.commit.chars().all(|c| c.is_ascii_hexdigit());
        let message = if looks_like_ref {
            format!("{} is a branch or tag name, not a commit. Branches move, so the plugin could change without anyone choosing it. Use the full 40-character commit ID.", req.commit)
        } else {
            "Use the full 40-character commit ID in lowercase hex.".into()
        };
        errs.push(FieldError {
            field: "commit",
            message,
        });
    }
    if req.files.is_empty() {
        errs.push(FieldError {
            field: "files",
            message: "List at least one file the plugin needs from upstream.".into(),
        });
    } else if req.files.iter().any(|f| {
        f.starts_with('/')
            || f.split('/').any(|c| c == ".." || c.is_empty())
            || f.contains('"')
            || f.contains('\\')
            || f.contains(')')
    }) {
        errs.push(FieldError {
            field: "files",
            message:
                "Use plain relative paths such as src/prog-block.js, with no .. and no leading /."
                    .into(),
        });
    }
    errs
}

/// Runs git with the given arguments; returns stdout or a one-line error.
fn git(args: &[&str]) -> Result<Vec<u8>, String> {
    let out = Command::new("git")
        .args(args)
        .output()
        .map_err(|e| format!("could not run git: {e}"))?;
    if out.status.success() {
        Ok(out.stdout)
    } else {
        let err = String::from_utf8_lossy(&out.stderr);
        Err(err
            .lines()
            .last()
            .unwrap_or("git failed")
            .trim()
            .to_string())
    }
}

/// Recognizes a license by text fragments after normalizing whitespace.
/// Returns an SPDX identifier, the generic label `GPL`, or `None` if unrecognized.
/// Recognition alone does not imply compatibility; see [`COMPATIBLE`].
pub fn detect_licence(text: &str) -> Option<&'static str> {
    let t: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if t.contains("Mozilla Public License Version 2.0")
        || t.contains("Mozilla Public License, version 2.0")
    {
        Some("MPL-2.0")
    } else if t.contains("Apache License") && t.contains("Version 2.0") {
        Some("Apache-2.0")
    } else if t
        .contains("Permission is hereby granted, free of charge, to any person obtaining a copy")
    {
        Some("MIT")
    } else if t.contains("Redistribution and use in source and binary forms") {
        if t.contains("Neither the name") {
            Some("BSD-3-Clause")
        } else {
            Some("BSD-2-Clause")
        }
    } else if t.contains("GNU AFFERO GENERAL PUBLIC LICENSE") {
        Some("AGPL-3.0")
    } else if t.contains("GNU GENERAL PUBLIC LICENSE") {
        Some("GPL")
    } else {
        None
    }
}

/// The plugin's manifest with its `(upstream …)` clause replaced or added.
/// Preserves the existing `:interface` value when found. Returns an error if
/// the existing clause is unbalanced or the remaining manifest has no closing form.
fn manifest_with_upstream(manifest: &str, req: &ProvisionRequest) -> Result<String, String> {
    let interface = manifest
        .find("(upstream")
        .and_then(|i| manifest[i..].split(":interface \"").nth(1))
        .and_then(|r| r.split('"').next())
        .map(|v| format!("\n    :interface \"{v}\""))
        .unwrap_or_default();
    let without = remove_clause(manifest, "(upstream")?;
    let files = req
        .files
        .iter()
        .map(|f| format!("\"{f}\""))
        .collect::<Vec<_>>()
        .join(" ");
    let clause = format!(
        "\n  (upstream\n    :repo \"{}\"\n    :commit \"{}\"\n    :files ({files}){interface})",
        req.repo, req.commit
    );
    let close = last_top_level_close(&without).ok_or("the manifest has no closing parenthesis")?;
    Ok(format!(
        "{}{clause}{}",
        &without[..close],
        &without[close..]
    ))
}

/// Byte offset of the `)` that closes the first top-level form.
fn last_top_level_close(text: &str) -> Option<usize> {
    let (mut depth, mut in_str, mut esc, mut in_comment) = (0i32, false, false, false);
    for (i, c) in text.char_indices() {
        if in_comment {
            in_comment = c != '\n';
            continue;
        }
        if in_str {
            match (esc, c) {
                (true, _) => esc = false,
                (false, '\\') => esc = true,
                (false, '"') => in_str = false,
                _ => {}
            }
            continue;
        }
        match c {
            ';' => in_comment = true,
            '"' => in_str = true,
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

/// Removes the clause starting with `head` (and the whitespace before it).
/// Uses the first literal match; returns the text unchanged if absent, or an
/// error if the matched clause has no matching closing parenthesis.
fn remove_clause(text: &str, head: &str) -> Result<String, String> {
    let Some(start) = text.find(head) else {
        return Ok(text.to_string());
    };
    let tail = &text[start..];
    let end = last_top_level_close(tail).ok_or("unbalanced clause in the manifest")?;
    let trimmed_start = text[..start].trim_end().len();
    Ok(format!(
        "{}{}",
        &text[..trimmed_start],
        &text[start + end + 1..]
    ))
}

/// The cache repository for a plugin's upstream.
fn cache_dir(root: &Path, plugin: &str) -> PathBuf {
    root.join("vendor")
        .join(".cache")
        .join(format!("{plugin}.git"))
}

/// Fetches into `vendor/.cache/<plugin>.git` under `root` and checks the commit,
/// license, and requested paths. Returns checks, the subject, and total bytes.
/// A fetch failure or commit mismatch returns one failed check, an empty subject,
/// and zero bytes. Subject lookup failures yield an empty subject; paths whose
/// sizes cannot be read are reported as missing. Unparseable sizes contribute zero.
fn run_checks(root: &Path, req: &ProvisionRequest) -> (Vec<Check>, String, u64) {
    let cache = cache_dir(root, &req.plugin);
    let cache_s = cache.to_string_lossy().to_string();
    let mut checks = Vec::new();
    if !cache.join("HEAD").is_file() {
        let _ = fs::create_dir_all(&cache);
        let _ = git(&["init", "-q", "--bare", &cache_s]);
    }
    let fetched = git(&[
        "--git-dir",
        &cache_s,
        "fetch",
        "-q",
        "--depth",
        "1",
        &req.repo,
        &req.commit,
    ])
    .and_then(|_| git(&["--git-dir", &cache_s, "rev-parse", "FETCH_HEAD^{commit}"]))
    .map(|o| String::from_utf8_lossy(&o).trim().to_string());
    match fetched {
        Ok(sha) if sha == req.commit => {
            let subject = git(&[
                "--git-dir",
                &cache_s,
                "log",
                "-1",
                "--format=%s",
                &req.commit,
            ])
            .map(|o| String::from_utf8_lossy(&o).trim().to_string())
            .unwrap_or_default();
            checks.push(Check {
                what: "The commit exists in the repository".into(),
                passed: true,
                evidence: format!("{}, \"{subject}\"", &sha[..7]),
            });
            let licence_text = ["LICENSE", "LICENSE.md", "LICENSE.txt", "COPYING"]
                .iter()
                .find_map(|f| {
                    git(&[
                        "--git-dir",
                        &cache_s,
                        "show",
                        &format!("{}:{f}", req.commit),
                    ])
                    .ok()
                    .map(|t| (f, String::from_utf8_lossy(&t).into_owned()))
                });
            match licence_text.as_ref().map(|(f, t)| (f, detect_licence(t))) {
                Some((f, Some(id))) if COMPATIBLE.contains(&id) => checks.push(Check {
                    what: "Its licence is compatible".into(),
                    passed: true,
                    evidence: format!("{id}, from {f}"),
                }),
                Some((f, Some(id))) => checks.push(Check {
                    what: "Its licence is compatible".into(),
                    passed: false,
                    evidence: format!(
                        "{id} (from {f}) is not compatible with MPL-2.0; compatible: {}",
                        COMPATIBLE.join(", ")
                    ),
                }),
                Some((f, None)) => checks.push(Check {
                    what: "Its licence is compatible".into(),
                    passed: false,
                    evidence: format!("{f} exists but its licence was not recognised"),
                }),
                None => checks.push(Check {
                    what: "Its licence is compatible".into(),
                    passed: false,
                    evidence: "no LICENSE, LICENSE.md, LICENSE.txt or COPYING file at that commit"
                        .into(),
                }),
            }
            let mut bytes = 0u64;
            let mut missing = Vec::new();
            for f in &req.files {
                match git(&[
                    "--git-dir",
                    &cache_s,
                    "cat-file",
                    "-s",
                    &format!("{}:{f}", req.commit),
                ]) {
                    Ok(o) => {
                        bytes += String::from_utf8_lossy(&o)
                            .trim()
                            .parse::<u64>()
                            .unwrap_or(0)
                    }
                    Err(_) => missing.push(f.clone()),
                }
            }
            checks.push(if missing.is_empty() {
                Check {
                    what: "The files the plugin needs are there".into(),
                    passed: true,
                    evidence: format!("{} · {bytes} bytes", req.files.join(", ")),
                }
            } else {
                Check {
                    what: "The files the plugin needs are there".into(),
                    passed: false,
                    evidence: format!("missing at that commit: {}", missing.join(", ")),
                }
            });
            (checks, subject, bytes)
        }
        Ok(other) => {
            checks.push(Check {
                what: "The commit exists in the repository".into(),
                passed: false,
                evidence: format!("fetched {other}, not the requested commit"),
            });
            (checks, String::new(), 0)
        }
        Err(e) => {
            checks.push(Check {
                what: "The commit exists in the repository".into(),
                passed: false,
                evidence: format!("could not fetch it: {e}"),
            });
            (checks, String::new(), 0)
        }
    }
}

/// Computes what provisioning would do. Fetches into the cache only; the
/// repository's own files are not changed.
/// `root` is the lab repository root. The returned plan includes changed
/// file contents, check results, and a digest for [`apply`]. Fetch, license, and
/// missing-file failures are returned as failed checks in an `Ok` plan.
///
/// # Errors
/// Returns [`ProvisionError::Invalid`] for invalid fields before fetching, or
/// [`ProvisionError::Failed`] if reading or parsing the pins or preparing the
/// manifest fails.
pub fn plan(req: &ProvisionRequest, root: &Path) -> Result<Plan, ProvisionError> {
    let errs = validate(req, root);
    if !errs.is_empty() {
        return Err(ProvisionError::Invalid(errs));
    }
    let fail = |e: String| ProvisionError::Failed {
        written: vec![],
        error: e,
    };
    let pins_before = fs::read_to_string(root.join("pins.kyaml"))
        .map_err(|e| fail(format!("pins.kyaml: {e}")))?;
    let mut pins = Pins::parse(&pins_before).map_err(|e| fail(format!("pins.kyaml: {e}")))?;
    match pins.pins.iter_mut().find(|p| p.name == req.plugin) {
        Some(p) => {
            p.repo = req.repo.clone();
            p.commit = req.commit.clone();
        }
        None => pins.pins.push(Pin {
            name: req.plugin.clone(),
            repo: req.repo.clone(),
            commit: req.commit.clone(),
        }),
    }
    let man_path = manifest_path(root, &req.plugin);
    let man_before = fs::read_to_string(&man_path).map_err(|e| fail(e.to_string()))?;
    let man_after = manifest_with_upstream(&man_before, req).map_err(fail)?;
    let (checks, subject, bytes) = run_checks(root, req);
    let mut changes = vec![Change {
        path: "pins.kyaml".into(),
        before: pins_before.clone(),
        after: pins.render(),
    }];
    if man_after != man_before {
        changes.push(Change {
            path: format!("plugins/{0}/{0}.plugin_praxis.deed", req.plugin),
            before: man_before,
            after: man_after,
        });
    }
    changes.retain(|c| c.before != c.after);
    let mut h = Sha256::new();
    h.update(
        format!(
            "{}\0{}\0{}\0{}\0",
            req.plugin,
            req.repo,
            req.commit,
            req.files.join("\n")
        )
        .as_bytes(),
    );
    for c in &checks {
        h.update(format!("{}\0{}\0{}\0", c.what, c.passed, c.evidence).as_bytes());
    }
    for c in &changes {
        h.update(format!("{}\0{}\0{}\0", c.path, c.before, c.after).as_bytes());
    }
    let digest = h.finalize().iter().map(|b| format!("{b:02x}")).collect();
    Ok(Plan {
        checks,
        changes,
        subject,
        bytes,
        digest,
    })
}

/// Writes the plan, if and only if it matches the preview and every check passed.
/// Recomputes [`plan`] under the lab repository `root`, including its cache fetch,
/// and compares it with `previewed_digest` from the previewed plan. Writes the
/// changed pins and manifest, then replaces `vendor/<plugin>/` with a checkout
/// of the requested commit. Returns the applied plan.
///
/// # Errors
/// Propagates errors from [`plan`]. Returns [`ProvisionError::Stale`] on a digest
/// mismatch, or [`ProvisionError::ChecksFailed`] if the matching plan fails checks.
/// These refusals can update the cache but do not write the plan or checkout.
/// Write and checkout errors return [`ProvisionError::Failed`] with the repository
/// files already written. Changes are not rolled back: temporary files or a
/// partially replaced vendor checkout may remain.
pub fn apply(
    req: &ProvisionRequest,
    root: &Path,
    previewed_digest: &str,
) -> Result<Plan, ProvisionError> {
    let plan = plan(req, root)?;
    if plan.digest != previewed_digest {
        return Err(ProvisionError::Stale);
    }
    if !plan.ok() {
        return Err(ProvisionError::ChecksFailed(plan));
    }
    let mut written = Vec::new();
    for c in &plan.changes {
        let target = root.join(&c.path);
        let tmp = target.with_extension("berry-blocks-tmp");
        if let Err(e) = fs::write(&tmp, &c.after).and_then(|_| fs::rename(&tmp, &target)) {
            return Err(ProvisionError::Failed {
                written,
                error: format!("{}: {e}", c.path),
            });
        }
        written.push(c.path.clone());
    }
    let vendor = root.join("vendor").join(&req.plugin);
    let vendor_s = vendor.to_string_lossy().to_string();
    let cache = cache_dir(root, &req.plugin);
    let checkout = (|| {
        if vendor.exists() {
            fs::remove_dir_all(&vendor).map_err(|e| e.to_string())?;
        }
        git(&["init", "-q", &vendor_s])?;
        git(&[
            "-C",
            &vendor_s,
            "fetch",
            "-q",
            "--depth",
            "1",
            &format!("file://{}", cache.display()),
            &req.commit,
        ])?;
        git(&[
            "-C",
            &vendor_s,
            "-c",
            "advice.detachedHead=false",
            "checkout",
            "-q",
            "FETCH_HEAD",
        ])
        .map(|_| ())
    })();
    if let Err(e) = checkout {
        return Err(ProvisionError::Failed {
            written,
            error: format!("vendor/{}: {e}", req.plugin),
        });
    }
    Ok(plan)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The pins.kyaml committed at the start of this work.
    const PINS: &str = "# SPDX-License-Identifier: MPL-2.0\n# The exact commits this lab integrates.\n{\n  berrywiki: {\n    repo: \"https://github.com/metadatastician/berrywiki\",\n    commit: \"9bf43190e799573ccc4b08d306e50affeb4228bb\",\n  },\n  progblocks: {\n    repo: \"https://github.com/metadatastician/progblocks\",\n    commit: \"ccb412289d736abc6aea5609ed50bb53d90e9388\",\n  },\n}\n";

    #[test]
    /// The reader and writer round-trip the real file shape exactly.
    fn pins_round_trip() {
        let p = Pins::parse(PINS).unwrap();
        assert_eq!(p.pins.len(), 2);
        assert_eq!(
            p.get("progblocks").unwrap().commit,
            "ccb412289d736abc6aea5609ed50bb53d90e9388"
        );
        assert_eq!(p.render(), PINS);
    }

    #[test]
    /// Any other shape is refused rather than rewritten.
    fn unexpected_pins_shape_is_refused() {
        assert!(Pins::parse(&PINS.replace("    commit:", "    sha:")).is_err());
        assert!(Pins::parse(&format!("{PINS}extra\n")).is_err());
    }

    #[test]
    /// Licences are recognised from their text.
    fn detects_licences() {
        assert_eq!(
            detect_licence("Mozilla Public License Version 2.0\n=="),
            Some("MPL-2.0")
        );
        assert_eq!(
            detect_licence("                 Apache License\n           Version 2.0, January 2004"),
            Some("Apache-2.0")
        );
        assert_eq!(
            detect_licence(
                "Permission is hereby granted, free of charge, to any person obtaining a copy"
            ),
            Some("MIT")
        );
        assert_eq!(
            detect_licence("GNU GENERAL PUBLIC LICENSE Version 3"),
            Some("GPL")
        );
        assert_eq!(detect_licence("all rights reserved"), None);
    }

    #[test]
    /// The upstream clause is replaced in place, keeping the manifest balanced.
    fn manifest_upstream_is_replaced() {
        let m = "(praxis-deed\n  :schema-version \"1.0.0\" ; a (comment)\n  (plugin :id \"x\")\n  (upstream\n    :repo \"old\"\n    :commit \"0\"\n    :interface \"<x-y> element\"))\n";
        let r = ProvisionRequest {
            plugin: "p".into(),
            repo: "https://e/x".into(),
            commit: "a".repeat(40),
            files: vec!["src/a.js".into()],
        };
        let out = manifest_with_upstream(m, &r).unwrap();
        assert_eq!(out.matches("(upstream").count(), 1);
        assert!(out.contains(":repo \"https://e/x\""));
        assert!(out.contains(":files (\"src/a.js\")"));
        assert!(!out.contains("old"));
        assert!(out.trim_end().ends_with("))"));
        assert!(out.contains("; a (comment)"));
        assert!(out.contains(":interface \"<x-y> element\""));
    }

    #[test]
    /// A branch name is refused with the reason, before anything is fetched.
    fn branch_names_are_refused() {
        let dir = std::env::temp_dir();
        let r = ProvisionRequest {
            plugin: "nope".into(),
            repo: "ssh://x".into(),
            commit: "main".into(),
            files: vec![],
        };
        let fields: Vec<_> = validate(&r, &dir)
            .into_iter()
            .map(|e| (e.field, e.message))
            .collect();
        assert_eq!(
            fields.iter().map(|f| f.0).collect::<Vec<_>>(),
            vec!["plugin", "repo", "commit", "files"]
        );
        assert!(fields[2].1.contains("branch or tag name"));
    }
}
