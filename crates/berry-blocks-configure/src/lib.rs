// SPDX-License-Identifier: MPL-2.0
//! Configure: turn plugins on for one wiki and choose how its pages are
//! delivered. The result is `wikis/<name>.kyaml`, which `berry-blocks render
//! --config` reads.
//!
//! As with Mint and Provision, [`plan`] computes everything without writing:
//! the exact file, and which pages of the wiki would render differently (it
//! renders every page to find out). [`apply`] writes only if the digest still
//! matches the preview. The wiki itself is never written.

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use berry_blocks_host::{render_page, Block, Profile};
use sha2::{Digest, Sha256};

/// One enabled plugin and its option values.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PluginConfig {
    /// Registered plugin name.
    pub name: String,
    /// Option values, in file order.
    pub options: Vec<(String, bool)>,
}

/// The contents of `wikis/<name>.kyaml`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WikiConfig {
    /// Comment lines before the opening brace, kept verbatim.
    pub header: Vec<String>,
    /// The wiki folder, relative to the berry-blocks root or absolute.
    pub wiki: String,
    /// `static` or `enhanced`.
    pub profile: String,
    /// Enabled plugins, in render order.
    pub plugins: Vec<PluginConfig>,
}

/// The header written on new configuration files.
pub const HEADER: [&str; 3] = [
    "# SPDX-License-Identifier: MPL-2.0",
    "# berry-blocks wiki configuration, written by the wizard's Configure step.",
    "# Rendered with: berry-blocks render --config <this file> OUT",
];

impl WikiConfig {
    /// Reads the exact KYAML shape [`WikiConfig::render`] writes; refuses anything else.
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut lines = text.lines().peekable();
        let mut header = Vec::new();
        while lines.peek().is_some_and(|l| l.starts_with('#')) {
            header.push(lines.next().unwrap().to_string());
        }
        let mut next = |want: &str| -> Result<String, String> {
            let l = lines.next().ok_or("unexpected end of file")?;
            if want.is_empty() || l == want {
                Ok(l.to_string())
            } else {
                Err(format!("expected `{want}`, got `{l}`"))
            }
        };
        next("{")?;
        let wiki = string_field(&next("")?, "  ", "wiki")?;
        let profile = string_field(&next("")?, "  ", "profile")?;
        next("  plugins: [")?;
        let mut plugins = Vec::new();
        loop {
            let l = next("")?;
            if l == "  ]," {
                break;
            }
            if l != "    {" {
                return Err(format!("expected `    {{`, got `{l}`"));
            }
            let name = string_field(&next("")?, "      ", "name")?;
            let mut options = Vec::new();
            let opt = next("")?;
            if opt != "      options: {}," {
                if opt != "      options: {" {
                    return Err(format!("expected options, got `{opt}`"));
                }
                loop {
                    let l = next("")?;
                    if l == "      }," {
                        break;
                    }
                    let (k, v) = l
                        .strip_prefix("        ")
                        .and_then(|r| r.strip_suffix(','))
                        .and_then(|r| r.split_once(": "))
                        .ok_or_else(|| format!("unexpected option line `{l}`"))?;
                    let v = match v {
                        "true" => true,
                        "false" => false,
                        other => {
                            return Err(format!("option {k} must be true or false, got {other}"))
                        }
                    };
                    options.push((k.to_string(), v));
                }
            }
            next("    },")?;
            plugins.push(PluginConfig { name, options });
        }
        next("}")?;
        if lines.any(|l| !l.trim().is_empty()) {
            return Err("unexpected content after the closing `}`".into());
        }
        Ok(Self {
            header,
            wiki,
            profile,
            plugins,
        })
    }

    /// Writes the canonical KYAML form (what `yq -o kyaml` produces).
    pub fn render(&self) -> String {
        let mut out = String::new();
        for h in &self.header {
            out.push_str(h);
            out.push('\n');
        }
        out.push_str(&format!(
            "{{\n  wiki: \"{}\",\n  profile: \"{}\",\n  plugins: [\n",
            self.wiki, self.profile
        ));
        for p in &self.plugins {
            out.push_str(&format!("    {{\n      name: \"{}\",\n", p.name));
            if p.options.is_empty() {
                out.push_str("      options: {},\n");
            } else {
                out.push_str("      options: {\n");
                for (k, v) in &p.options {
                    out.push_str(&format!("        {k}: {v},\n"));
                }
                out.push_str("      },\n");
            }
            out.push_str("    },\n");
        }
        out.push_str("  ],\n}\n");
        out
    }

    /// The delivery profile.
    pub fn profile(&self) -> Profile {
        if self.profile == "enhanced" {
            Profile::Enhanced
        } else {
            Profile::Static
        }
    }

    /// Builds the enabled plugins from the registry, in order.
    pub fn blocks(&self) -> Result<Vec<Box<dyn Block>>, String> {
        self.plugins
            .iter()
            .map(|p| {
                let entry = berry_blocks_registry::find(&p.name)
                    .ok_or_else(|| format!("plugin {} is not registered", p.name))?;
                let opts: BTreeMap<String, bool> = p.options.iter().cloned().collect();
                Ok((entry.build)(&opts))
            })
            .collect()
    }

    /// The wiki folder resolved against the berry-blocks root.
    pub fn wiki_dir(&self, root: &Path) -> PathBuf {
        let p = PathBuf::from(&self.wiki);
        if p.is_absolute() {
            p
        } else {
            root.join(p)
        }
    }
}

