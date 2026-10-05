// SPDX-License-Identifier: MPL-2.0
//! The berry-blocks plugin wizard: server-rendered, script-free screens that
//! follow `design/wizard/PATTERN.adoc` exactly (topbar and status strip, step
//! rail, work column, preview block, context panel).
//!
//! [`handle`] is a pure function from a [`Request`] to a [`Response`], so every
//! route is tested without a socket. [`serve`] is a small blocking `std::net`
//! loop, like BerryWiki's own server; it binds to loopback by default and
//! refuses form posts from other sites.
//!
//! Built so far: **Mint**. Provision, Configure and Harness are shown in the
//! rail and say plainly that they are not built yet.

use std::collections::BTreeMap;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::time::Duration;

mod provision;

use berry_blocks_mint::{
    apply, plan, uuid_v8_profile_c, ChangeKind, FieldError, MintError, MintRequest, Plan, LICENCES,
};

/// The stylesheet, shared with the approved prototype so the look cannot drift.
const CSS: &str = include_str!("../../../design/wizard/wizard.css");

/// Largest request body accepted, in bytes.
const MAX_BODY: usize = 64 * 1024;

/// The wizard's configuration.
pub struct App {
    /// The berry-blocks checkout the wizard writes into.
    pub root: PathBuf,
    /// The address it serves on, e.g. `127.0.0.1:23880`; requests must name it.
    pub addr: String,
}

/// A parsed HTTP request.
#[derive(Debug, Default)]
pub struct Request {
    /// `GET` or `POST`.
    pub method: String,
    /// Path without the query string.
    pub path: String,
    /// Decoded query parameters.
    pub query: BTreeMap<String, String>,
    /// Header names in lower case.
    pub headers: BTreeMap<String, String>,
    /// Decoded form fields of a POST body.
    pub form: BTreeMap<String, String>,
}

/// An HTTP response.
#[derive(Debug, PartialEq, Eq)]
pub struct Response {
    /// Status code.
    pub status: u16,
    /// Content type.
    pub content_type: &'static str,
    /// Location header for redirects.
    pub location: Option<String>,
    /// Body bytes.
    pub body: String,
}

impl Response {
    /// A 200 HTML page.
    fn html(body: String) -> Self {
        Self {
            status: 200,
            content_type: "text/html; charset=utf-8",
            location: None,
            body,
        }
    }
    /// A page with a non-200 status.
    fn status(status: u16, body: String) -> Self {
        Self {
            status,
            ..Self::html(body)
        }
    }
    /// A 303 redirect after a successful POST.
    fn see_other(to: String) -> Self {
        Self {
            status: 303,
            content_type: "text/plain; charset=utf-8",
            location: Some(to),
            body: String::new(),
        }
    }
}

/// Escapes text for HTML content and double-quoted attributes.
pub fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

/// Decodes `application/x-www-form-urlencoded` text into a map.
pub fn parse_urlencoded(s: &str) -> BTreeMap<String, String> {
    s.split('&')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let (k, v) = p.split_once('=').unwrap_or((p, ""));
            (percent_decode(k), percent_decode(v))
        })
        .collect()
}

/// Decodes `+` and `%XX`; invalid escapes stay literal; invalid UTF-8 is replaced.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < bytes.len() => {
                match u8::from_str_radix(
                    std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("zz"),
                    16,
                ) {
                    Ok(b) => {
                        out.push(b);
                        i += 2;
                    }
                    Err(_) => out.push(b'%'),
                }
            }
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The four verbs, in their fixed order.
const STEPS: [(&str, &str, &str); 4] = [
    ("Mint", "Create the plugin and its manifest", "/mint"),
    ("Provision", "Fetch it at an exact commit", "/provision"),
    ("Configure", "Turn it on for a wiki", "/configure"),
    ("Harness", "Check it does no harm", "/harness"),
];

