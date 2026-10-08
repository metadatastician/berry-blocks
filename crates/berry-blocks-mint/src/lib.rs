// SPDX-License-Identifier: MPL-2.0
//! Mint: create a new, empty berry-blocks plugin.
//!
//! Minting is split so that what a person previews is exactly what is written:
//!
//! * [`plan`] validates a [`MintRequest`] against the repository and returns a
//!   [`Plan`]: every file to create or modify, with its full new content, and a
//!   [`Plan::digest`] over all of it. It writes nothing.
//! * [`apply`] recomputes the plan, refuses unless its digest equals the one the
//!   person previewed, refuses to overwrite any file it would create, and only
//!   then writes.
//!
//! A plugin's ID is a UUID v8, profile C (ADR-008, adopted ahead of
//! ratification): bytes 0..15 of SHA-256(`berry-blocks-plugin:<name>`) with the
//! version and variant bits set. The same name always gives the same ID.

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

/// Licences a plugin may use: compatible with the lab's MPL-2.0.
pub const LICENCES: [&str; 3] = ["MPL-2.0", "Apache-2.0", "MIT"];

/// What a person asks to mint.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MintRequest {
    /// Short name: crate and folder names derive from it.
    pub name: String,
    /// Name shown to authors and readers.
    pub display: String,
    /// Info-string key that marks a fence as this plugin's.
    pub claims: String,
    /// Info-string key whose equal values group consecutive fences.
    pub run_key: String,
    /// Whether the plugin offers the enhanced profile as well as static.
    pub enhanced: bool,
    /// SPDX licence identifier, one of [`LICENCES`].
    pub licence: String,
}

/// A problem with one form field, in words a person can act on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FieldError {
    /// The form field's name, used to link the message to the input.
    pub field: &'static str,
    /// What is wrong and what to do.
    pub message: String,
}

/// Whether a change creates a file or rewrites one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChangeKind {
    /// A new file; refused if the path exists by the time it is applied.
    Create,
    /// An existing file rewritten in full.
    Modify,
}

/// One file the plan will write.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    /// Create or modify.
    pub kind: ChangeKind,
    /// Path relative to the repository root, `/`-separated.
    pub path: String,
    /// One-line note shown beside the path.
    pub note: &'static str,
    /// The file's full content after the change.
    pub content: String,
    /// For a modification, the lines added (shown as the diff).
    pub added_lines: Vec<String>,
}

/// Everything minting would do, computed without writing anything.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    /// The new plugin's ID (UUID v8, profile C).
    pub plugin_id: String,
    /// Crate name, `berry-blocks-<name>`.
    pub crate_name: String,
    /// The files, in the order they are shown and written.
    pub changes: Vec<Change>,
    /// SHA-256 over every change, hex. Apply refuses a different digest.
    pub digest: String,
}

/// Why minting was refused.
#[derive(Debug, PartialEq, Eq)]
pub enum MintError {
    /// The request itself is invalid; nothing was written.
    Invalid(Vec<FieldError>),
    /// The plan no longer matches what was previewed; nothing was written.
    Stale,
    /// A file that would be created already exists; nothing was written.
    Exists(String),
    /// Writing failed part-way. Lists what was written before the failure.
    Io { written: Vec<String>, error: String },
}

impl fmt::Display for MintError {
    /// Describes the refusal in one sentence.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MintError::Invalid(errs) => write!(f, "{} field(s) need fixing", errs.len()),
            MintError::Stale => write!(f, "the plan changed since it was previewed; preview again"),
            MintError::Exists(p) => write!(f, "{p} already exists"),
            MintError::Io { written, error } => {
                write!(f, "writing failed after {} file(s): {error}", written.len())
            }
        }
    }
}

impl std::error::Error for MintError {}

/// Formats bytes 0..15 of SHA-256(`domain:name`) as a UUID v8, profile C.
pub fn uuid_v8_profile_c(domain: &str, name: &str) -> String {
    let hash = Sha256::digest(format!("{domain}:{name}").as_bytes());
    let mut b = [0u8; 16];
    b.copy_from_slice(&hash[..16]);
    b[6] = (b[6] & 0x0f) | 0x80;
    b[8] = (b[8] & 0x3f) | 0x80;
    let h: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &h[0..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..32]
    )
}

