// SPDX-License-Identifier: MPL-2.0
//! Configure screens: turn plugins on for a wiki and choose its profile.
//! Same frame and behaviour as Mint and Provision (design/wizard/PATTERN.adoc).

use std::collections::BTreeMap;
use std::fs;

use berry_blocks_configure::{
    apply, config_path, plan, ConfigureError, ConfigureRequest, Plan, PluginConfig, WikiConfig,
};
use berry_blocks_mint::FieldError;

use crate::provision::diff;
use crate::{error_banner, esc, frame, installed, text_field, App, Response};

const KICKER: &str = "Step 3 of 4 · Configure";

/// Plugins that can be turned on: minted (have a manifest) and registered.
fn available(app: &App) -> Vec<berry_blocks_registry::Entry> {
    let minted: Vec<String> = installed(app).into_iter().map(|p| p.name).collect();
    berry_blocks_registry::entries()
        .into_iter()
        .filter(|e| minted.iter().any(|m| m == e.name))
        .collect()
}

/// Reads a configure request from form fields (`use_<plugin>`, `opt_<plugin>_<key>`).
fn request(app: &App, f: &BTreeMap<String, String>) -> ConfigureRequest {
    let get = |k: &str| f.get(k).map(|v| v.trim().to_string()).unwrap_or_default();
    let plugins = available(app)
        .into_iter()
        .filter(|e| f.contains_key(&format!("use_{}", e.name)))
        .map(|e| PluginConfig {
            name: e.name.to_string(),
            options: e
                .options
                .iter()
                .map(|o| {
                    (
                        o.key.to_string(),
                        f.contains_key(&format!("opt_{}_{}", e.name, o.key)),
                    )
                })
                .collect(),
        })
        .collect();
    ConfigureRequest {
        name: get("config_name"),
        wiki: get("wiki"),
        profile: get("profile"),
        plugins,
    }
}

/// The configure form. `digest` is set once a valid preview has been shown.
fn form(app: &App, r: &ConfigureRequest, errors: &[FieldError], digest: Option<&str>) -> String {
    let radio = |v: &str, label: &str, hint: &str| {
        format!("<div class=\"choice\"><input type=\"radio\" id=\"prof-{v}\" name=\"profile\" value=\"{v}\"{c}><label for=\"prof-{v}\">{label}</label><p class=\"field-hint\">{hint}</p></div>", c = if r.profile == v { " checked" } else { "" })
    };
    let plugins: String = available(app)
        .iter()
        .map(|e| {
            let on = r.plugins.iter().find(|p| p.name == e.name);
            let opts: String = e
                .options
                .iter()
                .map(|o| {
                    let checked = on.and_then(|p| p.options.iter().find(|(k, _)| k == o.key)).map(|(_, v)| *v).unwrap_or(o.default);
                    format!("<div class=\"choice\"><input type=\"checkbox\" id=\"opt-{n}-{k}\" name=\"opt_{n}_{k}\"{c}><label for=\"opt-{n}-{k}\">{l}</label><p class=\"field-hint\">{h}</p></div>", n = e.name, k = o.key, l = esc(o.label), h = esc(o.hint), c = if checked { " checked" } else { "" })
                })
                .collect();
            format!("<div class=\"choice\"><input type=\"checkbox\" id=\"use-{n}\" name=\"use_{n}\"{c}><label for=\"use-{n}\">{n}</label></div>\n{opts}", n = e.name, c = if on.is_some() { " checked" } else { "" })
        })
        .collect();
    let plugin_err = errors
        .iter()
        .filter(|e| e.field == "plugins")
        .map(|e| format!("<p class=\"field-error\">Error: {}</p>", esc(&e.message)))
        .collect::<String>();
    let act = match digest {
        Some(d) => format!("<input type=\"hidden\" name=\"digest\" value=\"{}\"><button class=\"btn secondary\" type=\"submit\" formaction=\"/configure/preview\">Preview again</button><button class=\"btn\" type=\"submit\" formaction=\"/configure\">Save configuration</button>", esc(d)),
        None => "<button class=\"btn secondary\" type=\"submit\" formaction=\"/configure/preview\">Preview</button><button class=\"btn\" type=\"submit\" disabled>Save configuration</button>".into(),
    };
    let hint = if digest.is_none() {
        "The action stays unavailable until you have seen the preview."
    } else {
        "Saving writes exactly the file previewed below. If you change a field, preview again first."
    };
    format!(
        "<p class=\"lede\">Configuring turns plugins on for one wiki and chooses how its pages are delivered. Pages that do not use a plugin are not affected, and the wiki itself is never written.</p>\n<form method=\"post\" action=\"/configure/preview\">\n{name}{wiki}<fieldset class=\"field\" id=\"profile\"><legend>How pages are delivered</legend>\n{s}\n{e}\n</fieldset>\n<fieldset class=\"field\" id=\"plugins\"><legend>Plugins</legend>\n{plugin_err}{plugins}</fieldset>\n<div class=\"actions\">{act}<a class=\"cancel\" href=\"/configure\">Cancel</a></div>\n<p class=\"field-hint\">{hint}</p>\n</form>",
        name = text_field("config_name", "Configuration name", &r.name, "Saved as wikis/&lt;name&gt;.kyaml.", errors),
        wiki = text_field("wiki", "Wiki folder", &r.wiki, "A BerryWiki folder or clone, relative to this checkout. It is read, never written.", errors),
        s = radio("static", "Static", "Every variant shown as an expandable section. No script. BerryWiki could serve this."),
        e = radio("enhanced", "Enhanced", "The static result, upgraded to tabs with editable values when script runs. BerryWiki cannot serve this today; a documentation site can."),
    )
}

