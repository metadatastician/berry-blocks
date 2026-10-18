// SPDX-License-Identifier: MPL-2.0
//! ProgBlocks as a berry-blocks block.
//!
//! Authors mark variants on ordinary fences, which GitHub's own wiki still
//! renders as plain code blocks:
//!
//! ````markdown
//! ```bash variant=macOS group=os label="Operating system"
//! brew install {{ package = ripgrep }}
//! ```
//! ```bash variant=Linux group=os
//! sudo apt-get install {{ package = ripgrep }}
//! ```
//! ````
//!
//! * **Static profile** (BerryWiki-compatible, no script): every variant is a
//!   native `<details>` element whose summary is the variant name, inside a
//!   labelled group; `{{ name = default }}` shows its default.
//! * **Enhanced profile**: the same static markup, wrapped in `<prog-block>`
//!   with one `<template data-variant>` per variant. Without script the static
//!   markup is what readers get; when ProgBlocks loads, it upgrades the block to
//!   tabs with editable variables. ProgBlocks is never required.

use berry_blocks_host::{escape_html, Asset, Block, Fence, FenceRun, Profile};

/// The block. Stateless; configuration lives on the fences.
pub struct ProgBlocks {
    /// Page-relative URL of ProgBlocks' module, used by the enhanced profile.
    pub module_url: String,
    /// Remember each reader's variant choice for every grouped block, as if
    /// each group's first fence said `persist` (a wiki-level option).
    pub persist_by_default: bool,
}

impl Default for ProgBlocks {
    /// Uses the lab's layout: ProgBlocks copied to `progblocks/` beside pages.
    fn default() -> Self {
        Self {
            module_url: "progblocks/prog-block.js".into(),
            persist_by_default: false,
        }
    }
}

/// Replaces `{{ name = default }}` with its default; leaves `{{ name }}` as written.
pub fn resolve_defaults(code: &str) -> String {
    let mut out = String::with_capacity(code.len());
    let mut rest = code;
    while let Some(open) = rest.find("{{") {
        let Some(close) = rest[open..].find("}}") else {
            break;
        };
        let inner = &rest[open + 2..open + close];
        out.push_str(&rest[..open]);
        match inner.split_once('=') {
            Some((name, default)) if is_var_name(name.trim()) => out.push_str(default.trim()),
            _ => out.push_str(&rest[open..open + close + 2]),
        }
        rest = &rest[open + close + 2..];
    }
    out.push_str(rest);
    out
}

/// True for names ProgBlocks accepts: letters, digits, `_`, `-`, `:`.
fn is_var_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | ':'))
}

/// Renders one variant's code as a `<pre><code>` with its language class.
fn code_block(fence: &Fence, code: &str) -> String {
    let class = if fence.lang.is_empty() {
        String::new()
    } else {
        format!(" class=\"language-{}\"", escape_html(&fence.lang))
    };
    format!("<pre><code{class}>{}</code></pre>", escape_html(code))
}

/// The script-free rendering every profile includes.
fn static_markup(run: &FenceRun, label: &str) -> String {
    let mut html = format!(
        "<div class=\"bb-variants\" role=\"group\" aria-label=\"{}\">\n",
        escape_html(label)
    );
    for (i, fence) in run.fences.iter().enumerate() {
        let open = if i == 0 { " open" } else { "" };
        html.push_str(&format!(
            "<details class=\"bb-variant\"{open}><summary>{}</summary>{}</details>\n",
            escape_html(fence.get("variant").unwrap_or("Example")),
            code_block(fence, &resolve_defaults(&fence.code)),
        ));
    }
    html.push_str("</div>\n");
    html
}