/// True for `[a-z][a-z0-9-]*` of the given length range, with no `--` or trailing `-`.
fn is_slug(s: &str, max: usize) -> bool {
    let mut chars = s.chars();
    matches!(chars.next(), Some('a'..='z'))
        && s.len() <= max
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !s.ends_with('-')
        && !s.contains("--")
}

/// Checks every field; returns all problems at once, never just the first.
pub fn validate(req: &MintRequest, root: &Path) -> Vec<FieldError> {
    let mut errs = Vec::new();
    if !is_slug(&req.name, 40) {
        errs.push(FieldError {
            field: "name",
            message:
                "Use 1 to 40 lowercase letters, digits and single hyphens, starting with a letter."
                    .into(),
        });
    } else if root.join("plugins").join(&req.name).exists()
        || root
            .join("crates")
            .join(format!("berry-blocks-{}", req.name))
            .exists()
    {
        errs.push(FieldError {
            field: "name",
            message: format!(
                "A plugin called {} already exists. Choose another name.",
                req.name
            ),
        });
    }
    if req.display.trim().is_empty()
        || req.display.chars().count() > 60
        || req.display.chars().any(char::is_control)
    {
        errs.push(FieldError {
            field: "display",
            message: "Give a display name of 1 to 60 characters.".into(),
        });
    }
    if !is_slug(&req.claims, 30) {
        errs.push(FieldError {
            field: "claims",
            message: "Use a lowercase key such as variant, starting with a letter.".into(),
        });
    }
    if !is_slug(&req.run_key, 30) {
        errs.push(FieldError {
            field: "run_key",
            message: "Use a lowercase key such as group, starting with a letter.".into(),
        });
    } else if req.run_key == req.claims {
        errs.push(FieldError {
            field: "run_key",
            message: "Must differ from the key it claims.".into(),
        });
    }
    if !LICENCES.contains(&req.licence.as_str()) {
        errs.push(FieldError {
            field: "licence",
            message: format!("Choose one of: {}.", LICENCES.join(", ")),
        });
    }
    errs
}

/// Escapes text for a DEED string literal (only `\"`, `\\`, `\n`, `\t` exist).
fn deed_str(s: &str) -> String {
    let body = s
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\t', "\\t");
    format!("\"{body}\"")
}

/// Escapes text for a Rust string literal in generated code.
fn rust_str(s: &str) -> String {
    format!("{s:?}")
}

/// The manifest deed for a new plugin.
fn manifest(req: &MintRequest, id: &str, crate_name: &str) -> String {
    let profiles = if req.enhanced {
        "(static enhanced)"
    } else {
        "(static)"
    };
    format!(
        ";; SPDX-License-Identifier: {lic}\n;\n; Manifest of berry-blocks plugin {name}, per ADR-0002 (draft contract).\n; Minted by the berry-blocks wizard. The ID is a UUID v8, profile C, over\n; \"berry-blocks-plugin:{name}\".\n(praxis-deed\n  :schema-version \"1.0.0\"\n  :canonical-name {canon}\n  :beholding-chora #u5\"estate/chora\"\n  (plugin\n    :id \"{id}\"\n    :id-profile C\n    :id-domain \"berry-blocks-plugin\"\n    :display {display}\n    :crate \"{crate_name}\"\n    :claims {claims}\n    :run-key {run}\n    :profiles {profiles}\n    :licence \"{lic}\"\n    :static-assets ()\n    :enhanced-assets ()))\n",
        lic = req.licence,
        name = req.name,
        canon = deed_str(&format!("{}-block", req.name)),
        display = deed_str(&req.display),
        claims = deed_str(&format!("fenced code blocks whose info string has {}=", req.claims)),
        run = deed_str(&req.run_key),
    )
}

/// The new crate's manifest.
fn crate_toml(req: &MintRequest, crate_name: &str) -> String {
    format!(
        "# SPDX-License-Identifier: {lic}\n[package]\nname = \"{crate_name}\"\ndescription = {desc}\nversion.workspace = true\nedition.workspace = true\nrust-version.workspace = true\nlicense = \"{lic}\"\nrepository.workspace = true\n\n[dependencies]\nberry-blocks-host = {{ path = \"../berry-blocks-host\" }}\n\n[dev-dependencies]\nberrywiki-render.workspace = true\n\n[lints]\nworkspace = true\n",
        lic = req.licence,
        desc = rust_str(&format!("{}: a berry-blocks block.", req.display)),
    )
}