/// The change, the file, and the per-page effects.
fn preview_block(p: &Plan) -> String {
    let (kind, label) = if p.before.is_some() {
        ("modify", "modify")
    } else {
        ("create", "create")
    };
    let content = match &p.before {
        Some(b) => diff(b, &p.after),
        None => esc(&p.after),
    };
    let changed = p.pages.iter().filter(|e| e.runs > 0).count();
    let rows: String = p
        .pages
        .iter()
        .map(|e| {
            let what = if e.runs > 0 { format!("<span class=\"outcome pass\">renders differently</span> <span class=\"evidence\">{} block(s)</span>", e.runs) } else { "<span class=\"outcome skip\">unchanged</span>".into() };
            format!("<tr><th scope=\"row\">{}</th><td>{what}</td></tr>\n", esc(&e.page))
        })
        .collect();
    format!(
        "<section class=\"preview\" aria-labelledby=\"pv\"><h2 id=\"pv\">Preview: what saving will change</h2>\n<p>Of the wiki's {n} pages, {changed} render differently with this configuration. The others stay exactly as BerryWiki renders them.</p>\n<ul class=\"changes\">\n<li><span class=\"change-kind {kind}\">{label}</span><span class=\"path\">{path}</span></li>\n<li><span class=\"change-kind none\">unchanged</span><span class=\"path\">the wiki folder <span class=\"evidence\">· never written</span></span></li>\n</ul>\n<details class=\"file\" open><summary>{path}</summary><pre>{content}</pre></details>\n<table class=\"checks\"><caption class=\"visually-hidden\">Pages and how this configuration affects them</caption><thead><tr><th scope=\"col\">Page</th><th scope=\"col\">Effect</th></tr></thead><tbody>\n{rows}</tbody></table></section>",
        n = p.pages.len(),
        path = esc(&p.path),
    )
}

/// Existing configurations, by name.
fn configs(app: &App) -> Vec<(String, WikiConfig)> {
    let mut v: Vec<(String, WikiConfig)> = fs::read_dir(app.root.join("wikis"))
        .map(|rd| {
            rd.flatten()
                .filter_map(|e| {
                    let p = e.path();
                    let name = p.file_name()?.to_str()?.strip_suffix(".kyaml")?.to_string();
                    let cfg = WikiConfig::parse(&fs::read_to_string(&p).ok()?).ok()?;
                    Some((name, cfg))
                })
                .collect()
        })
        .unwrap_or_default();
    v.sort_by(|a, b| a.0.cmp(&b.0));
    v
}

