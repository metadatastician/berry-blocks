// SPDX-License-Identifier: MPL-2.0
//! The berry-blocks plugin host.
//!
//! A *block* is a plugin that claims runs of fenced code blocks in a BerryWiki
//! page and renders them differently. The host never renders Markdown itself:
//!
//! 1. it finds the fence runs a block claims, by source position;
//! 2. it replaces each run, in the Markdown, with one empty fence whose language
//!    is a one-off marker;
//! 3. it hands the page to **BerryWiki's own** `render_markdown`, so BerryWiki
//!    stays the authority on escaping, URL neutralising and heading levels;
//! 4. it swaps each marker's `<pre><code class="language-…"></code></pre>` for
//!    the block's HTML, failing closed unless every marker appears exactly once.
//!
//! BerryWiki does not know this crate exists, and never will (ADR-0001).

use std::collections::hash_map::RandomState;
use std::fmt;
use std::hash::{BuildHasher, Hasher};

use comrak::nodes::{AstNode, NodeValue};
use comrak::{parse_document, Arena, Options};

/// How a page is delivered. `Static` is BerryWiki-compatible: no script, ever.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Profile {
    /// Plain HTML and CSS only; the page must contain no `<script>`.
    Static,
    /// Static HTML upgraded by a script when one runs (progressive enhancement).
    Enhanced,
}

/// One fenced code block: its language token, `key=value` metadata and text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fence {
    /// First word of the info string (`bash` in "```bash variant=macOS").
    pub lang: String,
    /// The remaining `key=value` pairs, in order. Values may be double-quoted.
    pub meta: Vec<(String, String)>,
    /// The fence's literal contents.
    pub code: String,
}

impl Fence {
    /// Returns the value of a metadata key, if present.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.meta
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }
}

/// Consecutive fences that one block renders as a single unit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FenceRun {
    /// The fences, in document order.
    pub fences: Vec<Fence>,
}

/// A page-level resource a block needs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Asset {
    /// A stylesheet URL, relative to the page.
    Stylesheet(String),
    /// An ES module script URL, relative to the page. Never allowed in `Static`.
    ModuleScript(String),
}

/// A berry-blocks plugin. See docs/decisions/ADR-0002-plugin-contract.adoc.
pub trait Block {
    /// Stable short name, used in errors and reports.
    fn name(&self) -> &'static str;
    /// Whether this block wants to render the given fence.
    fn claims(&self, fence: &Fence) -> bool;
    /// Whether `next` continues the run that `previous` is part of.
    fn continues(&self, previous: &Fence, next: &Fence) -> bool;
    /// Renders one run as an HTML fragment for the given profile.
    fn render(&self, run: &FenceRun, profile: Profile) -> String;
    /// The page assets this block needs when it rendered at least one run.
    fn assets(&self, profile: Profile) -> Vec<Asset>;
}

/// A rendered page fragment plus the assets its blocks asked for.
#[derive(Debug, PartialEq, Eq)]
pub struct Rendered {
    /// The HTML fragment (no `<html>` shell).
    pub html: String,
    /// De-duplicated assets, in first-requested order.
    pub assets: Vec<Asset>,
    /// How many fence runs blocks rendered.
    pub runs: usize,
}

/// Why the host refused to produce a page.
#[derive(Debug, PartialEq, Eq)]
pub enum HostError {
    /// A marker did not appear exactly once in BerryWiki's output.
    MarkerMismatch { marker: String, found: usize },
    /// A block asked for a script under the `Static` profile.
    ScriptInStaticProfile { block: &'static str },
    /// The finished `Static` page contains `<script`.
    ScriptInStaticOutput,
}

impl fmt::Display for HostError {
    /// Describes the refusal in one line.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HostError::MarkerMismatch { marker, found } => {
                write!(
                    f,
                    "marker {marker} appeared {found} times in BerryWiki output, expected 1"
                )
            }
            HostError::ScriptInStaticProfile { block } => {
                write!(
                    f,
                    "block {block} requested a script under the static profile"
                )
            }
            HostError::ScriptInStaticOutput => write!(f, "static page output contains <script"),
        }
    }
}

impl std::error::Error for HostError {}

/// Escapes text for use in HTML element content and double-quoted attributes.
pub fn escape_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

/// Splits a fence info string into its language token and `key=value` pairs.
///
/// Values may be wrapped in double quotes to contain spaces. A bare word is a
/// flag and is recorded with an empty value (`persist` -> ("persist", "")).
pub fn parse_info(info: &str) -> (String, Vec<(String, String)>) {
    let info = info.trim();
    let (lang, rest) = match info.find(char::is_whitespace) {
        Some(i) => (&info[..i], &info[i..]),
        None => (info, ""),
    };
    let mut meta = Vec::new();
    let mut chars = rest.chars().peekable();
    loop {
        while chars.peek().is_some_and(|c| c.is_whitespace()) {
            chars.next();
        }
        let mut key = String::new();
        while let Some(&c) = chars.peek() {
            if c == '=' || c.is_whitespace() {
                break;
            }
            key.push(c);
            chars.next();
        }
        if key.is_empty() && chars.peek().is_none() {
            break;
        }
        if chars.peek() != Some(&'=') {
            meta.push((key, String::new())); // a bare flag
            continue;
        }
        chars.next();
        let mut value = String::new();
        if chars.peek() == Some(&'"') {
            chars.next();
            for c in chars.by_ref() {
                if c == '"' {
                    break;
                }
                value.push(c);
            }
        } else {
            while let Some(&c) = chars.peek() {
                if c.is_whitespace() {
                    break;
                }
                value.push(c);
                chars.next();
            }
        }
        if !key.is_empty() {
            meta.push((key, value));
        }
    }
    (lang.to_string(), meta)
}

