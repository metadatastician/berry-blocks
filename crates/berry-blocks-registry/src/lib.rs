// SPDX-License-Identifier: MPL-2.0
//! The registry: every plugin the renderer can run, its options, and how to
//! build it from a wiki configuration. Plugins are compiled in, so the list is
//! checked by the compiler; there is no dynamic loading (ADR-0002, question 2).
//!
//! Mint adds each new plugin here, and that change is part of Mint's preview.

use std::collections::BTreeMap;

use berry_blocks_host::Block;

/// One on/off option a plugin offers to a wiki configuration.
pub struct OptionSpec {
    /// Key in the configuration file.
    pub key: &'static str,
    /// Label on the Configure form.
    pub label: &'static str,
    /// One-line explanation under the label.
    pub hint: &'static str,
    /// Value when the configuration does not set it.
    pub default: bool,
}

/// A plugin the renderer can run.
pub struct Entry {
    /// Plugin name, as in `plugins/<name>/`.
    pub name: &'static str,
    /// Options the plugin accepts.
    pub options: &'static [OptionSpec],
    /// Builds the plugin from option values (missing keys take their default).
    pub build: fn(&BTreeMap<String, bool>) -> Box<dyn Block>,
}

/// Every registered plugin, in a stable order.
pub fn entries() -> Vec<Entry> {
    vec![
        Entry {
            name: "progblocks",
            options: &[OptionSpec {
                key: "persist",
                label: "Remember each reader's choice of variant",
                hint: "Stored only in the reader's own browser, and only the variant name. Enhanced profile only.",
                default: false,
            }],
            build: |o| {
                Box::new(berry_blocks_progblocks::ProgBlocks {
                    persist_by_default: o.get("persist").copied().unwrap_or(false),
                    ..Default::default()
                })
            },
        },
        // berry-blocks:mint-entries (Mint inserts new plugins above this line)
    ]
}

/// The registered plugin with this name, if any.
pub fn find(name: &str) -> Option<Entry> {
    entries().into_iter().find(|e| e.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    /// ProgBlocks is registered and honours its option.
    fn progblocks_is_registered() {
        let e = find("progblocks").expect("registered");
        assert_eq!(e.options[0].key, "persist");
        let on = (e.build)(&BTreeMap::from([("persist".to_string(), true)]));
        let out = berry_blocks_host::render_page(
            "```sh variant=a group=g\n1\n```\n",
            &[on.as_ref()],
            berry_blocks_host::Profile::Enhanced,
        )
        .unwrap();
        assert!(out.html.contains(" persist"));
        assert!(find("nope").is_none());
    }
}