/// The new crate's source: a working block that renders claimed fences as
/// plain escaped code, so it is safe and testable before anyone changes it.
fn crate_lib(req: &MintRequest) -> String {
    let ty = req
        .name
        .split('-')
        .map(|w| {
            let mut c = w.chars();
            c.next()
                .map(|f| f.to_ascii_uppercase().to_string() + c.as_str())
                .unwrap_or_default()
        })
        .collect::<String>();
    let enhanced_note = if req.enhanced {
        "    /// Static output only, so far. Add enhanced assets when the block has them."
    } else {
        "    /// This block has no enhanced profile; it never needs assets."
    };
    format!(
        "// SPDX-License-Identifier: {lic}\n//! {display}: a berry-blocks block, minted by the wizard.\n//!\n//! It claims fences whose info string has `{claims}=` and groups consecutive\n//! fences that share a `{run}=` value. Until you change [`Block::render`], it\n//! renders each claimed fence as a plain, escaped code block.\n\nuse berry_blocks_host::{{escape_html, Asset, Block, Fence, FenceRun, Profile}};\n\n/// The {display} block.\n#[derive(Default)]\npub struct {ty};\n\nimpl Block for {ty} {{\n    /// Short name for errors and reports.\n    fn name(&self) -> &'static str {{\n        {name_lit}\n    }}\n\n    /// Claims any fence whose info string has `{claims}=`.\n    fn claims(&self, fence: &Fence) -> bool {{\n        fence.get({claims_lit}).is_some()\n    }}\n\n    /// A run continues while fences share the same `{run}` value.\n    fn continues(&self, previous: &Fence, next: &Fence) -> bool {{\n        previous.get({run_lit}) == next.get({run_lit})\n    }}\n\n    /// Renders each fence as escaped code. Replace with the block's real output.\n    fn render(&self, run: &FenceRun, _profile: Profile) -> String {{\n        run.fences\n            .iter()\n            .map(|f| format!(\"<pre><code>{{}}</code></pre>\\n\", escape_html(&f.code)))\n            .collect()\n    }}\n\n{enhanced_note}\n    fn assets(&self, _profile: Profile) -> Vec<Asset> {{\n        Vec::new()\n    }}\n}}\n",
        lic = req.licence,
        display = req.display.replace('\n', " "),
        claims = req.claims,
        run = req.run_key,
        name_lit = rust_str(&req.name),
        claims_lit = rust_str(&req.claims),
        run_lit = rust_str(&req.run_key),
    )
}

/// The new crate's conformance test: BerryWiki's pages must stay byte-identical.
fn crate_test(req: &MintRequest) -> String {
    let ty = req
        .name
        .split('-')
        .map(|w| {
            let mut c = w.chars();
            c.next()
                .map(|f| f.to_ascii_uppercase().to_string() + c.as_str())
                .unwrap_or_default()
        })
        .collect::<String>();
    let krate = format!("berry_blocks_{}", req.name.replace('-', "_"));
    format!(
        "// SPDX-License-Identifier: {lic}\n//! Every page of BerryWiki's fixture wiki must render byte-identically\n//! through the host with this block (run scripts/fetch-pins.sh first).\n\nuse berry_blocks_host::{{render_page, Profile}};\n\nuse {krate}::{ty};\n\n#[test]\n/// No BerryWiki fixture page changes under either profile.\nfn berrywiki_fixture_pages_are_unchanged() {{\n    let dir = std::path::PathBuf::from(env!(\"CARGO_MANIFEST_DIR\"))\n        .join(\"../../vendor/berrywiki/fixtures/test-wiki\");\n    assert!(\n        dir.is_dir(),\n        \"{{}} missing: run scripts/fetch-pins.sh first\",\n        dir.display()\n    );\n    let mut checked = 0;\n    for entry in std::fs::read_dir(dir).unwrap() {{\n        let path = entry.unwrap().path();\n        if path.extension().is_none_or(|x| x != \"md\") {{\n            continue;\n        }}\n        let md = std::fs::read_to_string(&path).unwrap();\n        let expected = berrywiki_render::render_markdown(&md);\n        for profile in [Profile::Static, Profile::Enhanced] {{\n            assert_eq!(\n                render_page(&md, &[&{ty}], profile).unwrap().html,\n                expected,\n                \"{{}}\",\n                path.display()\n            );\n        }}\n        checked += 1;\n    }}\n    assert!(checked >= 10, \"checked only {{checked}} pages\");\n}}\n",
        lic = req.licence,
    )
}

