// SPDX-License-Identifier: MPL-2.0
//! Harness: run every check against a configured wiki and write a report.
//!
//! Each check ends as pass, fail or **not run**. A check that could not run
//! (no Chromium, pins not fetched) says why and is never counted as a pass.
//! The report is `reports/<config>-<date>.kyaml`; the wizard's results page
//! reads it back, so what is shown is what was recorded.
//!
//! Rust checks: BerryWiki conformance, escaping of hostile fences, no script in
//! the static profile, pins agree. Browser checks (via
//! `tools/harness-browser.mjs` and a local Chromium): axe in light and dark,
//! readable without script, every `<prog-block>` upgrades.

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use berry_blocks_configure::WikiConfig;
use berry_blocks_host::{render_page, Block, Profile};

/// How one check ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The check ran and held.
    Pass,
    /// The check ran and found a problem.
    Fail,
    /// The check could not run; the evidence says why. Never a pass.
    NotRun,
}

impl Outcome {
    /// The word written in the report.
    pub fn as_str(self) -> &'static str {
        match self {
            Outcome::Pass => "pass",
            Outcome::Fail => "fail",
            Outcome::NotRun => "not-run",
        }
    }
    /// Reads the word written in the report.
    fn parse(s: &str) -> Option<Self> {
        match s {
            "pass" => Some(Outcome::Pass),
            "fail" => Some(Outcome::Fail),
            "not-run" => Some(Outcome::NotRun),
            _ => None,
        }
    }
}

/// One check's result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckResult {
    /// Stable key, e.g. `conformance`.
    pub key: String,
    /// Plain-words name.
    pub title: String,
    /// How it ended.
    pub outcome: Outcome,
    /// What was found.
    pub evidence: String,
}

/// A harness run, as written to `reports/`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Report {
    /// Configuration name.
    pub config: String,
    /// UTC date, `YYYY-MM-DD`.
    pub date: String,
    /// Results, in the order the checks ran.
    pub checks: Vec<CheckResult>,
}

/// What a failing check usually means and what to change, by key.
pub fn advice(key: &str) -> &'static str {
    match key {
        "conformance" => "A plugin claimed or changed a block on a page that has no claimed fences. Check its claims() and continues() logic; only fences an author explicitly marked may change.",
        "escaping" => "A plugin wrote text from the page into the output without escaping it. Pass every variant name, label and line of code through escape_html.",
        "static-no-script" => "A plugin added script to the static profile. The static profile must be plain HTML; move the script to the enhanced profile's assets.",
        "pins" => "The checked-out code is not the pinned commit. Run scripts/fetch-pins.sh, or provision the plugin again.",
        "axe-light" | "axe-dark" => "The rendered pages have an accessibility problem. The evidence names the rule and the element; fix the plugin's markup or styles.",
        "no-js" => "A variant group is empty or unreadable without script. The static markup must show every variant and its code.",
        "upgrade" => "A <prog-block> did not upgrade when script ran. Check that ProgBlocks is provisioned and that the enhanced profile loads its module.",
        _ => "See the evidence.",
    }
}

/// Keeps report text inside KYAML string quotes: no `"` or `\` survives.
fn clean(s: &str) -> String {
    s.replace('"', "'")
        .replace('\\', "/")
        .replace(['\n', '\t'], " ")
}

impl Report {
    /// Writes the canonical KYAML form.
    pub fn render(&self) -> String {
        let mut out = String::from("# SPDX-License-Identifier: MPL-2.0\n# berry-blocks harness report, written by the wizard's Harness step.\n{\n");
        out.push_str(&format!(
            "  config: \"{}\",\n  date: \"{}\",\n  checks: [\n",
            clean(&self.config),
            clean(&self.date)
        ));
        for c in &self.checks {
            out.push_str(&format!(
                "    {{\n      key: \"{}\",\n      title: \"{}\",\n      outcome: \"{}\",\n      evidence: \"{}\",\n    }},\n",
                clean(&c.key),
                clean(&c.title),
                c.outcome.as_str(),
                clean(&c.evidence)
            ));
        }
        out.push_str("  ],\n}\n");
        out
    }

