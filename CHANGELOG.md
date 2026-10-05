<!-- SPDX-License-Identifier: MPL-2.0 -->
# Changelog

## [Unreleased]

### Added

- The plugin wizard, `berry-blocks wizard`: Mint is built. `berry-blocks-mint` plans (preview) and applies a mint, with a SHA-256 digest tying the two together. It refuses stale previews and never overwrites a file. Plugin IDs are UUID v8 profile C. `berry-blocks-wizard` serves the approved screens with no script, a strict CSP, loopback-only binding and a cross-site post refusal. `tools/e2e-mint.sh` mints through the running wizard in CI and runs fmt, clippy and the conformance test on the generated crate.

- Plugin wizard look and feel: a clickable, script-free prototype of mint, provision, configure and harness (14 screens, axe-clean in light and dark), generated from one frame (`design/wizard/`), and the screen pattern it defines (`design/wizard/PATTERN.adoc`).

- Plugin host (`berry-blocks-host`): finds fence runs a block claims, lets BerryWiki render the page, substitutes the block output behind one-off markers, and fails closed on any marker mismatch.
- ProgBlocks block (`berry-blocks-progblocks`): variant fences rendered as script-free `<details>` groups, optionally wrapped in `<prog-block>`.
- `berry-blocks render` CLI, lab wiki fixtures, pinned BerryWiki and ProgBlocks (`pins.kyaml`, `scripts/fetch-pins.sh`).
- Conformance test over BerryWiki's own fixture wiki (byte-identical), pin-consistency test, browser checks (`tools/check-pages.mjs`).
- ADR-0001 (independence and pins), ADR-0002 (plugin contract, draft), the static-vs-enhanced report, and a proposal to BerryWiki.