/// Adds `crates/<crate>` to the workspace members, before the CLI crate.
fn workspace_with_member(cargo_toml: &str, crate_name: &str) -> Option<String> {
    let anchor = "    \"crates/berry-blocks-cli\",\n";
    let line = format!("    \"crates/{crate_name}\",\n");
    if cargo_toml.contains(&line) || !cargo_toml.contains(anchor) {
        return None;
    }
    Some(cargo_toml.replacen(anchor, &format!("{line}{anchor}"), 1))
}

/// The plugin's type name: `zebra-notes` becomes `ZebraNotes`.
fn type_name(name: &str) -> String {
    name.split('-')
        .map(|w| {
            let mut c = w.chars();
            c.next()
                .map(|f| f.to_ascii_uppercase().to_string() + c.as_str())
                .unwrap_or_default()
        })
        .collect()
}

/// Inserts `insert` on the line before the line containing `marker`.
fn insert_before_marker(text: &str, marker: &str, insert: &str) -> Option<String> {
    if text.contains(insert) {
        return None;
    }
    let at = text.find(marker)?;
    let line_start = text[..at].rfind('\n').map_or(0, |i| i + 1);
    Some(format!(
        "{}{insert}{}",
        &text[..line_start],
        &text[line_start..]
    ))
}

/// Paths of the registry files Mint edits.
const REGISTRY_TOML: &str = "crates/berry-blocks-registry/Cargo.toml";
const REGISTRY_LIB: &str = "crates/berry-blocks-registry/src/lib.rs";

/// Computes what minting would do. Writes nothing.
pub fn plan(req: &MintRequest, root: &Path) -> Result<Plan, MintError> {
    let errs = validate(req, root);
    if !errs.is_empty() {
        return Err(MintError::Invalid(errs));
    }
    let id = uuid_v8_profile_c("berry-blocks-plugin", &req.name);
    let crate_name = format!("berry-blocks-{}", req.name);
    let workspace = fs::read_to_string(root.join("Cargo.toml")).map_err(|e| MintError::Io {
        written: vec![],
        error: e.to_string(),
    })?;
    let new_workspace = workspace_with_member(&workspace, &crate_name).ok_or_else(|| {
        MintError::Invalid(vec![FieldError {
            field: "name",
            message: "The workspace Cargo.toml does not have the expected members list.".into(),
        }])
    })?;
    let registry_err = |what: &str| {
        MintError::Invalid(vec![FieldError {
            field: "name",
            message: format!(
                "The registry's {what} is missing its Mint marker, or already lists this plugin."
            ),
        }])
    };
    let read = |p: &str| {
        fs::read_to_string(root.join(p)).map_err(|e| MintError::Io {
            written: vec![],
            error: format!("{p}: {e}"),
        })
    };
    let dep_line = format!("{crate_name} = {{ path = \"../{crate_name}\" }}\n");
    let reg_toml = insert_before_marker(
        &read(REGISTRY_TOML)?,
        "# berry-blocks:mint-dependencies",
        &dep_line,
    )
    .ok_or_else(|| registry_err("Cargo.toml"))?;
    let entry = format!(
        "        Entry {{\n            name: {name:?},\n            options: &[],\n            build: |_| Box::new({krate}::{ty}),\n        }},\n",
        name = req.name,
        krate = crate_name.replace('-', "_"),
        ty = type_name(&req.name),
    );
    let reg_lib =
        insert_before_marker(&read(REGISTRY_LIB)?, "// berry-blocks:mint-entries", &entry)
            .ok_or_else(|| registry_err("list"))?;
    let changes = vec![
        Change {
            kind: ChangeKind::Create,
            path: format!("plugins/{0}/{0}.plugin_praxis.deed", req.name),
            note: "manifest",
            content: manifest(req, &id, &crate_name),
            added_lines: vec![],
        },
        Change {
            kind: ChangeKind::Create,
            path: format!("crates/{crate_name}/Cargo.toml"),
            note: "the plugin's crate",
            content: crate_toml(req, &crate_name),
            added_lines: vec![],
        },
        Change {
            kind: ChangeKind::Create,
            path: format!("crates/{crate_name}/src/lib.rs"),
            note: "a working block that renders claimed fences as plain code",
            content: crate_lib(req),
            added_lines: vec![],
        },
        Change {
            kind: ChangeKind::Create,
            path: format!("crates/{crate_name}/tests/conformance.rs"),
            note: "BerryWiki pages must stay byte-identical",
            content: crate_test(req),
            added_lines: vec![],
        },
        Change {
            kind: ChangeKind::Modify,
            path: "Cargo.toml".into(),
            note: "adds the crate to the workspace",
            content: new_workspace,
            added_lines: vec![format!("    \"crates/{crate_name}\",")],
        },
        Change {
            kind: ChangeKind::Modify,
            path: REGISTRY_TOML.into(),
            note: "lets the registry build the plugin",
            content: reg_toml,
            added_lines: vec![dep_line.trim_end().to_string()],
        },
        Change {
            kind: ChangeKind::Modify,
            path: REGISTRY_LIB.into(),
            note: "registers the plugin so a wiki configuration can turn it on",
            content: reg_lib,
            added_lines: entry.lines().map(str::to_string).collect(),
        },
    ];
    let mut h = Sha256::new();
    for c in &changes {
        h.update(format!("{:?}\0{}\0{}\0", c.kind, c.path, c.content).as_bytes());
    }
    let digest = h.finalize().iter().map(|b| format!("{b:02x}")).collect();
    Ok(Plan {
        plugin_id: id,
        crate_name,
        changes,
        digest,
    })
}