/// A plugin found in `plugins/<name>/<name>.plugin_praxis.deed`.
pub(crate) struct Installed {
    pub(crate) name: String,
    pub(crate) id: String,
    pub(crate) display: String,
    pub(crate) crate_name: String,
    pub(crate) profiles: String,
    /// The pinned upstream commit from pins.kyaml, if provisioned.
    pub(crate) pinned: Option<String>,
    /// Whether the manifest names an upstream at all.
    pub(crate) has_upstream: bool,
    /// The upstream clause's repo, commit and files, if any.
    pub(crate) upstream: Option<(String, String, Vec<String>)>,
}

/// The first string value after `key` in a deed, if any.
fn deed_value(text: &str, key: &str) -> Option<String> {
    text.split(&format!("{key} \""))
        .nth(1)
        .and_then(|r| r.split('"').next())
        .map(str::to_string)
}

impl Installed {
    /// The context-panel card for this plugin.
    pub(crate) fn card(&self) -> Card {
        Card {
            display: self.display.clone(),
            id: self.id.clone(),
            crate_name: self.crate_name.clone(),
            pinned: self.pinned.clone(),
            profiles: self.profiles.clone(),
        }
    }
}

/// What the context panel shows about one plugin.
pub(crate) struct Card {
    pub(crate) display: String,
    pub(crate) id: String,
    pub(crate) crate_name: String,
    pub(crate) pinned: Option<String>,
    pub(crate) profiles: String,
}