/// GET /configure: the configurations, or the form for one.
pub(crate) fn get(app: &App, q: &BTreeMap<String, String>) -> Response {
    if q.contains_key("new") || q.contains_key("config") {
        let r = match q
            .get("config")
            .and_then(|n| configs(app).into_iter().find(|(c, _)| c == n))
        {
            Some((name, c)) => ConfigureRequest {
                name,
                wiki: c.wiki,
                profile: c.profile,
                plugins: c.plugins,
            },
            None => ConfigureRequest {
                profile: "static".into(),
                ..Default::default()
            },
        };
        return Response::html(frame(
            app,
            "Configure a wiki",
            KICKER,
            Some(2),
            None,
            &form(app, &r, &[], None),
        ));
    }
    let rows: String = configs(app)
        .iter()
        .map(|(n, c)| format!("<tr><th scope=\"row\">{n}</th><td class=\"mono\">{w}</td><td>{p}</td><td>{pl}</td><td><a href=\"/configure?config={n}\">Edit…</a></td></tr>\n", n = esc(n), w = esc(&c.wiki), p = esc(&c.profile), pl = esc(&c.plugins.iter().map(|p| p.name.as_str()).collect::<Vec<_>>().join(", "))))
        .collect();
    let table = if rows.is_empty() {
        "<p>No wiki is configured yet.</p>".to_string()
    } else {
        format!("<table class=\"plugins-table\"><caption class=\"visually-hidden\">Configured wikis</caption><thead><tr><th scope=\"col\">Name</th><th scope=\"col\">Wiki</th><th scope=\"col\">Profile</th><th scope=\"col\">Plugins</th><th scope=\"col\">Action</th></tr></thead><tbody>\n{rows}</tbody></table>")
    };
    let body = format!("<p class=\"lede\">A configuration turns plugins on for one wiki. Render it with <span class=\"mono\">berry-blocks render --config wikis/&lt;name&gt;.kyaml OUT</span>.</p>\n{table}\n<div class=\"actions\"><a class=\"btn\" href=\"/configure?new\">Configure a wiki</a></div>");
    Response::html(frame(app, "Configure", KICKER, Some(2), None, &body))
}

/// Converts configure field errors to the shared type.
fn field_errors(errs: Vec<berry_blocks_configure::FieldError>) -> Vec<FieldError> {
    errs.into_iter()
        .map(|e| FieldError {
            field: if e.field == "name" {
                "config_name"
            } else {
                e.field
            },
            message: e.message,
        })
        .collect()
}

/// POST /configure/preview.
pub(crate) fn preview(app: &App, f: &BTreeMap<String, String>) -> Response {
    let r = request(app, f);
    match plan(&r, &app.root) {
        Ok(p) => Response::html(frame(
            app,
            "Configure a wiki",
            KICKER,
            Some(2),
            None,
            &format!(
                "{}\n{}",
                form(app, &r, &[], Some(&p.digest)),
                preview_block(&p)
            ),
        )),
        Err(e) => failed(app, &r, e),
    }
}

/// POST /configure.
pub(crate) fn post(app: &App, f: &BTreeMap<String, String>) -> Response {
    let r = request(app, f);
    let digest = f.get("digest").map(String::as_str).unwrap_or("");
    match apply(&r, &app.root, digest) {
        Ok(_) => Response::see_other(format!("/configure/done?config={}", r.name)),
        Err(e) => failed(app, &r, e),
    }
}

/// A refusal; nothing was written.
fn failed(app: &App, r: &ConfigureRequest, e: ConfigureError) -> Response {
    let (status, body) = match e {
        ConfigureError::Invalid(errs) => {
            let errs = field_errors(errs);
            (422, format!("{}{}", error_banner("Nothing was saved", &errs, ""), form(app, r, &errs, None)))
        }
        ConfigureError::Stale => (409, format!("{}{}", error_banner("Nothing was saved", &[], "<p>The form or the wiki changed after the preview, so what would be written is no longer what you saw. Preview again.</p>"), form(app, r, &[], None))),
        ConfigureError::Failed(msg) => (500, format!("{}{}", error_banner("Nothing was saved", &[], &format!("<p>{}</p>", esc(&msg))), form(app, r, &[], None))),
    };
    Response::status(
        status,
        frame(app, "Configure a wiki", KICKER, Some(2), None, &body),
    )
}

/// GET /configure/done.
pub(crate) fn done(app: &App, q: &BTreeMap<String, String>) -> Response {
    let name = q.get("config").cloned().unwrap_or_default();
    let Some((_, c)) = configs(app).into_iter().find(|(n, _)| *n == name) else {
        return Response::status(
            404,
            frame(
                app,
                "Configure",
                KICKER,
                Some(2),
                None,
                &error_banner("No configuration by that name", &[], ""),
            ),
        );
    };
    let body = format!("<div class=\"notice\" role=\"status\"><h2>{n} is configured</h2><p>Saved <span class=\"mono\">{p}</span>: {pl} on <span class=\"mono\">{w}</span>, {prof} profile.</p></div>\n<p>Render it with <span class=\"mono\">berry-blocks render --config {p} OUT</span>. Next is Harness, which is not built yet.</p>\n<div class=\"actions\"><a class=\"btn\" href=\"/configure\">Back to configurations</a></div>",
        n = esc(&name), p = esc(&config_path(&name)), w = esc(&c.wiki), prof = esc(&c.profile), pl = esc(&c.plugins.iter().map(|p| p.name.as_str()).collect::<Vec<_>>().join(", ")));
    Response::html(frame(app, "Configured", KICKER, Some(2), None, &body))
}
