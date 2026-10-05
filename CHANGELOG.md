<!-- SPDX-License-Identifier: MPL-2.0 -->
# Changelog

## [Unreleased]

### Added

- Wizard step 4, **Harness** (`berry-blocks-harness`). It runs eight checks per configuration, with pass, fail or not-run outcomes, and saves a canonical KYAML report in `reports/`. Failing checks come with advice. `tools/harness-browser.mjs` runs the browser checks for any wiki. `berry-blocks-site` is extracted from the CLI so render and Harness write identical pages; the output was verified byte-identical before and after the extraction. All four wizard steps are built. Run from the wizard against `lab-wiki`, all 8 checks pass.

- Wizard step 3, **Configure** (`berry-blocks-configure`). It writes `wikis/<name>.kyaml`: wiki folder, profile, plugins and their options. The file is read and written in canonical KYAML. The preview renders every page to show which pages change. Apply is digest-gated, so editing the wiki after the preview makes the preview stale. `berry-blocks render --config`. `berry-blocks-registry` lists runnable plugins and their options; ProgBlocks gains a wiki-level `persist` option. Mint now registers each new plugin as two more previewed changes. `wikis/lab-wiki.kyaml` is committed, and CI renders the enhanced lab through it.

- Wizard step 2, **Provision** (`berry-blocks-provision`): fetches an upstream commit by SHA into `vendor/.cache/` and checks it exists, its licence is MPL-2.0-compatible (otherwise refused), and the needed files are present. The preview shows the checks and the diffs to `pins.kyaml` and the manifest. Apply is digest-gated and checks the commit out into `vendor/<plugin>/`. `pins.kyaml` is read and written by a strict reader that round-trips the file exactly. `scripts/fetch-pins.sh` now fetches every pin. Verified against GitHub: ProgBlocks was provisioned at `ee10c66` in a scratch copy, and the lab passed all tests and browser checks on that pin.

- The plugin wizard, `berry-blocks wizard`: Mint is built. `berry-blocks-mint` plans (preview) and applies a mint, with a SHA-256 digest tying the two together. It refuses stale previews and never overwrites a file. Plugin IDs are UUID v8 profile C. `berry-blocks-wizard` serves the approved screens with no script, a strict CSP, loopback-only binding and a cross-site post refusal. `tools/e2e-mint.sh` mints through the running wizard in CI and runs fmt, clippy and the conformance test on the generated crate.

- Plugin wizard look and feel: a clickable, script-free prototype of mint, provision, configure and harness (14 screens, axe-clean in light and dark), generated from one frame (`design/wizard/`), and the screen pattern it defines (`design/wizard/PATTERN.adoc`).

- Plugin host (`berry-blocks-host`): finds fence runs a block claims, lets BerryWiki render the page, substitutes the block output behind one-off markers, and fails closed on any marker mismatch.
- ProgBlocks block (`berry-blocks-progblocks`): variant fences rendered as script-free `<details>` groups, optionally wrapped in `<prog-block>`.
- `berry-blocks render` CLI, lab wiki fixtures, pinned BerryWiki and ProgBlocks (`pins.kyaml`, `scripts/fetch-pins.sh`).
- Conformance test over BerryWiki's own fixture wiki (byte-identical), pin-consistency test, browser checks (`tools/check-pages.mjs`).
- ADR-0001 (independence and pins), ADR-0002 (plugin contract, draft), the static-vs-enhanced report, and a proposal to BerryWiki.