/// Writes the plan, if and only if it still has the previewed digest.
///
/// Every create target is checked before anything is written. Files are written
/// to a temporary name in the same directory and renamed into place.
pub fn apply(req: &MintRequest, root: &Path, previewed_digest: &str) -> Result<Plan, MintError> {
    let plan = plan(req, root)?;
    if plan.digest != previewed_digest {
        return Err(MintError::Stale);
    }
    for c in &plan.changes {
        if c.kind == ChangeKind::Create && root.join(&c.path).exists() {
            return Err(MintError::Exists(c.path.clone()));
        }
    }
    let mut written = Vec::new();
    for c in &plan.changes {
        let target: PathBuf = root.join(&c.path);
        let result = (|| {
            if let Some(dir) = target.parent() {
                fs::create_dir_all(dir)?;
            }
            let tmp = target.with_extension("berry-blocks-tmp");
            fs::write(&tmp, &c.content)?;
            match c.kind {
                ChangeKind::Create => install_new(&tmp, &target),
                ChangeKind::Modify => fs::rename(&tmp, &target),
            }
        })();
        if let Err(e) = result {
            if e.kind() == std::io::ErrorKind::AlreadyExists && written.is_empty() {
                return Err(MintError::Exists(c.path.clone()));
            }
            return Err(MintError::Io {
                written,
                error: format!("{}: {e}", c.path),
            });
        }
        written.push(c.path.clone());
    }
    Ok(plan)
}

