// SPDX-License-Identifier: MPL-2.0
//! Writes a rendered wiki as static HTML pages: one page per `*.md` file
//! (names starting with `_` are skipped), a page listing, and ProgBlocks'
//! assets when a page needs them. Shared by `berry-blocks render` and Harness,
//! so what Harness checks is exactly what `render` produces.

use std::fs;
use std::path::{Path, PathBuf};

use berry_blocks_host::{escape_html, render_page, Asset, Block, Profile};

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
pub const LISTING: &str = "_pages.html";

/// Wraps a rendered fragment in an accessible HTML document.
pub fn page_shell(title: &str, body: &str, assets: &[Asset], nav: &str) -> String {
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

/// Renders every page of `wiki` into `out` with `blocks` under `profile`;
/// returns the number of pages written. When any page needs ProgBlocks, its
/// `prog-block.js` and `prog-block.css` are copied from `progblocks_src`.
pub fn render_site(
    wiki: &Path,
    out: &Path,
    blocks: &[&dyn Block],
    profile: Profile,
    progblocks_src: &Path,
) -> Result<usize, String> {
    let entries = fs::read_dir(wiki).map_err(|e| format!("{}: {e}", wiki.display()))?;
    let mut paths = Vec::new();
    for entry in entries {
        paths.push(
            entry
                .map_err(|e| format!("{}: {e}", wiki.display()))?
                .path(),
        );
    }
    let mut pages: Vec<PathBuf> = paths
        .into_iter()
        .filter(|p| p.extension().is_some_and(|x| x == "md"))
        .filter(|p| !p.file_name().unwrap().to_string_lossy().starts_with('_'))
        .collect();
    pages.sort();
    fs::create_dir_all(out).map_err(|e| e.to_string())?;
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
    let mut needs_progblocks = false;
    for page in &pages {
        let md = fs::read_to_string(page).map_err(|e| format!("{}: {e}", page.display()))?;
        let rendered =
            render_page(&md, blocks, profile).map_err(|e| format!("{}: {e}", page.display()))?;
        needs_progblocks |= !rendered.assets.is_empty();
        let title = stem(page).replace(['-', '_'], " ");
        let html = page_shell(&title, &rendered.html, &rendered.assets, &nav);
        fs::write(out.join(format!("{}.html", stem(page))), html).map_err(|e| e.to_string())?;
    }
    let index = page_shell(
        "Pages",
        "<p>Rendered by the berry-blocks lab.</p>\n",
        &[],
        &nav,
    );
    fs::write(out.join(LISTING), index).map_err(|e| e.to_string())?;
    if needs_progblocks {
        let dest = out.join("progblocks");
        fs::create_dir_all(&dest).map_err(|e| e.to_string())?;
        for file in ["prog-block.js", "prog-block.css"] {
            let src = progblocks_src.join(file);
            fs::copy(&src, dest.join(file))
                .map_err(|e| format!("{}: {e} (run scripts/fetch-pins.sh)", src.display()))?;
        }
    }
    Ok(pages.len())
}