/// A claimed run, located by the 1-based source lines it spans.
struct LocatedRun {
    block: usize,
    first_line: usize,
    last_line: usize,
    run: FenceRun,
}

/// Parses with BerryWiki's extension set so fences are found where BerryWiki
/// will find them. Used only for locating, never for output.
fn locating_options() -> Options<'static> {
    let mut o = Options::default();
    o.extension.table = true;
    o.extension.strikethrough = true;
    o.extension.tasklist = true;
    o.extension.autolink = true;
    o.extension.tagfilter = true;
    o.extension.footnotes = true;
    o
}

/// Collects top-level fence runs that some block claims, in document order.
///
/// Fences join one run only when nothing but blank lines separates them.
/// Some Markdown constructs, such as link reference definitions, produce no
/// AST node, so AST adjacency alone would swallow them into the replaced span.
fn locate_runs(markdown: &str, blocks: &[&dyn Block]) -> Vec<LocatedRun> {
    let lines: Vec<&str> = markdown.split_inclusive('\n').collect();
    let only_blank_between = |after: usize, before: usize| {
        (after + 1..before).all(|n| lines.get(n - 1).is_none_or(|l| l.trim().is_empty()))
    };
    let arena = Arena::new();
    let root = parse_document(&arena, markdown, &locating_options());
    let mut runs: Vec<LocatedRun> = Vec::new();
    let mut previous: Option<(usize, Fence, usize)> = None; // (block, fence, last line)
    for node in root.children() {
        let fence = fence_of(node);
        let (start, end) = {
            let pos = node.data.borrow().sourcepos;
            (pos.start.line, pos.end.line)
        };
        match fence {
            Some(f) => {
                let owner = blocks.iter().position(|b| b.claims(&f));
                match (owner, previous.take()) {
                    (Some(b), Some((pb, pf, prev_end)))
                        if b == pb
                            && blocks[b].continues(&pf, &f)
                            && only_blank_between(prev_end, start) =>
                    {
                        let current = runs.last_mut().expect("a previous claimed fence has a run");
                        current.last_line = end;
                        current.run.fences.push(f.clone());
                        previous = Some((b, f, end));
                    }
                    (Some(b), _) => {
                        runs.push(LocatedRun {
                            block: b,
                            first_line: start,
                            last_line: end,
                            run: FenceRun {
                                fences: vec![f.clone()],
                            },
                        });
                        previous = Some((b, f, end));
                    }
                    (None, _) => previous = None,
                }
            }
            None => previous = None,
        }
    }
    runs
}

/// Returns the fence held by a node, if it is a fenced code block.
fn fence_of<'a>(node: &'a AstNode<'a>) -> Option<Fence> {
    match &node.data.borrow().value {
        NodeValue::CodeBlock(cb) if cb.fenced => {
            let (lang, meta) = parse_info(&cb.info);
            Some(Fence {
                lang,
                meta,
                code: cb.literal.clone(),
            })
        }
        _ => None,
    }
}

/// A marker that does not occur anywhere in the page source.
fn fresh_marker(markdown: &str, index: usize) -> String {
    let state = RandomState::new();
    loop {
        let mut h = state.build_hasher();
        h.write(markdown.as_bytes());
        h.write_usize(index);
        let marker = format!("berry-blocks-slot-{:016x}-{index}", h.finish());
        if !markdown.contains(&marker) {
            return marker;
        }
    }
}