/// Lists minted plugins by reading their manifests and `pins.kyaml`.
pub(crate) fn installed(app: &App) -> Vec<Installed> {
    let pins = fs::read_to_string(app.root.join("pins.kyaml"))
        .ok()
        .and_then(|t| berry_blocks_provision::Pins::parse(&t).ok());
    let mut out = Vec::new();
    let Ok(dirs) = fs::read_dir(app.root.join("plugins")) else {
        return out;
    };
    for d in dirs.flatten() {
        let name = d.file_name().to_string_lossy().into_owned();
        let Ok(text) = fs::read_to_string(d.path().join(format!("{name}.plugin_praxis.deed")))
        else {
            continue;
        };
        let id = deed_value(&text, ":id").unwrap_or_else(|| "unknown".into());
        let display = deed_value(&text, ":display").unwrap_or_else(|| name.clone());
        let crate_name = deed_value(&text, ":crate").unwrap_or_default();
        let profiles = if text.contains(":profiles (static enhanced)") {
            "static, enhanced"
        } else {
            "static"
        }
        .to_string();
        let upstream = text.find("(upstream").map(|i| {
            let clause = &text[i..];
            let files = clause
                .split(":files (")
                .nth(1)
                .and_then(|r| r.split(')').next())
                .map(|l| {
                    l.split('"')
                        .skip(1)
                        .step_by(2)
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default();
            (
                deed_value(clause, ":repo").unwrap_or_default(),
                deed_value(clause, ":commit").unwrap_or_default(),
                files,
            )
        });
        let pinned = pins
            .as_ref()
            .and_then(|p| p.get(&name))
            .map(|p| p.commit.clone());
        out.push(Installed {
            has_upstream: upstream.is_some() || pinned.is_some(),
            upstream,
            pinned,
            name,
            id,
            display,
            crate_name,
            profiles,
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// Renders the step rail. `current` is the step index, if any.
fn rail(app: &App, current: Option<usize>) -> String {
    let items: String = STEPS
        .iter()
        .enumerate()
        .map(|(i, (verb, what, href))| {
            let cur = if Some(i) == current { " aria-current=\"step\"" } else { "" };
            let state = if Some(i) == current { "current step" } else if i <= 1 { "available" } else { "not built yet" };
            format!("<li><a href=\"{href}\"{cur}><span class=\"n\" aria-hidden=\"true\">{n}</span><span><span class=\"verb\">{verb}</span><span class=\"what\">{what}</span><span class=\"visually-hidden\">, {state}</span></span></a></li>\n", n = i + 1)
        })
        .collect();
    let plugins: String = installed(app)
        .iter()
        .map(|p| format!("<li><a href=\"/\">{}</a></li>\n", esc(&p.name)))
        .collect();
    format!("<nav class=\"rail\" aria-label=\"Plugin steps\">\n<h2 id=\"steps-h\">Steps</h2>\n<ol class=\"steps\" aria-labelledby=\"steps-h\">\n{items}</ol>\n<h2>Plugins</h2>\n<ul class=\"plugins\">\n{plugins}<li><a href=\"/mint\">+ Mint a new plugin</a></li>\n</ul>\n</nav>")
}

/// The card for a plugin that is being minted, from the form.
fn mint_card(r: &MintRequest) -> Option<Card> {
    if r.name.is_empty() {
        return None;
    }
    let id = if validate_name_shape(&r.name) {
        uuid_v8_profile_c("berry-blocks-plugin", &r.name)
    } else {
        "assigned at mint".into()
    };
    Some(Card {
        display: r.display.clone(),
        id,
        crate_name: format!("berry-blocks-{}", r.name),
        pinned: None,
        profiles: if r.enhanced {
            "static, enhanced"
        } else {
            "static"
        }
        .into(),
    })
}

/// Renders the context panel for a plugin (or none).
pub(crate) fn context(card: Option<&Card>) -> String {
    let card = match card {
        Some(c) => format!("<dt>Name</dt><dd>{}</dd>\n<dt>ID</dt><dd class=\"mono\">{}</dd>\n<dt>ID kind</dt><dd>UUID v8, profile C</dd>\n<dt>Crate</dt><dd class=\"mono\">{}</dd>\n<dt>Pinned at</dt><dd class=\"mono\">{}</dd>\n<dt>Profiles</dt><dd>{}</dd>",
            esc(&c.display), esc(&c.id), esc(&c.crate_name), esc(&c.pinned.as_deref().map(|s| s[..7.min(s.len())].to_string()).unwrap_or_else(|| "not yet".into())), esc(&c.profiles)),
        None => "<dt>Plugin</dt><dd>none chosen yet</dd>".into(),
    };
    format!("<aside class=\"context\" aria-label=\"About this plugin\">\n<h2>This plugin</h2>\n<div class=\"card\"><dl>\n{card}\n</dl></div>\n<h2>Independence</h2>\n<ul class=\"checklist\">\n<li class=\"yes\">BerryWiki is not modified</li>\n<li class=\"yes\">ProgBlocks is not modified</li>\n<li class=\"yes\">Neither depends on this plugin</li>\n<li class=\"yes\">Static profile has no script</li>\n</ul>\n<h2>Last checks</h2>\n<p class=\"evidence\">Not run yet.</p>\n</aside>")
}

/// Whether a name is shaped like a plugin name (used only to show its future ID).
fn validate_name_shape(name: &str) -> bool {
    name.chars().next().is_some_and(|c| c.is_ascii_lowercase())
        && name.len() <= 40
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// The one frame every screen uses.
pub(crate) fn frame(
    app: &App,
    title: &str,
    kicker: &str,
    step: Option<usize>,
    ctx: Option<&Card>,
    body: &str,
) -> String {
    format!(
        "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n<title>{t} · berry-blocks</title>\n<link rel=\"stylesheet\" href=\"/wizard.css\">\n</head>\n<body>\n<a class=\"skip-link\" href=\"#main\">Skip to content</a>\n<header>\n<div class=\"topbar\">\n<a class=\"brand\" href=\"/\">berry-blocks</a>\n<span class=\"section\">Plugins</span>\n</div>\n<p class=\"status-strip\"><strong>Lab mode</strong> · writes only to <span class=\"mono\">{root}</span> · BerryWiki and ProgBlocks are never modified · nothing happens until you press an action button</p>\n</header>\n<div class=\"grid\">\n{rail}\n<main class=\"main\" id=\"main\">\n<p class=\"kicker\">{k}</p>\n<h1>{t}</h1>\n{body}\n</main>\n{ctx}\n</div>\n</body>\n</html>\n",
        t = esc(title),
        k = esc(kicker),
        root = esc(&app.root.display().to_string()),
        rail = rail(app, step),
        ctx = context(ctx),
    )
}

/// The plugins overview.
fn overview(app: &App) -> Response {
    let rows: String = installed(app)
        .iter()
        .map(|p| {
            let prov = match (&p.pinned, p.has_upstream) {
                (Some(c), _) => format!("<td class=\"state yes\">✓ {}</td>", esc(&c[..7.min(c.len())])),
                (None, false) => "<td class=\"state no\">not needed</td>".to_string(),
                (None, true) => format!("<td class=\"state no\"><a href=\"/provision?plugin={0}\">not yet</a></td>", esc(&p.name)),
            };
            format!("<tr><th scope=\"row\">{}</th><td class=\"mono\">{}</td><td class=\"state yes\">✓ yes</td>{prov}<td class=\"state no\">not built yet</td></tr>\n", esc(&p.name), esc(&p.id))
        })
        .collect();
    let table = if rows.is_empty() {
        "<p>No plugins have been minted yet.</p>".to_string()
    } else {
        format!("<table class=\"plugins-table\"><caption class=\"visually-hidden\">Minted plugins</caption><thead><tr><th scope=\"col\">Plugin</th><th scope=\"col\">ID</th><th scope=\"col\">Minted</th><th scope=\"col\">Provisioned</th><th scope=\"col\">Configure, Harness</th></tr></thead><tbody>\n{rows}</tbody></table>")
    };
    let body = format!("<p class=\"lede\">A plugin changes how certain blocks in a BerryWiki page are shown. Every plugin goes through the same four steps, in this order, and every step shows you exactly what it will change before it changes anything.</p>\n{table}\n<div class=\"actions\"><a class=\"btn\" href=\"/mint\">Mint a new plugin</a></div>");
    Response::html(frame(app, "Plugins", "Plugins", None, None, &body))
}

/// Reads a mint request from form fields.
fn mint_request(form: &BTreeMap<String, String>) -> MintRequest {
    let get = |k: &str| {
        form.get(k)
            .map(|v| v.trim().to_string())
            .unwrap_or_default()
    };
    MintRequest {
        name: get("name"),
        display: get("display"),
        claims: get("claims"),
        run_key: get("run_key"),
        enhanced: form.get("enhanced").is_some(),
        licence: if get("licence").is_empty() {
            "MPL-2.0".into()
        } else {
            get("licence")
        },
    }
}

/// A labelled text field, marked invalid with its message when it has an error.
pub(crate) fn text_field(
    id: &str,
    label: &str,
    value: &str,
    hint: &str,
    errors: &[FieldError],
) -> String {
    let err = errors.iter().find(|e| e.field == id);
    let (invalid, described, msg) = match err {
        Some(e) => (
            " aria-invalid=\"true\"",
            format!("{id}-error {id}-hint"),
            format!(
                "<p class=\"field-error\" id=\"{id}-error\">Error: {}</p>",
                esc(&e.message)
            ),
        ),
        None => ("", format!("{id}-hint"), String::new()),
    };
    format!("<div class=\"field\"><label for=\"{id}\">{label}</label><input type=\"text\" class=\"mono\" id=\"{id}\" name=\"{id}\" value=\"{v}\"{invalid} aria-describedby=\"{described}\">{msg}<p class=\"field-hint\" id=\"{id}-hint\">{hint}</p></div>\n", v = esc(value))
}

/// The mint form. `digest` is set once a valid preview has been shown.
fn mint_form(r: &MintRequest, errors: &[FieldError], digest: Option<&str>) -> String {
    let licences: String = LICENCES
        .iter()
        .map(|l| {
            format!(
                "<option{}>{l}</option>",
                if *l == r.licence { " selected" } else { "" }
            )
        })
        .collect();
    let act = match digest {
        Some(d) => format!("<input type=\"hidden\" name=\"digest\" value=\"{}\"><button class=\"btn secondary\" type=\"submit\" formaction=\"/mint/preview\">Preview again</button><button class=\"btn\" type=\"submit\" formaction=\"/mint\">Mint plugin</button>", esc(d)),
        None => "<button class=\"btn secondary\" type=\"submit\" formaction=\"/mint/preview\">Preview</button><button class=\"btn\" type=\"submit\" disabled>Mint plugin</button>".into(),
    };
    let hint = if digest.is_none() {
        "<p class=\"field-hint\">The action stays unavailable until you have seen the preview.</p>"
    } else {
        "<p class=\"field-hint\">Minting writes exactly the files previewed below. If you change a field, preview again first.</p>"
    };
    format!(
        "<p class=\"lede\">Minting creates a new, empty plugin in this repository: a Rust crate for its code and a manifest that says what it claims. Nothing is fetched and no wiki changes.</p>\n<form method=\"post\" action=\"/mint/preview\">\n{name}{display}{claims}{run}<fieldset class=\"field\"><legend>Profiles</legend>\n<div class=\"choice\"><input type=\"checkbox\" id=\"p-static\" checked disabled><label for=\"p-static\">Static <span class=\"fixed\">always required</span></label><p class=\"field-hint\">Plain HTML with no script, so BerryWiki can serve it. Every plugin must work here first.</p></div>\n<div class=\"choice\"><input type=\"checkbox\" id=\"p-enhanced\" name=\"enhanced\"{enh}><label for=\"p-enhanced\">Enhanced</label><p class=\"field-hint\">Adds script on top of the static result, for hosts that allow it.</p></div>\n</fieldset>\n<div class=\"field\"><label for=\"licence\">Licence</label><select id=\"licence\" name=\"licence\" aria-describedby=\"licence-hint\">{licences}</select><p class=\"field-hint\" id=\"licence-hint\">Must be compatible with MPL-2.0.</p></div>\n<div class=\"actions\">{act}<a class=\"cancel\" href=\"/\">Cancel</a></div>\n{hint}\n</form>",
        name = text_field("name", "Short name", &r.name, "Lowercase letters, digits and hyphens. Used for the crate and folder names.", errors),
        display = text_field("display", "Display name", &r.display, "What readers and authors see.", errors),
        claims = text_field("claims", "What it claims in a page", &r.claims, "A fenced code block belongs to this plugin when its info string has this key, as in <code>```bash variant=macOS</code>.", errors),
        run = text_field("run_key", "What groups blocks together", &r.run_key, "Consecutive claimed blocks with the same value of this key are rendered as one.", errors),
        enh = if r.enhanced { " checked" } else { "" },
    )
}

/// The preview block: every change, then each file in full.
fn preview_block(p: &Plan) -> String {
    let creates = p
        .changes
        .iter()
        .filter(|c| c.kind == ChangeKind::Create)
        .count();
    let modifies = p.changes.len() - creates;
    let list: String = p
        .changes
        .iter()
        .map(|c| {
            let (kind, label) = match c.kind { ChangeKind::Create => ("create", "create"), ChangeKind::Modify => ("modify", "modify") };
            format!("<li><span class=\"change-kind {kind}\">{label}</span><span class=\"path\">{} <span class=\"evidence\">· {}</span></span></li>\n", esc(&c.path), esc(c.note))
        })
        .collect();
    let files: String = p
        .changes
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let content = match c.kind {
                ChangeKind::Create => esc(&c.content),
                ChangeKind::Modify => c
                    .content
                    .lines()
                    .map(|l| if c.added_lines.iter().any(|a| a == l) { format!("<ins>{}</ins>", esc(l)) } else { esc(l) })
                    .collect::<Vec<_>>()
                    .join("\n"),
            };
            format!("<details class=\"file\"{open}><summary>{}</summary><pre>{content}</pre></details>\n", esc(&c.path), open = if i == 0 { " open" } else { "" })
        })
        .collect();
    format!("<section class=\"preview\" aria-labelledby=\"pv\"><h2 id=\"pv\">Preview: what minting will do</h2>\n<p>{creates} new files and {modifies} change. The plugin's ID, <span class=\"mono\">{id}</span>, is derived from its name, so minting the same name again gives the same ID.</p>\n<ul class=\"changes\">\n{list}</ul>\n{files}</section>", id = esc(&p.plugin_id))
}

/// An error banner that links each problem to its field.
pub(crate) fn error_banner(heading: &str, errors: &[FieldError], extra: &str) -> String {
    let items: String = errors
        .iter()
        .map(|e| {
            format!(
                "<li><a href=\"#{}\">{}</a>: {}</li>",
                e.field,
                esc(&field_label(e.field)),
                esc(&e.message)
            )
        })
        .collect();
    let list = if items.is_empty() {
        String::new()
    } else {
        format!("<p>Fix these and preview again:</p><ul>{items}</ul>")
    };
    format!("<div class=\"error-banner\" role=\"alert\" aria-labelledby=\"err-h\"><h2 id=\"err-h\">{}</h2>{list}{extra}</div>\n", esc(heading))
}

/// The visible label of a form field, for error links.
pub(crate) fn field_label(field: &str) -> String {
    match field {
        "name" => "Short name",
        "display" => "Display name",
        "claims" => "What it claims in a page",
        "run_key" => "What groups blocks together",
        "licence" => "Licence",
        "repo" => "Upstream repository",
        "commit" => "Commit",
        "files" => "Files it needs",
        "plugin" => "Plugin",
        other => other,
    }
    .to_string()
}

/// GET /mint: the empty form, with the ProgBlocks-shaped defaults as hints.
fn mint_get(app: &App) -> Response {
    let r = MintRequest {
        claims: "variant".into(),
        run_key: "group".into(),
        enhanced: true,
        licence: "MPL-2.0".into(),
        ..Default::default()
    };
    Response::html(frame(
        app,
        "Mint a plugin",
        "Step 1 of 4 · Mint",
        Some(0),
        None,
        &mint_form(&r, &[], None),
    ))
}

/// POST /mint/preview: validate and show exactly what would be written.
fn mint_preview(app: &App, form: &BTreeMap<String, String>) -> Response {
    let r = mint_request(form);
    match plan(&r, &app.root) {
        Ok(p) => {
            let body = format!(
                "{}\n{}",
                mint_form(&r, &[], Some(&p.digest)),
                preview_block(&p)
            );
            Response::html(frame(
                app,
                "Mint a plugin",
                "Step 1 of 4 · Mint",
                Some(0),
                mint_card(&r).as_ref(),
                &body,
            ))
        }
        Err(MintError::Invalid(errs)) => {
            let body = format!(
                "{}{}",
                error_banner("Nothing was created or changed", &errs, ""),
                mint_form(&r, &errs, None)
            );
            Response::status(
                422,
                frame(
                    app,
                    "Mint a plugin",
                    "Step 1 of 4 · Mint",
                    Some(0),
                    mint_card(&r).as_ref(),
                    &body,
                ),
            )
        }
        Err(e) => mint_failed(app, &r, &e),
    }
}

/// POST /mint: apply the previewed plan, or refuse without writing.
fn mint_post(app: &App, form: &BTreeMap<String, String>) -> Response {
    let r = mint_request(form);
    let digest = form.get("digest").map(String::as_str).unwrap_or("");
    match apply(&r, &app.root, digest) {
        Ok(_) => Response::see_other(format!("/mint/done?name={}", r.name)),
        Err(MintError::Invalid(errs)) => {
            let body = format!(
                "{}{}",
                error_banner("Nothing was created or changed", &errs, ""),
                mint_form(&r, &errs, None)
            );
            Response::status(
                422,
                frame(
                    app,
                    "Mint a plugin",
                    "Step 1 of 4 · Mint",
                    Some(0),
                    mint_card(&r).as_ref(),
                    &body,
                ),
            )
        }
        Err(e) => mint_failed(app, &r, &e),
    }
}

/// A refusal that is not a field error: stale preview, existing file, or I/O.
fn mint_failed(app: &App, r: &MintRequest, e: &MintError) -> Response {
    let (status, heading, detail) = match e {
        MintError::Stale => (409, "Nothing was created or changed".to_string(), "<p>The form or the repository changed after the preview, so what would be written is no longer what you saw. Preview again.</p>".to_string()),
        MintError::Exists(p) => (409, "Nothing was created or changed".to_string(), format!("<p><span class=\"mono\">{}</span> already exists, and minting never overwrites a file.</p>", esc(p))),
        MintError::Io { written, error } => (500, "Minting stopped part-way".to_string(), format!("<p>{}</p><p>Written before the failure: {}. Remove these by hand or with git before trying again.</p>", esc(error), if written.is_empty() { "nothing".to_string() } else { esc(&written.join(", ")) })),
        MintError::Invalid(_) => unreachable!("handled by the caller"),
    };
    let body = format!(
        "{}{}",
        error_banner(&heading, &[], &detail),
        mint_form(r, &[], None)
    );
    Response::status(
        status,
        frame(
            app,
            "Mint a plugin",
            "Step 1 of 4 · Mint",
            Some(0),
            mint_card(r).as_ref(),
            &body,
        ),
    )
}

/// GET /mint/done: what happened, and the one next step.
fn mint_done(app: &App, q: &BTreeMap<String, String>) -> Response {
    let name = q.get("name").cloned().unwrap_or_default();
    let found = installed(app).into_iter().find(|p| p.name == name);
    let body = match found {
        Some(p) => format!("<div class=\"notice\" role=\"status\"><h2>{n} is minted</h2><p>Created 4 files and changed 1. Its ID is <span class=\"mono\">{id}</span>.</p></div>\n<p>The plugin exists and builds, and it renders its blocks as plain code until you change it. Next is Provision, which is not built yet.</p>\n<div class=\"actions\"><a class=\"btn\" href=\"/\">Back to plugins</a></div>", n = esc(&p.name), id = esc(&p.id)),
        None => "<div class=\"error-banner\" role=\"alert\"><h2>No plugin by that name</h2><p>Nothing was minted under that name.</p></div>".to_string(),
    };
    Response::html(frame(
        app,
        "Minted",
        "Step 1 of 4 · Mint",
        Some(0),
        None,
        &body,
    ))
}

/// A step that is not built yet, said plainly in the same frame.
fn not_built(app: &App, step: usize) -> Response {
    let (verb, what, _) = STEPS[step];
    let body = format!("<div class=\"draft-banner\" role=\"status\"><h2>{verb} is not built yet</h2><p>This step will {w}. Its screens are designed (see <span class=\"mono\">design/wizard/site/</span>) but not implemented; nothing here can change anything.</p></div>\n<div class=\"actions\"><a class=\"btn secondary\" href=\"/\">Back to plugins</a></div>", w = what.to_lowercase());
    Response::html(frame(
        app,
        verb,
        &format!("Step {} of 4 · {verb}", step + 1),
        Some(step),
        None,
        &body,
    ))
}

/// Refuses requests that do not name this server, and cross-site form posts.
fn origin_problem(app: &App, req: &Request) -> Option<&'static str> {
    let host = req.headers.get("host").map(String::as_str).unwrap_or("");
    let port = app.addr.rsplit(':').next().unwrap_or("");
    let allowed = [
        app.addr.clone(),
        format!("localhost:{port}"),
        format!("127.0.0.1:{port}"),
    ];
    if !allowed.iter().any(|a| a == host) {
        return Some("unexpected Host header");
    }
    if req.method == "POST" {
        if let Some(origin) = req.headers.get("origin") {
            if !allowed.iter().any(|a| origin == &format!("http://{a}")) {
                return Some("form posted from another site");
            }
        }
    }
    None
}

/// Routes one request. Pure: no socket, so every route is unit-testable.
pub fn handle(app: &App, req: &Request) -> Response {
    if let Some(why) = origin_problem(app, req) {
        return Response::status(403, format!("<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><title>Refused</title></head><body><main><h1>Refused</h1><p>{}</p></main></body></html>", esc(why)));
    }
    match (req.method.as_str(), req.path.as_str()) {
        ("GET", "/") => overview(app),
        ("GET", "/wizard.css") => Response {
            status: 200,
            content_type: "text/css; charset=utf-8",
            location: None,
            body: CSS.to_string(),
        },
        ("GET", "/mint") => mint_get(app),
        ("POST", "/mint/preview") => mint_preview(app, &req.form),
        ("POST", "/mint") => mint_post(app, &req.form),
        ("GET", "/mint/done") => mint_done(app, &req.query),
        ("GET", "/provision") => provision::get(app, &req.query),
        ("POST", "/provision/preview") => provision::preview(app, &req.form),
        ("POST", "/provision") => provision::post(app, &req.form),
        ("GET", "/provision/done") => provision::done(app, &req.query),
        ("GET", "/configure") => not_built(app, 2),
        ("GET", "/harness") => not_built(app, 3),
        _ => Response::status(
            404,
            frame(
                app,
                "Not found",
                "Plugins",
                None,
                None,
                "<p>There is no page here. <a href=\"/\">Back to plugins</a>.</p>",
            ),
        ),
    }
}

/// Reads one HTTP/1.1 request from a stream.
fn read_request(stream: &TcpStream) -> Option<Request> {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).ok()?;
    let mut parts = line.split_whitespace();
    let method = parts.next()?.to_string();
    let target = parts.next()?.to_string();
    let (path, query) = target.split_once('?').unwrap_or((&target, ""));
    let mut headers = BTreeMap::new();
    loop {
        let mut h = String::new();
        reader.read_line(&mut h).ok()?;
        let h = h.trim_end();
        if h.is_empty() {
            break;
        }
        if let Some((k, v)) = h.split_once(':') {
            headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
        }
    }
    let len: usize = headers
        .get("content-length")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    if len > MAX_BODY {
        return None;
    }
    let mut body = vec![0; len];
    reader.read_exact(&mut body).ok()?;
    Some(Request {
        method,
        path: path.to_string(),
        query: parse_urlencoded(query),
        headers,
        form: parse_urlencoded(&String::from_utf8_lossy(&body)),
    })
}

/// Writes a response, with no-store caching and a strict content policy.
fn write_response(mut stream: &TcpStream, r: &Response) {
    let reason = match r.status {
        200 => "OK",
        303 => "See Other",
        403 => "Forbidden",
        404 => "Not Found",
        409 => "Conflict",
        422 => "Unprocessable Content",
        _ => "Internal Server Error",
    };
    let location = r
        .location
        .as_ref()
        .map(|l| format!("Location: {l}\r\n"))
        .unwrap_or_default();
    let head = format!(
        "HTTP/1.1 {} {reason}\r\nContent-Type: {}\r\nContent-Length: {}\r\n{location}Cache-Control: no-store\r\nContent-Security-Policy: default-src 'none'; style-src 'self'; form-action 'self'; base-uri 'none'; frame-ancestors 'none'\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n",
        r.status, r.content_type, r.body.len()
    );
    let _ = stream
        .write_all(head.as_bytes())
        .and_then(|_| stream.write_all(r.body.as_bytes()));
}

/// How long one connection may take to send its request or accept a reply.
pub const IO_TIMEOUT: Duration = Duration::from_secs(10);

/// Serves the wizard until the process is stopped.
pub fn serve(app: &App) -> std::io::Result<()> {
    serve_listener(app, TcpListener::bind(&app.addr)?, IO_TIMEOUT)
}

/// Serves on an already bound listener, one connection at a time. Each
/// connection gets read and write timeouts, so a client that connects and
/// sends nothing, or stops part-way, cannot stop the wizard serving others.
pub fn serve_listener(app: &App, listener: TcpListener, timeout: Duration) -> std::io::Result<()> {
    for stream in listener.incoming().flatten() {
        if stream.set_read_timeout(Some(timeout)).is_err()
            || stream.set_write_timeout(Some(timeout)).is_err()
        {
            continue;
        }
        match read_request(&stream) {
            Some(req) => write_response(&stream, &handle(app, &req)),
            None => write_response(&stream, &Response::status(400, "Bad request".into())),
        }
    }
    Ok(())
}
