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

use berry_blocks_host::Profile;
use berry_blocks_progblocks::ProgBlocks;

/// Parsed command-line options for `render`.
struct Args {
    profile: Profile,
    progblocks: PathBuf,
    wiki: PathBuf,
    out: PathBuf,
    /// A wiki configuration (`--config`); its wiki, profile and plugins win.
    config: Option<PathBuf>,
}

/// Parses `render` arguments; returns a usage message on error.
fn parse_args(raw: &[String]) -> Result<Args, String> {
    let usage = "usage: berry-blocks render --profile static|enhanced [--progblocks DIR] WIKI OUT\n       berry-blocks render --config wikis/NAME.kyaml [--progblocks DIR] OUT";
    let mut it = raw.iter();
    if it.next().map(String::as_str) != Some("render") {
        return Err(usage.into());
    }
    let (mut profile, mut progblocks, mut positional, mut config) = (
        None,
        PathBuf::from("vendor/progblocks/src"),
        Vec::new(),
        None,
    );
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
            "--config" => config = Some(PathBuf::from(it.next().ok_or(usage)?)),
            other => positional.push(PathBuf::from(other)),
        }
    }
    if let Some(config) = config {
        let [out] = <[PathBuf; 1]>::try_from(positional).map_err(|_| usage.to_string())?;
        if profile.is_some() {
            return Err("--config sets the profile; do not also pass --profile".into());
        }
        return Ok(Args {
            profile: Profile::Static,
            progblocks,
            wiki: PathBuf::new(),
            out,
            config: Some(config),
        });
    }
    match (profile, <[PathBuf; 2]>::try_from(positional)) {
        (Some(profile), Ok([wiki, out])) => Ok(Args {
            profile,
            progblocks,
            wiki,
            out,
            config: None,
        }),
        _ => Err(usage.into()),
    }
}

/// Takes the wiki folder and profile from `--config`, when one was given.
fn with_config(mut args: Args) -> Result<Args, String> {
    if let Some(path) = &args.config {
        let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let cfg = berry_blocks_configure::WikiConfig::parse(&text)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        args.wiki = cfg.wiki_dir(Path::new("."));
        args.profile = cfg.profile();
    }
    Ok(args)
}

/// Renders every page of the wiki into OUT; returns the number of pages written.
fn render(args: &Args) -> Result<usize, String> {
    let default_block = ProgBlocks::default();
    let configured = match &args.config {
        Some(path) => Some(berry_blocks_configure::load(path)?.1),
        None => None,
    };
    let blocks: Vec<&dyn berry_blocks_host::Block> = match &configured {
        Some(b) => b.iter().map(|b| b.as_ref()).collect(),
        None => vec![&default_block],
    };
    berry_blocks_site::render_site(
        &args.wiki,
        &args.out,
        &blocks,
        args.profile,
        &args.progblocks,
    )
}

/// Runs `berry-blocks wizard [--addr HOST:PORT] [--root DIR]` until stopped.
fn wizard(raw: &[String]) -> ExitCode {
    let mut addr = "127.0.0.1:23880".to_string();
    let mut root = std::path::PathBuf::from(".");
    let mut it = raw.iter().skip(1);
    while let Some(arg) = it.next() {
        match (arg.as_str(), it.next()) {
            ("--addr", Some(v)) => addr = v.clone(),
            ("--root", Some(v)) => root = v.into(),
            _ => {
                eprintln!("usage: berry-blocks wizard [--addr HOST:PORT] [--root DIR]");
                return ExitCode::from(2);
            }
        }
    }
    let root = match root.canonicalize() {
        Ok(r) if r.join("Cargo.toml").is_file() => r,
        _ => {
            eprintln!(
                "berry-blocks: {} is not a berry-blocks checkout",
                root.display()
            );
            return ExitCode::from(2);
        }
    };
    println!(
        "berry-blocks wizard on http://{addr}/ writing into {}",
        root.display()
    );
    match berry_blocks_wizard::serve(&berry_blocks_wizard::App { root, addr }) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("berry-blocks: {e}");
            ExitCode::from(2)
        }
    }
}

/// Entry point: `render` or `wizard`.
fn main() -> ExitCode {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    if raw.first().map(String::as_str) == Some("wizard") {
        return wizard(&raw);
    }
    match parse_args(&raw)
        .and_then(with_config)
        .and_then(|a| render(&a).map(|n| (n, a)))
    {
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
