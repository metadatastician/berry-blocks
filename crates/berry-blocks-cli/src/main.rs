// SPDX-License-Identifier: MPL-2.0
//! `berry-blocks` — render a BerryWiki folder through the lab's blocks.
//!
//! ```text
//! berry-blocks render --profile static|enhanced [--progblocks DIR] WIKI OUT
//! ```
//!
//! Writes `OUT/<page>.html` for every `*.md` page (files starting with `_`, such
//! as `_Sidebar.md`, are skipped) and the listing `OUT/_pages.html`. The enhanced profile also
//! copies ProgBlocks' `prog-block.js` and `prog-block.css` from DIR (default
//! `vendor/progblocks/src`, filled by `scripts/fetch-pins.sh`) to
//! `OUT/progblocks/`. These pages are a lab artefact: BerryWiki itself never
//! serves them.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use berry_blocks_host::{escape_html, render_page, Asset, Profile};
use berry_blocks_progblocks::ProgBlocks;

/// Lab page styles: readable code, visible focus, clear variant summaries.
const CSS: &str = "body{font-family:system-ui,sans-serif;max-width:52rem;margin:auto;padding:1rem;line-height:1.5;color:#1b1b1b;background:#fff}\
pre{background:#f4f4f4;padding:.75rem;overflow-x:auto}\
code{font-family:ui-monospace,Menlo,Consolas,monospace}\
.bb-variants{border:1px solid #767676;border-radius:4px;margin:1rem 0}\
.bb-variant summary{cursor:pointer;padding:.4rem .75rem;font-weight:600}\
.bb-variant[open] summary{border-bottom:1px solid #767676}\
.bb-variant pre{margin:0}\
:focus-visible{outline:3px solid #005fcc;outline-offset:2px}\
nav a{margin-right:1rem}";

/// The page listing's file name. Pages whose names start with `_` are skipped,
/// so no wiki page can render to this name and be overwritten by the listing.
const LISTING: &str = "_pages.html";

/// Parsed command-line options for `render`.
struct Args {
    profile: Profile,
    progblocks: PathBuf,
    wiki: PathBuf,
    out: PathBuf,
}

/// Parses `render` arguments; returns a usage message on error.
fn parse_args(raw: &[String]) -> Result<Args, String> {
    let usage = "usage: berry-blocks render --profile static|enhanced [--progblocks DIR] WIKI OUT";
    let mut it = raw.iter();
    if it.next().map(String::as_str) != Some("render") {
        return Err(usage.into());
    }
    let (mut profile, mut progblocks, mut positional) =
        (None, PathBuf::from("vendor/progblocks/src"), Vec::new());
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--profile" => {
                profile = match it.next().map(String::as_str) {
                    Some("static") => Some(Profile::Static),
                    Some("enhanced") => Some(Profile::Enhanced),
                    _ => return Err(usage.into()),
                }
            }
            "--progblocks" => progblocks = it.next().ok_or(usage)?.into(),
            other => positional.push(PathBuf::from(other)),
        }
    }
    match (profile, <[PathBuf; 2]>::try_from(positional)) {
        (Some(profile), Ok([wiki, out])) => Ok(Args {
            profile,
            progblocks,
            wiki,
            out,
        }),
        _ => Err(usage.into()),
    }
}

/// Wraps a rendered fragment in an accessible HTML document.
fn page_shell(title: &str, body: &str, assets: &[Asset], nav: &str) -> String {
    let mut head = String::new();
    for asset in assets {
        match asset {
            Asset::Stylesheet(href) => head.push_str(&format!(
                "<link rel=\"stylesheet\" href=\"{}\">\n",
                escape_html(href)
            )),
            Asset::ModuleScript(src) => head.push_str(&format!(
                "<script type=\"module\" src=\"{}\"></script>\n",
                escape_html(src)
            )),
        }
    }
    format!(
        "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n<title>{t}</title>\n<style>{CSS}</style>\n{head}</head>\n<body>\n<nav aria-label=\"Pages\">{nav}</nav>\n<main>\n<h1>{t}</h1>\n{body}</main>\n</body>\n</html>\n",
        t = escape_html(title)
    )
}

/// Renders every page of the wiki into OUT; returns the number of pages written.
fn render(args: &Args) -> Result<usize, String> {
    let entries = fs::read_dir(&args.wiki).map_err(|e| format!("{}: {e}", args.wiki.display()))?;
    let mut paths = Vec::new();
    for entry in entries {
        paths.push(
            entry
                .map_err(|e| format!("{}: {e}", args.wiki.display()))?
                .path(),
        );
    }
    let mut pages: Vec<PathBuf> = paths
        .into_iter()
        .filter(|p| p.extension().is_some_and(|x| x == "md"))
        .filter(|p| !p.file_name().unwrap().to_string_lossy().starts_with('_'))
        .collect();
    pages.sort();
    fs::create_dir_all(&args.out).map_err(|e| e.to_string())?;
    let stem = |p: &Path| p.file_stem().unwrap().to_string_lossy().into_owned();
    let nav: String = pages
        .iter()
        .map(|p| {
            format!(
                "<a href=\"{0}.html\">{1}</a>",
                escape_html(&stem(p)),
                escape_html(&stem(p).replace(['-', '_'], " "))
            )
        })
        .collect();
    let block = ProgBlocks::default();
    let mut needs_progblocks = false;
    for page in &pages {
        let md = fs::read_to_string(page).map_err(|e| format!("{}: {e}", page.display()))?;
        let rendered = render_page(&md, &[&block], args.profile)
            .map_err(|e| format!("{}: {e}", page.display()))?;
        needs_progblocks |= !rendered.assets.is_empty();
        let title = stem(page).replace(['-', '_'], " ");
        let html = page_shell(&title, &rendered.html, &rendered.assets, &nav);
        fs::write(args.out.join(format!("{}.html", stem(page))), html)
            .map_err(|e| e.to_string())?;
    }
    let index = page_shell(
        "Pages",
        "<p>Rendered by the berry-blocks lab.</p>\n",
        &[],
        &nav,
    );
    fs::write(args.out.join(LISTING), index).map_err(|e| e.to_string())?;
    if needs_progblocks {
        let dest = args.out.join("progblocks");
        fs::create_dir_all(&dest).map_err(|e| e.to_string())?;
        for file in ["prog-block.js", "prog-block.css"] {
            let src = args.progblocks.join(file);
            fs::copy(&src, dest.join(file))
                .map_err(|e| format!("{}: {e} (run scripts/fetch-pins.sh)", src.display()))?;
        }
    }
    Ok(pages.len())
}

/// Entry point: runs `render`, printing a one-line result or an error.
fn main() -> ExitCode {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    match parse_args(&raw).and_then(|a| render(&a).map(|n| (n, a))) {
        Ok((n, a)) => {
            println!(
                "rendered {n} pages to {} ({:?} profile)",
                a.out.display(),
                a.profile
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("berry-blocks: {e}");
            ExitCode::from(2)
        }
    }
}