    /// Reads the form [`Report::render`] writes; refuses anything else.
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut lines = text.lines().filter(|l| !l.starts_with('#'));
        let mut next = || {
            lines
                .next()
                .ok_or_else(|| "unexpected end of report".to_string())
        };
        let field = |line: &str, indent: &str, key: &str| -> Result<String, String> {
            line.strip_prefix(&format!("{indent}{key}: \""))
                .and_then(|l| l.strip_suffix("\","))
                .map(str::to_string)
                .ok_or_else(|| format!("expected {key}, got `{line}`"))
        };
        if next()? != "{" {
            return Err("expected `{`".into());
        }
        let config = field(next()?, "  ", "config")?;
        let date = field(next()?, "  ", "date")?;
        if next()? != "  checks: [" {
            return Err("expected checks".into());
        }
        let mut checks = Vec::new();
        loop {
            let l = next()?;
            if l == "  ]," {
                break;
            }
            if l != "    {" {
                return Err(format!("unexpected `{l}`"));
            }
            let key = field(next()?, "      ", "key")?;
            let title = field(next()?, "      ", "title")?;
            let outcome =
                Outcome::parse(&field(next()?, "      ", "outcome")?).ok_or("unknown outcome")?;
            let evidence = field(next()?, "      ", "evidence")?;
            if next()? != "    }," {
                return Err("expected `    },`".into());
            }
            checks.push(CheckResult {
                key,
                title,
                outcome,
                evidence,
            });
        }
        if next()? != "}" {
            return Err("expected `}`".into());
        }
        Ok(Self {
            config,
            date,
            checks,
        })
    }

    /// Counts of (pass, fail, not run).
    pub fn tally(&self) -> (usize, usize, usize) {
        let n = |o| self.checks.iter().filter(|c| c.outcome == o).count();
        (n(Outcome::Pass), n(Outcome::Fail), n(Outcome::NotRun))
    }
}

/// Today's UTC date as `YYYY-MM-DD` (civil-from-days, no dependencies).
pub fn today() -> String {
    let days = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() / 86_400)
        .unwrap_or(0) as i64;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!("{y:04}-{m:02}-{d:02}")
}

/// Why a harness run could not start at all.
#[derive(Debug, PartialEq, Eq)]
pub struct HarnessError(pub String);

impl fmt::Display for HarnessError {
    /// The reason, as written.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for HarnessError {}

/// The `claims` key a plugin's manifest declares (`… info string has KEY=`).
fn claims_key(root: &Path, plugin: &str) -> Option<String> {
    let text = fs::read_to_string(
        root.join("plugins")
            .join(plugin)
            .join(format!("{plugin}.plugin_praxis.deed")),
    )
    .ok()?;
    let after = text.split("info string has ").nth(1)?;
    let key: String = after
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    (!key.is_empty()).then_some(key)
}

/// A unique scratch directory for rendered sites.
fn scratch() -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let d = std::env::temp_dir().join(format!(
        "berry-blocks-harness-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&d);
    d
}

/// A Chromium to drive: `CHROMIUM_PATH`, else one on PATH.
pub fn find_chromium() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("CHROMIUM_PATH") {
        let p = PathBuf::from(p);
        return p.is_file().then_some(p);
    }
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        for name in ["google-chrome", "chromium", "chromium-browser"] {
            let c = dir.join(name);
            if c.is_file() {
                return Some(c);
            }
        }
    }
    None
}

/// Runs every check for configuration `name` and writes the report.
pub fn run(
    root: &Path,
    name: &str,
    chromium: Option<&Path>,
) -> Result<(Report, String), HarnessError> {
    let cfg_path = root.join(berry_blocks_configure::config_path(name));
    let (config, blocks) = berry_blocks_configure::load(&cfg_path).map_err(HarnessError)?;
    let refs: Vec<&dyn Block> = blocks.iter().map(|b| b.as_ref()).collect();
    let mut checks = vec![
        conformance(root, &refs),
        escaping(root, &config, &refs),
        static_no_script(root, &config, &refs),
        pins(root, &config),
    ];
    checks.extend(browser(root, &config, &refs, chromium));
    let report = Report {
        config: name.to_string(),
        date: today(),
        checks,
    };
    let rel = format!("reports/{name}-{}.kyaml", report.date);
    let target = root.join(&rel);
    fs::create_dir_all(target.parent().unwrap())
        .and_then(|_| fs::write(&target, report.render()))
        .map_err(|e| HarnessError(format!("{rel}: {e}")))?;
    Ok((report, rel))
}