/// Reads `<indent>key: "value",` from one line.
fn string_field(line: &str, indent: &str, key: &str) -> Result<String, String> {
    line.strip_prefix(&format!("{indent}{key}: \""))
        .and_then(|l| l.strip_suffix("\","))
        .filter(|v| !v.contains('"') && !v.contains('\\'))
        .map(str::to_string)
        .ok_or_else(|| format!("expected `{key}: \"…\",`, got `{line}`"))
}

/// What a person asks to configure.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ConfigureRequest {
    /// Configuration name: the file is `wikis/<name>.kyaml`.
    pub name: String,
    /// The wiki folder.
    pub wiki: String,
    /// `static` or `enhanced`.
    pub profile: String,
    /// Enabled plugins with their options.
    pub plugins: Vec<PluginConfig>,
}

/// A problem with one form field.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FieldError {
    /// Form field name.
    pub field: &'static str,
    /// What is wrong and what to do.
    pub message: String,
}

/// One page of the wiki and what the configuration does to it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PageEffect {
    /// Page file name.
    pub page: String,
    /// Number of blocks plugins render on it; 0 means unchanged.
    pub runs: usize,
}

/// Everything configuring would do, computed without writing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    /// Repository path of the configuration file.
    pub path: String,
    /// Its content before (`None` when it is new) and after.
    pub before: Option<String>,
    /// The file's content after the change.
    pub after: String,
    /// Every page of the wiki, with how many blocks plugins render on it.
    pub pages: Vec<PageEffect>,
    /// SHA-256 over the file change and page effects, hex.
    pub digest: String,
}

/// Why configuring was refused.
#[derive(Debug, PartialEq, Eq)]
pub enum ConfigureError {
    /// The request is invalid; nothing was written.
    Invalid(Vec<FieldError>),
    /// The plan no longer matches the preview; nothing was written.
    Stale,
    /// Rendering or I/O failed.
    Failed(String),
}

impl fmt::Display for ConfigureError {
    /// Describes the refusal in one sentence.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigureError::Invalid(e) => write!(f, "{} field(s) need fixing", e.len()),
            ConfigureError::Stale => write!(f, "the plan changed since it was previewed"),
            ConfigureError::Failed(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for ConfigureError {}

/// The wiki's pages: `*.md` files not starting with `_`, sorted.
pub fn pages(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|x| x == "md"))
                .filter(|p| !p.file_name().unwrap().to_string_lossy().starts_with('_'))
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

/// Checks every field; returns all problems at once.
pub fn validate(req: &ConfigureRequest, root: &Path) -> Vec<FieldError> {
    let mut errs = Vec::new();
    let slug = req
        .name
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_lowercase())
        && req.name.len() <= 40
        && req
            .name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if !slug {
        errs.push(FieldError {
            field: "name",
            message: "Use 1 to 40 lowercase letters, digits and hyphens, starting with a letter."
                .into(),
        });
    }
    let dir = WikiConfig {
        header: vec![],
        wiki: req.wiki.clone(),
        profile: String::new(),
        plugins: vec![],
    }
    .wiki_dir(root);
    if req.wiki.is_empty() || req.wiki.contains('"') || req.wiki.contains('\\') || !dir.is_dir() {
        errs.push(FieldError {
            field: "wiki",
            message: "Give a folder that exists: a BerryWiki folder or clone.".into(),
        });
    } else if pages(&dir).is_empty() {
        errs.push(FieldError {
            field: "wiki",
            message: "That folder has no Markdown pages.".into(),
        });
    }
    if req.profile != "static" && req.profile != "enhanced" {
        errs.push(FieldError {
            field: "profile",
            message: "Choose static or enhanced.".into(),
        });
    }
    if req.plugins.is_empty() {
        errs.push(FieldError {
            field: "plugins",
            message: "Turn on at least one plugin.".into(),
        });
    }
    for p in &req.plugins {
        match berry_blocks_registry::find(&p.name) {
            None => errs.push(FieldError {
                field: "plugins",
                message: format!(
                    "{} is not registered, so the renderer cannot run it.",
                    p.name
                ),
            }),
            Some(e) => {
                if !root.join("plugins").join(&p.name).is_dir() {
                    errs.push(FieldError {
                        field: "plugins",
                        message: format!("{} has no manifest in plugins/; mint it first.", p.name),
                    });
                }
                for (k, _) in &p.options {
                    if !e.options.iter().any(|o| o.key == k) {
                        errs.push(FieldError {
                            field: "plugins",
                            message: format!("{} has no option called {k}.", p.name),
                        });
                    }
                }
            }
        }
    }
    errs
}