impl Block for ProgBlocks {
    /// Short name for errors and reports.
    fn name(&self) -> &'static str {
        "progblocks"
    }

    /// Claims any fence that names a variant.
    fn claims(&self, fence: &Fence) -> bool {
        fence.get("variant").is_some()
    }

    /// A run continues while fences stay in the same `group` (or both have none).
    fn continues(&self, previous: &Fence, next: &Fence) -> bool {
        previous.get("group") == next.get("group")
    }

    /// Renders static markup, wrapped in `<prog-block>` for the enhanced profile.
    fn render(&self, run: &FenceRun, profile: Profile) -> String {
        let first = &run.fences[0];
        let label = run
            .fences
            .iter()
            .find_map(|f| f.get("label"))
            .unwrap_or("Example variants");
        let fallback = static_markup(run, label);
        match profile {
            Profile::Static => fallback,
            Profile::Enhanced => {
                let mut attrs = format!(" label=\"{}\"", escape_html(label));
                if let Some(group) = first.get("group") {
                    attrs.push_str(&format!(" group=\"{}\"", escape_html(group)));
                    if self.persist_by_default
                        || run.fences.iter().any(|f| f.get("persist").is_some())
                    {
                        attrs.push_str(" persist");
                    }
                }
                if !first.lang.is_empty() {
                    attrs.push_str(&format!(" language=\"{}\"", escape_html(&first.lang)));
                }
                let mut html = format!("<prog-block{attrs}>\n");
                for fence in &run.fences {
                    html.push_str(&format!(
                        "<template data-variant=\"{}\">{}</template>\n",
                        escape_html(fence.get("variant").unwrap_or("Example")),
                        escape_html(&fence.code),
                    ));
                }
                html.push_str(&fallback);
                html.push_str("</prog-block>\n");
                html
            }
        }
    }

    /// The enhanced profile needs ProgBlocks' module; the static one needs nothing.
    fn assets(&self, profile: Profile) -> Vec<Asset> {
        match profile {
            Profile::Static => vec![],
            Profile::Enhanced => vec![Asset::ModuleScript(self.module_url.clone())],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use berry_blocks_host::render_page;

    const PAGE: &str = "# Install\n\n```bash variant=macOS group=os label=\"Operating system\"\nbrew install {{ package = ripgrep }}\n```\n\n```bash variant=Linux group=os\nsudo apt-get install {{ package = ripgrep }} && echo '<b>'\n```\n\nThen run it.\n";

    #[test]
    /// Defaults resolve; bare variables and non-variables stay as written.
    fn resolves_defaults_only() {
        assert_eq!(
            resolve_defaults("a {{ x = 1 }} b {{ y }} c {{ a b = 2 }}"),
            "a 1 b {{ y }} c {{ a b = 2 }}"
        );
    }

    #[test]
    /// Static output: every variant present, escaped, defaults shown, no script.
    fn static_profile_is_script_free_and_complete() {
        let r = render_page(PAGE, &[&ProgBlocks::default()], Profile::Static).unwrap();
        assert_eq!(r.runs, 1);
        assert!(r.assets.is_empty());
        assert!(r.html.contains("aria-label=\"Operating system\""));
        assert!(r.html.contains("<summary>macOS</summary>"));
        assert!(r.html.contains("<summary>Linux</summary>"));
        assert!(r.html.contains("brew install ripgrep"));
        assert!(r.html.contains("echo &#39;&lt;b&gt;&#39;"));
        assert!(!r.html.contains("<b>"));
        assert!(!r.html.to_ascii_lowercase().contains("<script"));
        assert!(r.html.contains("<p>Then run it.</p>"));
    }

    #[test]
    /// Enhanced output: templates keep the variables; the static markup is inside.
    fn enhanced_profile_wraps_the_static_fallback() {
        let r = render_page(PAGE, &[&ProgBlocks::default()], Profile::Enhanced).unwrap();
        assert!(r
            .html
            .contains("<prog-block label=\"Operating system\" group=\"os\" language=\"bash\">"));
        assert!(r.html.contains(
            "<template data-variant=\"macOS\">brew install {{ package = ripgrep }}\n</template>"
        ));
        assert!(r
            .html
            .contains("<details class=\"bb-variant\" open><summary>macOS</summary>"));
        assert_eq!(
            r.assets,
            vec![Asset::ModuleScript("progblocks/prog-block.js".into())]
        );
    }

    #[test]
    /// The wiki-level option adds `persist` to grouped blocks only.
    fn persist_by_default_marks_grouped_blocks() {
        let block = ProgBlocks {
            persist_by_default: true,
            ..Default::default()
        };
        let grouped = render_page(
            "```sh variant=a group=g\n1\n```\n",
            &[&block],
            Profile::Enhanced,
        )
        .unwrap();
        assert!(grouped.html.contains(" persist"));
        let ungrouped =
            render_page("```sh variant=a\n1\n```\n", &[&block], Profile::Enhanced).unwrap();
        assert!(!ungrouped.html.contains(" persist"));
    }

    #[test]
    /// Different groups make separate blocks.
    fn group_change_splits_runs() {
        let md = "```sh variant=a group=x\n1\n```\n\n```sh variant=b group=y\n2\n```\n";
        let r = render_page(md, &[&ProgBlocks::default()], Profile::Static).unwrap();
        assert_eq!(r.runs, 2);
    }

    #[test]
    /// Markup in variant names and labels is escaped.
    fn names_and_labels_are_escaped() {
        let md = "```sh variant=\"<img src=x>\" label=\"<i>\"\n1\n```\n";
        let r = render_page(md, &[&ProgBlocks::default()], Profile::Enhanced).unwrap();
        assert!(!r.html.contains("<img"));
        assert!(!r.html.contains("<i>"));
    }
}
