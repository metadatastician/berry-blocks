<!-- SPDX-License-Identifier: MPL-2.0 -->
# Changelog

## [Unreleased]

### Added

- Plugin wizard look and feel: a clickable, script-free prototype of mint, provision, configure and harness (14 screens, axe-clean in light and dark), generated from one frame (`design/wizard/`), and the screen pattern it defines (`design/wizard/PATTERN.adoc`).

- Plugin host (`berry-blocks-host`): finds fence runs a block claims, lets BerryWiki render the page, substitutes the block output behind one-off markers, and fails closed on any marker mismatch.
- ProgBlocks block (`berry-blocks-progblocks`): variant fences rendered as script-free `<details>` groups, optionally wrapped in `<prog-block>`.
- `berry-blocks render` CLI, lab wiki fixtures, pinned BerryWiki and ProgBlocks (`pins.kyaml`, `scripts/fetch-pins.sh`).
- Conformance test over BerryWiki's own fixture wiki (byte-identical), pin-consistency test, browser checks (`tools/check-pages.mjs`).
- ADR-0001 (independence and pins), ADR-0002 (plugin contract, draft), the static-vs-enhanced report, and a proposal to BerryWiki.