/// Builds a check result.
fn result(key: &str, title: &str, outcome: Outcome, evidence: impl Into<String>) -> CheckResult {
    CheckResult {
        key: key.into(),
        title: title.into(),
        outcome,
        evidence: evidence.into(),
    }
}

/// Every BerryWiki fixture page must render byte-identically, both profiles.
fn conformance(root: &Path, blocks: &[&dyn Block]) -> CheckResult {
    const T: &str = "BerryWiki pages stay identical";
    let dir = root.join("vendor/berrywiki/fixtures/test-wiki");
    let pages = berry_blocks_configure::pages(&dir);
    if pages.is_empty() {
        return result(
            "conformance",
            T,
            Outcome::NotRun,
            "BerryWiki's fixture wiki is not fetched; run scripts/fetch-pins.sh",
        );
    }
    for page in &pages {
        let md = fs::read_to_string(page).unwrap_or_default();
        let expected = berrywiki_render::render_markdown(&md);
        for profile in [Profile::Static, Profile::Enhanced] {
            match render_page(&md, blocks, profile) {
                Ok(r) if r.html == expected => {}
                _ => {
                    return result(
                        "conformance",
                        T,
                        Outcome::Fail,
                        format!(
                            "{} changed under the {profile:?} profile",
                            page.file_name().unwrap().to_string_lossy()
                        ),
                    )
                }
            }
        }
    }
    result(
        "conformance",
        T,
        Outcome::Pass,
        format!(
            "{} BerryWiki fixture pages, static and enhanced",
            pages.len()
        ),
    )
}

/// Hostile text in a claimed fence must come out as text.
fn escaping(root: &Path, config: &WikiConfig, blocks: &[&dyn Block]) -> CheckResult {
    const T: &str = "Untrusted text stays text";
    let keys: Vec<String> = config
        .plugins
        .iter()
        .filter_map(|p| claims_key(root, &p.name))
        .collect();
    if keys.is_empty() {
        return result(
            "escaping",
            T,
            Outcome::NotRun,
            "no plugin manifest declares what it claims",
        );
    }
    let mut claimed = 0;
    for key in &keys {
        let md = format!("```sh {key}=\"<img src=x onerror=alert(1)>\" group=g label=\"<i>x</i>\"\n<script>alert(1)</script>\n```\n");
        for profile in [Profile::Static, Profile::Enhanced] {
            let Ok(r) = render_page(&md, blocks, profile) else {
                return result(
                    "escaping",
                    T,
                    Outcome::Fail,
                    format!("rendering a hostile {key}= fence failed under {profile:?}"),
                );
            };
            claimed += r.runs;
            let lower = r.html.to_ascii_lowercase();
            if lower.contains("<img") || lower.contains("<script") || lower.contains("<i>") {
                return result(
                    "escaping",
                    T,
                    Outcome::Fail,
                    format!("markup from a {key}= fence reached the {profile:?} output"),
                );
            }
        }
    }
    if claimed == 0 {
        return result(
            "escaping",
            T,
            Outcome::Fail,
            "no plugin claimed the hostile test fences, so escaping was not exercised",
        );
    }
    result("escaping", T, Outcome::Pass, format!("<img>, <script> and <i> in names, labels and code stayed text ({} fence key(s), both profiles)", keys.len()))
}

