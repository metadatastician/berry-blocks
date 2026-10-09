// SPDX-License-Identifier: MPL-2.0
//! Provision screens: pin a minted plugin's upstream code to one commit.
//! Same frame and behaviour as Mint (design/wizard/PATTERN.adoc).

use std::collections::BTreeMap;

use berry_blocks_mint::FieldError;
use berry_blocks_provision::{apply, parse_files, plan, Plan, ProvisionError, ProvisionRequest};

use crate::{error_banner, esc, frame, installed, text_field, App, Card, Installed, Response};

const KICKER: &str = "Step 2 of 4 · Provision";

/// The minted plugin with this name, if any.
fn find(app: &App, name: &str) -> Option<Installed> {
    installed(app).into_iter().find(|p| p.name == name)
}

/// Converts provision field errors to the shared form-field error type.
fn field_errors(errs: Vec<berry_blocks_provision::FieldError>) -> Vec<FieldError> {
    errs.into_iter()
        .map(|e| FieldError {
            field: e.field,
            message: e.message,
        })
        .collect()
}

/// Reads a provision request from form fields.
fn request(form: &BTreeMap<String, String>) -> ProvisionRequest {
    let get = |k: &str| {
        form.get(k)
            .map(|v| v.trim().to_string())
            .unwrap_or_default()
    };
    ProvisionRequest {
        plugin: get("plugin"),
        repo: get("repo"),
        commit: get("commit"),
        files: parse_files(&get("files")),
    }
}

/// The provision form. `digest` is set once a preview with passing checks was shown.
fn form(r: &ProvisionRequest, errors: &[FieldError], digest: Option<&str>) -> String {
    let act = match digest {
        Some(d) => format!("<input type=\"hidden\" name=\"digest\" value=\"{}\"><button class=\"btn secondary\" type=\"submit\" formaction=\"/provision/preview\">Preview again</button><button class=\"btn\" type=\"submit\" formaction=\"/provision\">Provision</button>", esc(d)),
        None => "<button class=\"btn secondary\" type=\"submit\" formaction=\"/provision/preview\">Preview</button><button class=\"btn\" type=\"submit\" disabled>Provision</button>".into(),
    };
    let hint = if digest.is_none() {
        "The action stays unavailable until you have seen a preview whose checks all pass."
    } else {
        "Provisioning writes exactly the changes previewed below. If you change a field, preview again first."
    };
    let files_err = errors.iter().find(|e| e.field == "files");
    let (inv, described, msg) = match files_err {
        Some(e) => (
            " aria-invalid=\"true\"",
            "files-error files-hint",
            format!(
                "<p class=\"field-error\" id=\"files-error\">Error: {}</p>",
                esc(&e.message)
            ),
        ),
        None => ("", "files-hint", String::new()),
    };
    format!(
        "<p class=\"lede\">Provisioning fetches the plugin's upstream code at one exact commit. Pinning to a commit, not a branch, means the plugin never changes until you choose to move the pin.</p>\n<form method=\"post\" action=\"/provision/preview\">\n<input type=\"hidden\" name=\"plugin\" value=\"{plugin}\">\n{repo}{commit}<div class=\"field\"><label for=\"files\">Files it needs</label><input type=\"text\" class=\"mono\" id=\"files\" name=\"files\" value=\"{files}\"{inv} aria-describedby=\"{described}\">{msg}<p class=\"field-hint\" id=\"files-hint\">Paths in the upstream repository, separated by spaces. Each must exist at the commit.</p></div>\n<div class=\"actions\">{act}<a class=\"cancel\" href=\"/provision\">Cancel</a></div>\n<p class=\"field-hint\">{hint}</p>\n</form>",
        plugin = esc(&r.plugin),
        repo = text_field("repo", "Upstream repository", &r.repo, "Where the plugin's own code lives. It is read, never written.", errors),
        commit = text_field("commit", "Commit", &r.commit, "The full 40-character commit ID. A branch or tag name is refused because it can move.", errors),
        files = esc(&r.files.join(" ")),
    )
}