/// Repository path of a configuration.
pub fn config_path(name: &str) -> String {
    format!("wikis/{name}.kyaml")
}

/// Computes what configuring would do, rendering every page to find out.
pub fn plan(req: &ConfigureRequest, root: &Path) -> Result<Plan, ConfigureError> {
    let errs = validate(req, root);
    if !errs.is_empty() {
        return Err(ConfigureError::Invalid(errs));
    }
    let path = config_path(&req.name);
    let before = fs::read_to_string(root.join(&path)).ok();
    let header = before
        .as_deref()
        .and_then(|b| WikiConfig::parse(b).ok())
        .map(|c| c.header)
        .unwrap_or_else(|| HEADER.iter().map(|s| s.to_string()).collect());
    let config = WikiConfig {
        header,
        wiki: req.wiki.clone(),
        profile: req.profile.clone(),
        plugins: req.plugins.clone(),
    };
    let after = config.render();
    let blocks = config.blocks().map_err(ConfigureError::Failed)?;
    let refs: Vec<&dyn Block> = blocks.iter().map(|b| b.as_ref()).collect();
    let mut effects = Vec::new();
    for page in pages(&config.wiki_dir(root)) {
        let md = fs::read_to_string(&page)
            .map_err(|e| ConfigureError::Failed(format!("{}: {e}", page.display())))?;
        let r = render_page(&md, &refs, config.profile())
            .map_err(|e| ConfigureError::Failed(format!("{}: {e}", page.display())))?;
        effects.push(PageEffect {
            page: page.file_name().unwrap().to_string_lossy().into_owned(),
            runs: r.runs,
        });
    }
    let mut h = Sha256::new();
    h.update(format!("{path}\0{}\0{after}\0", before.as_deref().unwrap_or("")).as_bytes());
    for e in &effects {
        h.update(format!("{}\0{}\0", e.page, e.runs).as_bytes());
    }
    let digest = h.finalize().iter().map(|b| format!("{b:02x}")).collect();
    Ok(Plan {
        path,
        before,
        after,
        pages: effects,
        digest,
    })
}

/// Writes the configuration if and only if it matches the preview.
pub fn apply(
    req: &ConfigureRequest,
    root: &Path,
    previewed_digest: &str,
) -> Result<Plan, ConfigureError> {
    let plan = plan(req, root)?;
    if plan.digest != previewed_digest {
        return Err(ConfigureError::Stale);
    }
    let target = root.join(&plan.path);
    let tmp = target.with_extension("berry-blocks-tmp");
    fs::create_dir_all(target.parent().unwrap())
        .and_then(|_| fs::write(&tmp, &plan.after))
        .and_then(|_| fs::rename(&tmp, &target))
        .map_err(|e| ConfigureError::Failed(format!("{}: {e}", plan.path)))?;
    Ok(plan)
}

/// Loads a configuration file and builds its plugins.
pub fn load(path: &Path) -> Result<(WikiConfig, Vec<Box<dyn Block>>), String> {
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let config = WikiConfig::parse(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    let blocks = config.blocks()?;
    Ok((config, blocks))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A config with options, one without, as `yq -o kyaml` writes it.
    const SAMPLE: &str = "# SPDX-License-Identifier: MPL-2.0\n{\n  wiki: \"fixtures/lab-wiki\",\n  profile: \"enhanced\",\n  plugins: [\n    {\n      name: \"progblocks\",\n      options: {\n        persist: true,\n      },\n    },\n    {\n      name: \"callout\",\n      options: {},\n    },\n  ],\n}\n";

    #[test]
    /// Reading and writing round-trips the canonical form exactly.
    fn round_trips() {
        let c = WikiConfig::parse(SAMPLE).unwrap();
        assert_eq!(c.plugins[0].options, vec![("persist".to_string(), true)]);
        assert!(c.plugins[1].options.is_empty());
        assert_eq!(c.render(), SAMPLE);
    }

    #[test]
    /// Other shapes and non-boolean options are refused.
    fn refuses_other_shapes() {
        assert!(WikiConfig::parse(&SAMPLE.replace("persist: true", "persist: yes")).is_err());
        assert!(WikiConfig::parse(&SAMPLE.replace("  profile:", "  mode:")).is_err());
        assert!(WikiConfig::parse(&format!("{SAMPLE}x\n")).is_err());
    }
}