/// The configured wiki rendered with the static profile has no script.
fn static_no_script(root: &Path, config: &WikiConfig, blocks: &[&dyn Block]) -> CheckResult {
    const T: &str = "Static pages have no script";
    let pages = berry_blocks_configure::pages(&config.wiki_dir(root));
    for page in &pages {
        let md = fs::read_to_string(page).unwrap_or_default();
        match render_page(&md, blocks, Profile::Static) {
            Ok(r) if !r.html.to_ascii_lowercase().contains("<script") => {}
            Ok(_) => {
                return result(
                    "static-no-script",
                    T,
                    Outcome::Fail,
                    format!(
                        "{} contains <script>",
                        page.file_name().unwrap().to_string_lossy()
                    ),
                )
            }
            Err(e) => {
                return result(
                    "static-no-script",
                    T,
                    Outcome::Fail,
                    format!("{}: {e}", page.file_name().unwrap().to_string_lossy()),
                )
            }
        }
    }
    result(
        "static-no-script",
        T,
        Outcome::Pass,
        format!("{} pages checked", pages.len()),
    )
}

/// Every pinned plugin's checkout is at its pin, and BerryWiki's Cargo rev matches.
fn pins(root: &Path, config: &WikiConfig) -> CheckResult {
    const T: &str = "Pins agree";
    let Ok(text) = fs::read_to_string(root.join("pins.kyaml")) else {
        return result("pins", T, Outcome::NotRun, "pins.kyaml not found");
    };
    let Ok(pins) = berry_blocks_provision::Pins::parse(&text) else {
        return result(
            "pins",
            T,
            Outcome::Fail,
            "pins.kyaml is not in the expected shape",
        );
    };
    let mut seen = Vec::new();
    for p in &config.plugins {
        let Some(pin) = pins.get(&p.name) else {
            continue;
        };
        let head = Command::new("git")
            .arg("-C")
            .arg(root.join("vendor").join(&p.name))
            .args(["rev-parse", "HEAD"])
            .output();
        match head {
            Ok(o)
                if o.status.success()
                    && String::from_utf8_lossy(&o.stdout).trim() == pin.commit =>
            {
                seen.push(format!("{} at {}", p.name, &pin.commit[..7]))
            }
            Ok(o) if o.status.success() => {
                return result(
                    "pins",
                    T,
                    Outcome::Fail,
                    format!(
                        "vendor/{} is at {}, pinned {}",
                        p.name,
                        String::from_utf8_lossy(&o.stdout)
                            .trim()
                            .chars()
                            .take(7)
                            .collect::<String>(),
                        &pin.commit[..7]
                    ),
                )
            }
            _ => {
                return result(
                    "pins",
                    T,
                    Outcome::NotRun,
                    format!(
                        "vendor/{} is not checked out; run scripts/fetch-pins.sh",
                        p.name
                    ),
                )
            }
        }
    }
    let cargo = fs::read_to_string(root.join("Cargo.toml")).unwrap_or_default();
    if let Some(bw) = pins.get("berrywiki") {
        if !cargo.contains(&format!("rev = \"{}\"", bw.commit)) {
            return result(
                "pins",
                T,
                Outcome::Fail,
                "the berrywiki-render rev in Cargo.toml differs from pins.kyaml",
            );
        }
        seen.push(format!("berrywiki at {}", &bw.commit[..7]));
    }
    result(
        "pins",
        T,
        Outcome::Pass,
        if seen.is_empty() {
            "no pinned plugins in this configuration".to_string()
        } else {
            seen.join(", ")
        },
    )
}