/// The check table and change list for a plan.
fn preview_block(p: &Plan) -> String {
    let rows: String = p
        .checks
        .iter()
        .map(|c| {
            let outcome = if c.passed { "<span class=\"outcome pass\">✓ Pass</span>" } else { "<span class=\"outcome fail\">✗ Fail</span>" };
            format!("<tr><th scope=\"row\">{}</th><td>{outcome}</td><td class=\"evidence\">{}</td></tr>\n", esc(&c.what), esc(&c.evidence))
        })
        .collect();
    let changes: String = p
        .changes
        .iter()
        .map(|c| format!("<li><span class=\"change-kind modify\">modify</span><span class=\"path\">{}</span></li>\n", esc(&c.path)))
        .collect();
    let files: String = p
        .changes
        .iter()
        .enumerate()
        .map(|(i, c)| {
            format!(
                "<details class=\"file\"{}><summary>{}</summary><pre>{}</pre></details>\n",
                if i == 0 { " open" } else { "" },
                esc(&c.path),
                diff(&c.before, &c.after)
            )
        })
        .collect();
    format!(
        "<section class=\"preview\" aria-labelledby=\"pv\"><h2 id=\"pv\">Preview: what provisioning will do</h2>\n<table class=\"checks\"><caption class=\"visually-hidden\">Checks run before anything changes</caption><thead><tr><th scope=\"col\">Before changing anything</th><th scope=\"col\">Result</th><th scope=\"col\">Evidence</th></tr></thead><tbody>\n{rows}</tbody></table>\n<ul class=\"changes\">\n{changes}<li><span class=\"change-kind create\">create</span><span class=\"path\">vendor/ <span class=\"evidence\">· a checkout of that commit; not committed to git</span></span></li>\n</ul>\n{files}</section>"
    )
}

/// Marks lines removed and added between two versions of a small file.
/// Returns escaped HTML with removed lines before the new contents. Line order
/// and duplicate counts are ignored when deciding whether a line changed.
fn diff(before: &str, after: &str) -> String {
    let b: Vec<&str> = before.lines().collect();
    let a: Vec<&str> = after.lines().collect();
    let mut out = Vec::new();
    for line in &b {
        if !a.contains(line) {
            out.push(format!("<del>{}</del>", esc(line)));
        }
    }
    let removed = out.clone();
    let mut merged = Vec::new();
    for line in &a {
        if b.contains(line) {
            merged.push(esc(line));
        } else {
            merged.push(format!("<ins>{}</ins>", esc(line)));
        }
    }
    if removed.is_empty() {
        merged.join("\n")
    } else {
        format!("{}\n{}", removed.join("\n"), merged.join("\n"))
    }
}

/// The card for the plugin named in a request.
fn card(app: &App, name: &str) -> Option<Card> {
    find(app, name).map(|p| p.card())
}

/// GET /provision: the list of plugins, or the form for one.
pub(crate) fn get(app: &App, q: &BTreeMap<String, String>) -> Response {
    let Some(name) = q.get("plugin") else {
        return list(app);
    };
    let Some(p) = find(app, name) else {
        return Response::status(
            404,
            frame(
                app,
                "Provision",
                KICKER,
                Some(1),
                None,
                &error_banner("No plugin by that name", &[], "<p>Mint it first.</p>"),
            ),
        );
    };
    let (repo, commit, files) = p.upstream.clone().unwrap_or_default();
    let r = ProvisionRequest {
        plugin: p.name.clone(),
        repo,
        commit: p.pinned.clone().unwrap_or(commit),
        files,
    };
    Response::html(frame(
        app,
        &format!("Provision {}", p.display),
        KICKER,
        Some(1),
        Some(&p.card()),
        &form(&r, &[], None),
    ))
}

/// The plugin list with each plugin's provisioning state.
fn list(app: &App) -> Response {
    let rows: String = installed(app)
        .iter()
        .map(|p| {
            let state = match (&p.pinned, p.has_upstream) {
                (Some(c), _) => format!("<span class=\"state yes\">✓ pinned at {}</span>", esc(&c[..7.min(c.len())])),
                (None, false) => "<span class=\"state no\">not needed: its code lives in berry-blocks</span>".into(),
                (None, true) => "<span class=\"state no\">not yet</span>".into(),
            };
            format!("<tr><th scope=\"row\">{}</th><td>{state}</td><td><a href=\"/provision?plugin={}\">Provision…</a></td></tr>\n", esc(&p.display), esc(&p.name))
        })
        .collect();
    let body = format!("<p class=\"lede\">Choose a plugin. A plugin whose code lives in berry-blocks needs no provisioning; one that wraps code from another repository is pinned here to an exact commit.</p>\n<table class=\"plugins-table\"><caption class=\"visually-hidden\">Plugins and their upstream pins</caption><thead><tr><th scope=\"col\">Plugin</th><th scope=\"col\">Upstream</th><th scope=\"col\">Action</th></tr></thead><tbody>\n{rows}</tbody></table>");
    Response::html(frame(app, "Provision", KICKER, Some(1), None, &body))
}

