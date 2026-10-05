<!-- SPDX-License-Identifier: MPL-2.0 -->
# AGENTS.md — berry-blocks

Canonical instructions for every coding agent. `CLAUDE.md` points here.

## What this repo is

An integration lab and plugin host for BerryWiki. ProgBlocks is plugin #1.
Read `README.md`, then `docs/decisions/ADR-0001-independence-and-pins.adoc`,
then `docs/decisions/ADR-0002-plugin-contract.adoc`.

## Hard rules

1. **Independence.** Never change BerryWiki or ProgBlocks from here, and never
   make either depend on this repo. Coupling only through documented interfaces:
   `berrywiki_render::render_markdown` and ProgBlocks' `<prog-block>` element.
   Anything that needs more from either project is written up as a proposal in
   `docs/proposals/` and taken to that project, not patched around silently.
2. **Pins move one at a time.** `pins.kyaml` and the `berrywiki-render` rev in
   `Cargo.toml` must agree (`tests/pins.rs`). Bump in a PR after the tests pass.
3. **The static profile never contains script.** The host refuses a block that
   requests a script under `Profile::Static`, and checks the output.
4. **Conformance.** BerryWiki's fixture pages must render byte-identically
   through the host. Do not weaken that test to make a change pass.
5. **Escape everything** a block writes: variant names, labels, code.
6. Estate formats: KYAML for YAML, JCS for JSON, DEED for machine-readable
   records, UUID v8 profile C for identifiers (owner decision 2026-10-05).

## Gates

```sh
./scripts/fetch-pins.sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
CHROMIUM_PATH=… bun tools/check-pages.mjs out/static out/enhanced   # after rendering both
```
