// SPDX-License-Identifier: MPL-2.0
//! Harness screens: run every check against a configured wiki and show the
//! report. Running checks changes nothing except writing the report.

use std::collections::BTreeMap;
use std::fs;

use berry_blocks_harness::{advice, find_chromium, run, Outcome, Report};

use crate::{error_banner, esc, frame, App, Response};

const KICKER: &str = "Step 4 of 4 · Harness";

/// True for a configuration name as Configure writes them.
fn is_name(s: &str) -> bool {
    s.chars().next().is_some_and(|c| c.is_ascii_lowercase())
        && s.len() <= 40
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// True for `YYYY-MM-DD`.
fn is_date(s: &str) -> bool {
    s.len() == 10
        && s.chars().enumerate().all(|(i, c)| {
            if i == 4 || i == 7 {
                c == '-'
            } else {
                c.is_ascii_digit()
            }
        })
}

/// Configuration names, sorted.
fn config_names(app: &App) -> Vec<String> {
    let mut v: Vec<String> = fs::read_dir(app.root.join("wikis"))
        .map(|rd| {
            rd.flatten()
                .filter_map(|e| {
                    e.file_name()
                        .to_str()?
                        .strip_suffix(".kyaml")
                        .map(str::to_string)
                })
                .filter(|n| is_name(n))
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

/// The newest report for a configuration, with its date.
fn latest(app: &App, name: &str) -> Option<(String, Report)> {
    let mut dates: Vec<String> = fs::read_dir(app.root.join("reports"))
        .ok()?
        .flatten()
        .filter_map(|e| {
            e.file_name()
                .to_str()?
                .strip_prefix(&format!("{name}-"))?
                .strip_suffix(".kyaml")
                .map(str::to_string)
        })
        .filter(|d| is_date(d))
        .collect();
    dates.sort();
    let date = dates.pop()?;
    let report = Report::parse(
        &fs::read_to_string(app.root.join(format!("reports/{name}-{date}.kyaml"))).ok()?,
    )
    .ok()?;
    Some((date, report))
}

/// One-line summary of a report, outcome in words.
fn summary(r: &Report) -> String {
    let (pass, fail, not_run) = r.tally();
    let mut s = if fail == 0 {
        format!(
            "<span class=\"outcome pass\">{pass} of {} pass</span>",
            r.checks.len()
        )
    } else {
        format!(
            "<span class=\"outcome fail\">{fail} of {} fail</span>",
            r.checks.len()
        )
    };
    if not_run > 0 {
        s.push_str(&format!(
            " <span class=\"outcome skip\">· {not_run} not run</span>"
        ));
    }
    s
}

/// GET /harness: configurations and their latest results, or the run page for one.
pub(crate) fn get(app: &App, q: &BTreeMap<String, String>) -> Response {
    if let Some(name) = q.get("config") {
        return run_page(app, name);
    }
    let rows: String = config_names(app)
        .iter()
        .map(|n| {
            let last = match latest(app, n) {
                Some((d, r)) => format!("{} <span class=\"evidence\">· <a href=\"/harness/results?config={n}&amp;date={d}\">{d}</a></span>", summary(&r), n = esc(n), d = esc(&d)),
                None => "<span class=\"evidence\">not run yet</span>".into(),
            };
            format!("<tr><th scope=\"row\">{n}</th><td>{last}</td><td><a href=\"/harness?config={n}\">Run checks…</a></td></tr>\n", n = esc(n))
        })
        .collect();
    let body = if rows.is_empty() {
        "<p class=\"lede\">Harnessing runs every check against a configured wiki. Nothing is configured yet: <a href=\"/configure?new\">configure a wiki</a> first.</p>".to_string()
    } else {
        format!("<p class=\"lede\">Harnessing runs every check against a configured wiki and records the result. Running checks changes nothing except writing the report.</p>\n<table class=\"plugins-table\"><caption class=\"visually-hidden\">Configured wikis and their latest checks</caption><thead><tr><th scope=\"col\">Configuration</th><th scope=\"col\">Latest checks</th><th scope=\"col\">Action</th></tr></thead><tbody>\n{rows}</tbody></table>")
    };
    Response::html(frame(app, "Harness", KICKER, Some(3), None, &body))
}

/// The page that says which checks will run, then runs them.
fn run_page(app: &App, name: &str) -> Response {
    if !config_names(app).iter().any(|n| n == name) {
        return Response::status(
            404,
            frame(
                app,
                "Harness",
                KICKER,
                Some(3),
                None,
                &error_banner(
                    "No configuration by that name",
                    &[],
                    "<p><a href=\"/harness\">Back to Harness</a>.</p>",
                ),
            ),
        );
    }
    let browser = match find_chromium() {
        Some(p) => format!("<div class=\"notice\" role=\"status\"><p>Browser checks will use <span class=\"mono\">{}</span>.</p></div>", esc(&p.display().to_string())),
        None => "<div class=\"draft-banner\" role=\"status\"><h2>No Chromium found</h2><p>The four browser checks will be recorded as <strong>not run</strong>, not as passed. Set <span class=\"mono\">CHROMIUM_PATH</span> before starting the wizard to run them.</p></div>".into(),
    };
    let checks = [
        ("BerryWiki pages stay identical", "Every page of BerryWiki's fixture wiki renders byte-for-byte as BerryWiki renders it, with this configuration's plugins on."),
        ("Untrusted text stays text", "Markup in variant names, labels and code comes out as text."),
        ("Static pages have no script", "Not one <script> in any page rendered with the static profile."),
        ("Pins agree", "Each plugin's checked-out code is at its pinned commit."),
        ("Accessible in light mode", "axe-core, WCAG 2.0–2.2 A and AA plus best practice, in a real browser."),
        ("Accessible in dark mode", "The same audit with the dark colour scheme."),
        ("Readable without script", "With JavaScript off, every variant and its code is visible."),
        ("Upgrades with script", "With JavaScript on, every <prog-block> upgrades."),
    ];
    let list: String = checks
        .iter()
        .map(|(t, h)| {
            format!(
                "<li><strong>{}</strong> <span class=\"evidence\">· {}</span></li>",
                esc(t),
                esc(h)
            )
        })
        .collect();
    let body = format!("<p class=\"lede\">These checks run against <span class=\"mono\">wikis/{n}.kyaml</span>. Running them changes nothing except writing <span class=\"mono\">reports/{n}-&lt;date&gt;.kyaml</span>. The browser checks take about half a minute.</p>\n{browser}\n<ul>{list}</ul>\n<form method=\"post\" action=\"/harness/run\"><input type=\"hidden\" name=\"config\" value=\"{n}\"><div class=\"actions\"><button class=\"btn\" type=\"submit\">Run checks</button><a class=\"cancel\" href=\"/harness\">Cancel</a></div></form>", n = esc(name));
    Response::html(frame(
        app,
        &format!("Harness {name}"),
        KICKER,
        Some(3),
        None,
        &body,
    ))
}

/// POST /harness/run: run the checks, save the report, show it.
pub(crate) fn post(app: &App, f: &BTreeMap<String, String>) -> Response {
    let name = f.get("config").cloned().unwrap_or_default();
    if !is_name(&name) || !config_names(app).contains(&name) {
        return Response::status(
            404,
            frame(
                app,
                "Harness",
                KICKER,
                Some(3),
                None,
                &error_banner("No configuration by that name", &[], ""),
            ),
        );
    }
    match run(&app.root, &name, find_chromium().as_deref()) {
        Ok((report, _)) => Response::see_other(format!(
            "/harness/results?config={name}&date={}",
            report.date
        )),
        Err(e) => Response::status(
            500,
            frame(
                app,
                "Harness",
                KICKER,
                Some(3),
                None,
                &error_banner(
                    "The checks could not run",
                    &[],
                    &format!("<p>{}</p><p>No report was written.</p>", esc(&e.0)),
                ),
            ),
        ),
    }
}

/// GET /harness/results: the saved report, read back from disk.
pub(crate) fn results(app: &App, q: &BTreeMap<String, String>) -> Response {
    let (name, date) = (
        q.get("config").cloned().unwrap_or_default(),
        q.get("date").cloned().unwrap_or_default(),
    );
    let report = (is_name(&name) && is_date(&date))
        .then(|| fs::read_to_string(app.root.join(format!("reports/{name}-{date}.kyaml"))).ok())
        .flatten()
        .and_then(|t| Report::parse(&t).ok());
    let Some(r) = report else {
        return Response::status(
            404,
            frame(
                app,
                "Harness",
                KICKER,
                Some(3),
                None,
                &error_banner("No report by that name and date", &[], ""),
            ),
        );
    };
    let (pass, fail, not_run) = r.tally();
    let n = r.checks.len();
    let mut banner = if fail > 0 {
        let links: String = r
            .checks
            .iter()
            .filter(|c| c.outcome == Outcome::Fail)
            .map(|c| {
                format!(
                    "<li><a href=\"#check-{k}\">{t}</a>: {e}</li>",
                    k = esc(&c.key),
                    t = esc(&c.title),
                    e = esc(&c.evidence)
                )
            })
            .collect();
        format!("<div class=\"error-banner\" role=\"alert\" aria-labelledby=\"err-h\"><h2 id=\"err-h\">{fail} of {n} checks fail</h2><ul>{links}</ul></div>")
    } else if not_run == 0 {
        format!("<div class=\"notice\" role=\"status\"><h2>All {n} checks pass</h2><p>{name} is safe to use with its current configuration.</p></div>", name = esc(&name))
    } else {
        format!("<div class=\"notice\" role=\"status\"><h2>{pass} of {n} checks pass</h2><p>None failed.</p></div>")
    };
    if not_run > 0 {
        banner.push_str(&format!("<div class=\"draft-banner\" role=\"status\"><h2>{not_run} check(s) were not run</h2><p>A check that did not run is not a pass. The evidence below says why each one did not run.</p></div>"));
    }
    let rows: String = r
        .checks
        .iter()
        .map(|c| {
            let o = match c.outcome {
                Outcome::Pass => "<span class=\"outcome pass\">✓ Pass</span>",
                Outcome::Fail => "<span class=\"outcome fail\">✗ Fail</span>",
                Outcome::NotRun => "<span class=\"outcome skip\">– Not run</span>",
            };
            format!("<tr id=\"check-{k}\"><th scope=\"row\">{t}</th><td>{o}</td><td class=\"evidence\">{e}</td></tr>\n", k = esc(&c.key), t = esc(&c.title), e = esc(&c.evidence))
        })
        .collect();
    let help: String = r
        .checks
        .iter()
        .filter(|c| c.outcome == Outcome::Fail)
        .map(|c| format!("<h3>{}</h3><p>{}</p>", esc(&c.title), esc(advice(&c.key))))
        .collect();
    let help = if help.is_empty() {
        String::new()
    } else {
        format!("<h2>What this usually means</h2>\n{help}")
    };
    let body = format!("{banner}\n<table class=\"checks\"><caption class=\"visually-hidden\">Check results</caption><thead><tr><th scope=\"col\">Check</th><th scope=\"col\">Result</th><th scope=\"col\">Evidence</th></tr></thead><tbody>\n{rows}</tbody></table>\n<p>Report saved as <span class=\"mono\">reports/{name}-{date}.kyaml</span>.</p>\n{help}\n<div class=\"actions\"><a class=\"btn secondary\" href=\"/harness?config={name}\">Run again</a><a class=\"cancel\" href=\"/harness\">Back to Harness</a></div>", name = esc(&name), date = esc(&date));
    Response::html(frame(
        app,
        &format!("Harness {name}"),
        KICKER,
        Some(3),
        None,
        &body,
    ))
}