/// POST /provision/preview: validate, fetch, check, and show exactly what would change.
pub(crate) fn preview(app: &App, f: &BTreeMap<String, String>) -> Response {
    let r = request(f);
    let title = format!(
        "Provision {}",
        find(app, &r.plugin)
            .map(|p| p.display)
            .unwrap_or_else(|| r.plugin.clone())
    );
    let c = card(app, &r.plugin);
    match plan(&r, &app.root) {
        Ok(p) if p.ok() => {
            let body = format!("{}\n{}", form(&r, &[], Some(&p.digest)), preview_block(&p));
            Response::html(frame(app, &title, KICKER, Some(1), c.as_ref(), &body))
        }
        Ok(p) => {
            let failing = p.checks.iter().filter(|c| !c.passed).count();
            let banner = error_banner("Nothing in the repository was changed", &[], &format!("<p>{failing} check(s) below failed, so this commit cannot be provisioned. Fix the cause and preview again.</p>"));
            let body = format!("{banner}{}\n{}", form(&r, &[], None), preview_block(&p));
            Response::status(422, frame(app, &title, KICKER, Some(1), c.as_ref(), &body))
        }
        Err(e) => failed(app, &r, e),
    }
}

/// POST /provision: apply the previewed plan and redirect to the done page on success.
/// Returns 422 for invalid fields or failed checks, 409 for a stale preview, and
/// 500 for I/O or Git failures. Applying refetches into the cache; write or checkout
/// failures may leave partial changes.
pub(crate) fn post(app: &App, f: &BTreeMap<String, String>) -> Response {
    let r = request(f);
    let digest = f.get("digest").map(String::as_str).unwrap_or("");
    match apply(&r, &app.root, digest) {
        Ok(_) => Response::see_other(format!("/provision/done?plugin={}", r.plugin)),
        Err(e) => failed(app, &r, e),
    }
}

/// A refusal: invalid fields, failed checks, a stale preview, or an I/O failure.
fn failed(app: &App, r: &ProvisionRequest, e: ProvisionError) -> Response {
    let c = card(app, &r.plugin);
    let title = format!("Provision {}", r.plugin);
    let (status, body) = match e {
        ProvisionError::Invalid(errs) => {
            let errs = field_errors(errs);
            (422, format!("{}{}", error_banner("Nothing was fetched or changed", &errs, ""), form(r, &errs, None)))
        }
        ProvisionError::ChecksFailed(p) => (422, format!("{}{}\n{}", error_banner("Nothing in the repository was changed", &[], "<p>A check failed.</p>"), form(r, &[], None), preview_block(&p))),
        ProvisionError::Stale => (409, format!("{}{}", error_banner("Nothing in the repository was changed", &[], "<p>The form or the repository changed after the preview, so what would be written is no longer what you saw. Preview again.</p>"), form(r, &[], None))),
        ProvisionError::Failed { written, error } => (500, format!("{}{}", error_banner("Provisioning stopped part-way", &[], &format!("<p>{}</p><p>Written before the failure: {}.</p>", esc(&error), if written.is_empty() { "nothing".into() } else { esc(&written.join(", ")) })), form(r, &[], None))),
    };
    Response::status(
        status,
        frame(app, &title, KICKER, Some(1), c.as_ref(), &body),
    )
}

/// GET /provision/done: what happened, and the next step.
pub(crate) fn done(app: &App, q: &BTreeMap<String, String>) -> Response {
    let name = q.get("plugin").cloned().unwrap_or_default();
    let Some(p) = find(app, &name).filter(|p| p.pinned.is_some()) else {
        return Response::status(
            404,
            frame(
                app,
                "Provision",
                KICKER,
                Some(1),
                None,
                &error_banner("Nothing is provisioned under that name", &[], ""),
            ),
        );
    };
    let sha = p.pinned.clone().unwrap_or_default();
    let body = format!("<div class=\"notice\" role=\"status\"><h2>{} is provisioned at {}</h2><p>Changed <span class=\"mono\">pins.kyaml</span> and the plugin's manifest. The upstream code is in <span class=\"mono\">vendor/{}/</span>.</p></div>\n<p>Next is Configure, which is not built yet.</p>\n<div class=\"actions\"><a class=\"btn\" href=\"/\">Back to plugins</a></div>", esc(&p.display), esc(&sha[..7.min(sha.len())]), esc(&p.name));
    Response::html(frame(
        app,
        "Provisioned",
        KICKER,
        Some(1),
        Some(&p.card()),
        &body,
    ))
}
