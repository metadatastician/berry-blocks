<!-- SPDX-License-Identifier: MPL-2.0 -->
# berry-blocks

**An integration lab and the start of a plugin host for BerryWiki. Its first
plugin is ProgBlocks.**

berry-blocks depends on [BerryWiki](https://github.com/metadatastician/berrywiki)
and [ProgBlocks](https://github.com/metadatastician/progblocks). Neither of them
depends on berry-blocks, and neither depends on the other. If this repository
disappeared tomorrow, both would keep working unchanged. See
[ADR-0001](docs/decisions/ADR-0001-independence-and-pins.adoc).

![The same Markdown page, three ways](docs/assets/2026-10-05-static-enhanced-nojs.png)

*One Markdown page, rendered three ways: static profile (no script), enhanced
profile with JavaScript, and enhanced profile with JavaScript turned off.*

## What it does today

Authors mark variants on ordinary fenced code blocks. GitHub's own wiki still
shows these as plain code blocks:

````markdown
```bash variant=macOS group=os persist label="Operating system"
brew install {{ package = ripgrep }}
```

```bash variant=Linux group=os
sudo apt-get install {{ package = ripgrep }}
```
````

`berry-blocks render` turns a BerryWiki folder into HTML pages in one of two
profiles:

| Profile | What readers get | Script |
|---|---|---|
| `static` | Each variant is a native `<details>` section inside a labelled group, with defaults filled in. It is compatible with BerryWiki's no-script rule. | none |
| `enhanced` | The same static markup wrapped in `<prog-block>`. With JavaScript it becomes tabs with editable variables, Copy and Download, and the choice persists. Without JavaScript it looks the same as `static`. | ProgBlocks, about 21 kB, fetched once |

BerryWiki stays the renderer. The host swaps each variant run for a one-off
marker and lets `berrywiki_render::render_markdown` render the page. It then
replaces each marker with the block's HTML. A conformance test requires every
page of BerryWiki's own fixture wiki to come out **byte-identical** to
BerryWiki's render.

## Try it

```sh
./scripts/fetch-pins.sh          # BerryWiki + ProgBlocks at the commits in pins.kyaml
cargo test --workspace           # host, block, conformance and pin tests
cargo run -p berry-blocks -- render --profile static   fixtures/lab-wiki out/static
cargo run -p berry-blocks -- render --profile enhanced fixtures/lab-wiki out/enhanced
bun install && CHROMIUM_PATH=/path/to/chrome bun tools/check-pages.mjs out/static out/enhanced
```

Open `_pages.html` for the page list. Serve `out/enhanced` over HTTP to see the tabs; browsers do not run module
scripts from `file://`.

## The plugin wizard

```sh
cargo run -p berry-blocks -- wizard          # http://127.0.0.1:23880/
```

Server-rendered screens with no script, following
[the screen pattern](design/wizard/PATTERN.adoc). **Mint** is built: it previews
exactly the files it would create (manifest, crate, conformance test,
workspace entry), and Mint plugin writes those files and nothing else. A
digest ties the two together, so if the form or the repository changes after
the preview, minting is refused and nothing is written. A minted plugin builds
and passes the repo's gates as generated; `tools/e2e-mint.sh` proves this on
every CI run.

**Provision** is built too. It pins a plugin's upstream code to one exact
commit, after checking that the commit exists (a branch or tag name is refused),
that its licence is compatible with MPL-2.0, and that every file the plugin
needs is there. The preview shows the check results and the exact changes to
`pins.kyaml` and the plugin's manifest. A plugin whose code lives in
berry-blocks is shown as "not needed".

**Configure** is built. It turns registered plugins on for one wiki, sets
their options, and chooses the static or enhanced profile. It saves the result
as `wikis/<name>.kyaml`. The preview renders every page of the wiki to report
which pages would change. Render a configuration with
`berry-blocks render --config wikis/<name>.kyaml OUT`. Plugins are listed in
`crates/berry-blocks-registry`; Mint adds each new plugin there, and that edit
appears in Mint's preview. Harness is designed but not built yet.

## Documents

| Document | What it answers |
|---|---|
| [ADR-0001](docs/decisions/ADR-0001-independence-and-pins.adoc) | The independence rules and how pins work |
| [ADR-0002](docs/decisions/ADR-0002-plugin-contract.adoc) | The plugin contract (draft) and the road to the plugin wizard |
| [Static vs enhanced report](docs/reports/2026-10-05-static-vs-enhanced.adoc) | The measured comparison of the two models |
| [Wizard look and feel](design/wizard/PATTERN.adoc) | The one screen pattern for mint, provision, configure, harness; clickable prototype in `design/wizard/site/` |
| [Proposal to BerryWiki](docs/proposals/berrywiki-codefence-hook.adoc) | The one small interface that would remove the lab's workaround |

## Status

This is a lab. Nothing here is released. BerryWiki does not serve these pages:
its plan lists plugins as a v1 non-goal, and ADR-0003 and ADR-0007 forbid
hand-written script in anything it serves. berry-blocks is where the plugin
system is designed and tested before BerryWiki decides whether to adopt any of
it.

Licence: [MPL-2.0](LICENSE).