/// Installs a fully written temporary file at `target` only if nothing is
/// there: a hard link fails atomically with `AlreadyExists`, so a file created
/// after the preflight check is never replaced, and the target is never left
/// half-written. The temporary file is removed either way.
fn install_new(tmp: &Path, target: &Path) -> std::io::Result<()> {
    let linked = fs::hard_link(tmp, target);
    let _ = fs::remove_file(tmp);
    linked
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scratch repository holding only a workspace Cargo.toml.
    fn scratch() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "berry-blocks-mint-test-{}-{}",
            std::process::id(),
            rand_suffix()
        ));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("Cargo.toml"), "[workspace]\nmembers = [\n    \"crates/berry-blocks-host\",\n    \"crates/berry-blocks-cli\",\n]\n").unwrap();
        fs::create_dir_all(dir.join("crates/berry-blocks-registry/src")).unwrap();
        fs::write(
            dir.join(REGISTRY_TOML),
            "[dependencies]\n# berry-blocks:mint-dependencies\n",
        )
        .unwrap();
        fs::write(
            dir.join(REGISTRY_LIB),
            "    vec![\n        // berry-blocks:mint-entries\n    ]\n",
        )
        .unwrap();
        dir
    }

    /// A per-call suffix so parallel tests never share a directory.
    fn rand_suffix() -> u64 {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        N.fetch_add(1, Ordering::Relaxed)
    }

    /// A valid request for the ProgBlocks example.
    fn req() -> MintRequest {
        MintRequest {
            name: "progblocks".into(),
            display: "ProgBlocks".into(),
            claims: "variant".into(),
            run_key: "group".into(),
            enhanced: true,
            licence: "MPL-2.0".into(),
        }
    }

    #[test]
    /// The ID matches the one already recorded for ProgBlocks.
    fn uuid_matches_the_recorded_progblocks_id() {
        assert_eq!(
            uuid_v8_profile_c("berry-blocks-plugin", "progblocks"),
            "c39f8b97-e19b-88cf-bc7f-15c773f72113"
        );
    }

    #[test]
    /// Every bad field is reported at once.
    fn reports_all_field_errors() {
        let dir = scratch();
        let bad = MintRequest {
            name: "Bad Name".into(),
            display: "".into(),
            claims: "1x".into(),
            run_key: "1x".into(),
            enhanced: false,
            licence: "GPL".into(),
        };
        let fields: Vec<_> = validate(&bad, &dir).into_iter().map(|e| e.field).collect();
        assert_eq!(
            fields,
            vec!["name", "display", "claims", "run_key", "licence"]
        );
    }

    #[test]
    /// Planning writes nothing; applying writes exactly the plan.
    fn plan_writes_nothing_and_apply_writes_the_plan() {
        let dir = scratch();
        let before = fs::read_to_string(dir.join("Cargo.toml")).unwrap();
        let p = plan(&req(), &dir).unwrap();
        assert_eq!(p.changes.len(), 7);
        assert_eq!(fs::read_to_string(dir.join("Cargo.toml")).unwrap(), before);
        assert!(!dir.join("plugins").exists());
        apply(&req(), &dir, &p.digest).unwrap();
        for c in &p.changes {
            assert_eq!(
                fs::read_to_string(dir.join(&c.path)).unwrap(),
                c.content,
                "{}",
                c.path
            );
        }
        assert!(fs::read_to_string(dir.join("Cargo.toml"))
            .unwrap()
            .contains("\"crates/berry-blocks-progblocks\","));
    }

    #[test]
    /// A different digest, or an edited field, is refused without writing.
    fn stale_preview_is_refused() {
        let dir = scratch();
        let p = plan(&req(), &dir).unwrap();
        let mut edited = req();
        edited.display = "Something else".into();
        assert_eq!(apply(&edited, &dir, &p.digest), Err(MintError::Stale));
        assert!(!dir.join("plugins").exists());
    }

    #[test]
    /// Minting the same name twice is refused as invalid the second time.
    fn second_mint_of_same_name_is_refused() {
        let dir = scratch();
        let p = plan(&req(), &dir).unwrap();
        apply(&req(), &dir, &p.digest).unwrap();
        match plan(&req(), &dir) {
            Err(MintError::Invalid(errs)) => assert_eq!(errs[0].field, "name"),
            other => panic!("expected refusal, got {other:?}"),
        }
    }

    #[test]
    /// A file that appears at a create target is never replaced, and no
    /// temporary file is left behind.
    fn install_never_replaces_an_existing_file() {
        let dir = scratch();
        let tmp = dir.join("new.berry-blocks-tmp");
        let target = dir.join("target.txt");
        fs::write(&target, "someone else's").unwrap();
        fs::write(&tmp, "ours").unwrap();
        let err = install_new(&tmp, &target).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read_to_string(&target).unwrap(), "someone else's");
        assert!(!tmp.exists());
        fs::write(&tmp, "ours").unwrap();
        install_new(&tmp, &dir.join("fresh.txt")).unwrap();
        assert_eq!(fs::read_to_string(dir.join("fresh.txt")).unwrap(), "ours");
    }

    #[test]
    /// Quotes and backslashes in the display name cannot break the generated files.
    fn display_name_is_escaped_everywhere() {
        let dir = scratch();
        let mut r = req();
        r.display = "Say \"hi\" \\ there".into();
        let p = plan(&r, &dir).unwrap();
        assert!(p.changes[0]
            .content
            .contains(":display \"Say \\\"hi\\\" \\\\ there\""));
        assert!(p.changes[1]
            .content
            .contains("description = \"Say \\\"hi\\\" \\\\ there: a berry-blocks block.\""));
    }
}