/// The four browser checks, or four "not run" results saying why.
fn browser(
    root: &Path,
    config: &WikiConfig,
    blocks: &[&dyn Block],
    chromium: Option<&Path>,
) -> Vec<CheckResult> {
    const KEYS: [(&str, &str); 4] = [
        ("axe-light", "Accessible in light mode"),
        ("axe-dark", "Accessible in dark mode"),
        ("no-js", "Readable without script"),
        ("upgrade", "Upgrades with script"),
    ];
    let not_run = |why: &str| {
        KEYS.iter()
            .map(|(k, t)| result(k, t, Outcome::NotRun, why))
            .collect::<Vec<_>>()
    };
    let Some(chromium) = chromium else {
        return not_run(
            "no Chromium found: set CHROMIUM_PATH or install google-chrome or chromium",
        );
    };
    if !root.join("node_modules/axe-core").is_dir()
        || !root.join("node_modules/playwright-core").is_dir()
    {
        return not_run("browser tools not installed: run bun install");
    }
    let dir = scratch();
    let (st, en) = (dir.join("static"), dir.join("enhanced"));
    let wiki = config.wiki_dir(root);
    let src = root.join("vendor/progblocks/src");
    let rendered = berry_blocks_site::render_site(&wiki, &st, blocks, Profile::Static, &src)
        .and_then(|_| berry_blocks_site::render_site(&wiki, &en, blocks, Profile::Enhanced, &src));
    if let Err(e) = rendered {
        let _ = fs::remove_dir_all(&dir);
        return not_run(&format!("could not render the wiki: {e}"));
    }
    let out = Command::new("bun")
        .current_dir(root)
        .arg("tools/harness-browser.mjs")
        .arg(&st)
        .arg(&en)
        .env("CHROMIUM_PATH", chromium)
        .output();
    let _ = fs::remove_dir_all(&dir);
    let out = match out {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).into_owned(),
        Ok(o) => {
            return not_run(&format!(
                "the browser run failed: {}",
                String::from_utf8_lossy(&o.stderr)
                    .lines()
                    .last()
                    .unwrap_or("no output")
            ))
        }
        Err(e) => return not_run(&format!("could not start bun: {e}")),
    };
    // One line per check: `key<TAB>pass|fail<TAB>evidence`.
    KEYS.iter()
        .map(|(k, t)| {
            let line = out.lines().find(|l| l.split('\t').next() == Some(k));
            match line.map(|l| l.splitn(3, '\t').collect::<Vec<_>>()) {
                Some(parts) if parts.len() == 3 => result(
                    k,
                    t,
                    if parts[1] == "pass" {
                        Outcome::Pass
                    } else {
                        Outcome::Fail
                    },
                    parts[2],
                ),
                _ => result(
                    k,
                    t,
                    Outcome::NotRun,
                    "the browser run did not report this check",
                ),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    /// A report round-trips, and quotes in evidence cannot break the file.
    fn report_round_trips() {
        let r = Report {
            config: "lab".into(),
            date: "2026-10-05".into(),
            checks: vec![
                result("pins", "Pins agree", Outcome::Pass, "progblocks at ee10c66"),
                result(
                    "no-js",
                    "Readable without script",
                    Outcome::NotRun,
                    "no \"Chromium\" \\ found",
                ),
            ],
        };
        let back = Report::parse(&r.render()).unwrap();
        assert_eq!(back.checks[0], r.checks[0]);
        assert_eq!(back.checks[1].evidence, "no 'Chromium' / found");
        assert_eq!(back.tally(), (1, 0, 1));
    }

    use berry_blocks_host::{Asset, Fence, FenceRun};

    /// A misbehaving block for planted failures.
    struct Bad {
        /// Claim every fence, not just marked ones.
        greedy: bool,
        /// Write fence text without escaping.
        leaky: bool,
        /// Emit a script even in the static profile.
        scripty: bool,
    }

    impl Block for Bad {
        /// Test name.
        fn name(&self) -> &'static str {
            "bad"
        }
        /// Claims `zkey=` fences, or every fence when greedy.
        fn claims(&self, f: &Fence) -> bool {
            self.greedy || f.get("zkey").is_some()
        }
        /// Every adjacent claimed fence continues the run.
        fn continues(&self, _: &Fence, _: &Fence) -> bool {
            true
        }
        /// Renders the run, possibly badly.
        fn render(&self, run: &FenceRun, _: Profile) -> String {
            let mut out = String::new();
            for f in &run.fences {
                let text = format!("{} {}", f.get("zkey").unwrap_or(""), f.code);
                out.push_str(&if self.leaky {
                    text
                } else {
                    berry_blocks_host::escape_html(&text)
                });
            }
            if self.scripty {
                out.push_str("<script>x()</script>");
            }
            out
        }
        /// No assets.
        fn assets(&self, _: Profile) -> Vec<Asset> {
            Vec::new()
        }
    }

    /// A root with a manifest declaring `zkey`, a one-page wiki, and the
    /// pinned BerryWiki fixtures linked from this checkout.
    fn root() -> (PathBuf, WikiConfig) {
        let r = scratch();
        fs::create_dir_all(r.join("plugins/bad")).unwrap();
        fs::write(
            r.join("plugins/bad/bad.plugin_praxis.deed"),
            "(praxis-deed (plugin :claims \"fenced code blocks whose info string has zkey=\"))\n",
        )
        .unwrap();
        fs::create_dir_all(r.join("wiki")).unwrap();
        fs::write(r.join("wiki/Page.md"), "```sh zkey=a\nhello\n```\n").unwrap();
        let cfg = WikiConfig {
            header: vec![],
            wiki: "wiki".into(),
            profile: "static".into(),
            plugins: vec![berry_blocks_configure::PluginConfig {
                name: "bad".into(),
                options: vec![],
            }],
        };
        (r, cfg)
    }

    #[test]
    /// A block that writes page text raw fails the escaping check; a good one passes.
    fn leaky_block_fails_escaping() {
        let (r, cfg) = root();
        let leaky = Bad {
            greedy: false,
            leaky: true,
            scripty: false,
        };
        assert_eq!(escaping(&r, &cfg, &[&leaky]).outcome, Outcome::Fail);
        let good = Bad {
            greedy: false,
            leaky: false,
            scripty: false,
        };
        assert_eq!(escaping(&r, &cfg, &[&good]).outcome, Outcome::Pass);
    }

    #[test]
    /// A block that claims unmarked fences fails conformance (needs fetched pins).
    fn greedy_block_fails_conformance() {
        let (r, _) = root();
        let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../vendor/berrywiki");
        assert!(fixtures.is_dir(), "run scripts/fetch-pins.sh first");
        fs::create_dir_all(r.join("vendor")).unwrap();
        std::os::unix::fs::symlink(fixtures.canonicalize().unwrap(), r.join("vendor/berrywiki"))
            .unwrap();
        let greedy = Bad {
            greedy: true,
            leaky: false,
            scripty: false,
        };
        assert_eq!(conformance(&r, &[&greedy]).outcome, Outcome::Fail);
        let good = Bad {
            greedy: false,
            leaky: false,
            scripty: false,
        };
        assert_eq!(conformance(&r, &[&good]).outcome, Outcome::Pass);
    }

    #[test]
    /// Script in the static profile fails; missing fixtures are "not run", never a pass.
    fn script_and_missing_fixtures() {
        let (r, cfg) = root();
        let scripty = Bad {
            greedy: false,
            leaky: false,
            scripty: true,
        };
        assert_eq!(
            static_no_script(&r, &cfg, &[&scripty]).outcome,
            Outcome::Fail
        );
        let good = Bad {
            greedy: false,
            leaky: false,
            scripty: false,
        };
        assert_eq!(conformance(&r, &[&good]).outcome, Outcome::NotRun);
    }

    #[test]
    /// A checkout at the wrong commit fails the pins check.
    fn wrong_checkout_fails_pins() {
        let (r, cfg) = root();
        let v = r.join("vendor/bad");
        fs::create_dir_all(&v).unwrap();
        let git = |args: &[&str]| {
            assert!(Command::new("git")
                .current_dir(&v)
                .args(args)
                .output()
                .unwrap()
                .status
                .success())
        };
        git(&["init", "-q"]);
        git(&[
            "-c",
            "user.email=t@t",
            "-c",
            "user.name=t",
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "x",
        ]);
        fs::write(r.join("pins.kyaml"), "{\n  bad: {\n    repo: \"https://example.org/x\",\n    commit: \"0000000000000000000000000000000000000000\",\n  },\n}\n").unwrap();
        assert_eq!(pins(&r, &cfg).outcome, Outcome::Fail);
    }

    #[test]
    /// Without a Chromium, all four browser checks are "not run".
    fn no_browser_means_not_run() {
        let (r, cfg) = root();
        let good = Bad {
            greedy: false,
            leaky: false,
            scripty: false,
        };
        let out = browser(&r, &cfg, &[&good], None);
        assert_eq!(out.len(), 4);
        assert!(out.iter().all(|c| c.outcome == Outcome::NotRun));
    }

    #[test]
    /// The date is a real calendar date.
    fn today_is_shaped_like_a_date() {
        let t = today();
        assert_eq!(t.len(), 10);
        assert!(t.starts_with("20"));
        assert_eq!(&t[4..5], "-");
    }
}