/// Renders a BerryWiki page with the given blocks under one profile.
pub fn render_page(
    markdown: &str,
    blocks: &[&dyn Block],
    profile: Profile,
) -> Result<Rendered, HostError> {
    let runs = locate_runs(markdown, blocks);
    let lines: Vec<&str> = markdown.split_inclusive('\n').collect();
    let mut spliced = String::with_capacity(markdown.len());
    let mut markers = Vec::with_capacity(runs.len());
    let mut line = 1;
    for (i, run) in runs.iter().enumerate() {
        while line < run.first_line {
            spliced.push_str(lines[line - 1]);
            line += 1;
        }
        let marker = fresh_marker(markdown, i);
        spliced.push_str(&format!("```{marker}\n```\n"));
        markers.push(marker);
        line = run.last_line + 1;
    }
    while line <= lines.len() {
        spliced.push_str(lines[line - 1]);
        line += 1;
    }

    let mut html = berrywiki_render::render_markdown(&spliced);
    let mut assets: Vec<Asset> = Vec::new();
    for (run, marker) in runs.iter().zip(&markers) {
        let slot = format!("<pre><code class=\"language-{marker}\"></code></pre>\n");
        let found = html.matches(&slot).count();
        if found != 1 {
            return Err(HostError::MarkerMismatch {
                marker: marker.clone(),
                found,
            });
        }
        let block = blocks[run.block];
        html = html.replacen(&slot, &block.render(&run.run, profile), 1);
        for asset in block.assets(profile) {
            if profile == Profile::Static && matches!(asset, Asset::ModuleScript(_)) {
                return Err(HostError::ScriptInStaticProfile {
                    block: block.name(),
                });
            }
            if !assets.contains(&asset) {
                assets.push(asset);
            }
        }
    }
    if profile == Profile::Static && html.to_ascii_lowercase().contains("<script") {
        return Err(HostError::ScriptInStaticOutput);
    }
    Ok(Rendered {
        html,
        assets,
        runs: runs.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A block that claims `x` fences and renders a fixed string, for host tests.
    struct Fixed;
    impl Block for Fixed {
        /// Test name.
        fn name(&self) -> &'static str {
            "fixed"
        }
        /// Claims fences whose language is `x`.
        fn claims(&self, f: &Fence) -> bool {
            f.lang == "x"
        }
        /// Every adjacent `x` fence continues the run.
        fn continues(&self, _: &Fence, _: &Fence) -> bool {
            true
        }
        /// Renders the run size.
        fn render(&self, run: &FenceRun, _: Profile) -> String {
            format!("<p>RUN {}</p>\n", run.fences.len())
        }
        /// Asks for a script, to test the static-profile refusal.
        fn assets(&self, p: Profile) -> Vec<Asset> {
            match p {
                Profile::Static => vec![],
                Profile::Enhanced => vec![Asset::ModuleScript("x.js".into())],
            }
        }
    }

    #[test]
    /// Info strings split into language and quoted or bare key=value pairs.
    fn parses_info_strings() {
        let (lang, meta) =
            parse_info(r#"bash variant=macOS group=os label="Operating system" persist"#);
        assert_eq!(lang, "bash");
        assert_eq!(
            meta,
            vec![
                ("variant".into(), "macOS".into()),
                ("group".into(), "os".into()),
                ("label".into(), "Operating system".into()),
                ("persist".into(), String::new())
            ]
        );
    }

    #[test]
    /// Pages with no claimed fences come out exactly as BerryWiki renders them.
    fn unclaimed_pages_are_byte_identical_to_berrywiki() {
        let md = "# T\n\nText [x](javascript:alert(1)) <b>raw</b>\n\n```rust\nfn main() {}\n```\n";
        let r = render_page(md, &[&Fixed], Profile::Static).unwrap();
        assert_eq!(r.html, berrywiki_render::render_markdown(md));
        assert_eq!(r.runs, 0);
    }

    #[test]
    /// Adjacent claimed fences form one run; others stay BerryWiki's.
    fn adjacent_fences_form_one_run() {
        let md = "A\n\n```x\n1\n```\n\n```x\n2\n```\n\nB\n\n```x\n3\n```\n";
        let r = render_page(md, &[&Fixed], Profile::Enhanced).unwrap();
        assert_eq!(r.runs, 2);
        assert!(r.html.contains("<p>RUN 2</p>"));
        assert!(r.html.contains("<p>RUN 1</p>"));
        assert!(r.html.contains("<p>B</p>"));
        assert_eq!(r.assets, vec![Asset::ModuleScript("x.js".into())]);
    }

    #[test]
    /// Content with no AST node (a link reference definition) between two
    /// claimed fences splits the run, so it is never deleted from the page.
    fn reference_definitions_between_fences_survive() {
        let md = "```x\n1\n```\n\n[ref]: https://example.org\n\n```x\n2\n```\n\nSee [docs][ref].\n";
        let r = render_page(md, &[&Fixed], Profile::Static).unwrap();
        assert_eq!(r.runs, 2);
        assert!(
            r.html.contains("<a href=\"https://example.org\">docs</a>"),
            "{}",
            r.html
        );
    }

    #[test]
    /// Fences nested in a list are not claimed in this version.
    fn nested_fences_are_left_to_berrywiki() {
        let md = "- item\n\n  ```x\n  1\n  ```\n";
        let r = render_page(md, &[&Fixed], Profile::Static).unwrap();
        assert_eq!(r.runs, 0);
        assert_eq!(r.html, berrywiki_render::render_markdown(md));
    }

    #[test]
    /// A page whose source already contains the marker text cannot spoof a slot.
    fn source_text_cannot_impersonate_a_marker() {
        let md = "```x\n1\n```\n\n```berry-blocks-slot-0000000000000000-0\n```\n";
        let r = render_page(md, &[&Fixed], Profile::Static).unwrap();
        assert_eq!(r.runs, 1);
        assert!(r
            .html
            .contains("language-berry-blocks-slot-0000000000000000-0"));
    }
}
