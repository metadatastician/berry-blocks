// SPDX-License-Identifier: MPL-2.0
// design/wizard/build.mjs — generates the clickable look-and-feel prototype of
// the berry-blocks plugin wizard (mint, provision, configure, harness) into
// design/wizard/site/. Every screen comes from ONE frame function, so the step
// rail, status strip, preview-then-act buttons and context panel are identical
// everywhere. The output is plain HTML and CSS; no screen contains script.
//
//   bun design/wizard/build.mjs
import { mkdirSync, writeFileSync, copyFileSync } from 'node:fs';

const OUT = new URL('./site/', import.meta.url).pathname;

/** Escapes text for HTML content and double-quoted attributes. */
const esc = (s) => String(s).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');

// Real data from the lab (berry-blocks 5c250c8, ProgBlocks ccb4122).
const P = {
  name: 'progblocks',
  display: 'ProgBlocks',
  id: 'c39f8b97-e19b-88cf-bc7f-15c773f72113',
  crate: 'berry-blocks-progblocks',
  repo: 'https://github.com/metadatastician/progblocks',
  commit: 'ccb412289d736abc6aea5609ed50bb53d90e9388',
  licence: 'MPL-2.0',
};
const STEPS = [
  { verb: 'Mint', what: 'Create the plugin and its manifest', file: 'mint' },
  { verb: 'Provision', what: 'Fetch it at an exact commit', file: 'provision' },
  { verb: 'Configure', what: 'Turn it on for a wiki', file: 'configure' },
  { verb: 'Harness', what: 'Check it does no harm', file: 'harness' },
];

/** Renders the step rail; `current` is a step index, `done` how many are complete. */
function rail(current, done) {
  const items = STEPS.map((s, i) => {
    const cls = i < done ? ' class="done"' : '';
    const cur = i === current ? ' aria-current="step"' : '';
    const state = i < done ? 'done' : i === current ? 'current step' : 'not started';
    return `<li${cls}><a href="${s.file}.html"${cur}><span class="n" aria-hidden="true">${i + 1}</span><span><span class="verb">${s.verb}</span><span class="what">${s.what}</span><span class="visually-hidden">, ${state}</span></span></a></li>`;
  }).join('\n');
  return `<nav class="rail" aria-label="Plugin steps">
<h2 id="steps-h">Steps</h2>
<ol class="steps" aria-labelledby="steps-h">
${items}
</ol>
<h2>Plugins</h2>
<ul class="plugins">
<li class="current"><a href="index.html">${P.display}</a></li>
<li><a href="mint.html">+ Mint a new plugin</a></li>
</ul>
</nav>`;
}

/** Renders the context panel: identity card, independence checklist, last checks. */
function context({ minted = true, pinned = true, checks = 'pass' } = {}) {
  const lastRun = {
    pass: '<p><span class="outcome pass">All 9 checks pass</span><br><span class="evidence">Last run 2026-10-05 against fixtures/lab-wiki.</span></p>',
    fail: '<p><span class="outcome fail">1 of 9 checks fails</span><br><span class="evidence">Last run 2026-10-05. Configured wikis keep the previous version until it passes.</span></p>',
    none: '<p class="evidence">Not run yet.</p>',
  }[checks];
  return `<aside class="context" aria-label="About this plugin">
<h2>This plugin</h2>
<div class="card"><dl>
<dt>Name</dt><dd>${P.display}</dd>
<dt>ID</dt><dd class="mono">${minted ? P.id : 'assigned at mint'}</dd>
<dt>ID kind</dt><dd>UUID v8, profile C</dd>
<dt>Crate</dt><dd class="mono">${P.crate}</dd>
<dt>Upstream</dt><dd><a href="${P.repo}">metadatastician/<wbr>progblocks</a></dd>
<dt>Pinned at</dt><dd class="mono">${pinned ? P.commit.slice(0, 7) : 'not yet'}</dd>
<dt>Profiles</dt><dd>static, enhanced</dd>
</dl></div>
<h2>Independence</h2>
<ul class="checklist">
<li class="yes">BerryWiki is not modified</li>
<li class="yes">ProgBlocks is not modified</li>
<li class="yes">Neither depends on this plugin</li>
<li class="yes">Static profile has no script</li>
</ul>
<h2>Last checks</h2>
${lastRun}
</aside>`;
}

/** The one frame every screen uses. */
function frame({ title, step = null, done = 0, ctx = {}, body }) {
  const kicker = step === null ? 'Plugins' : `Step ${step + 1} of 4 · ${STEPS[step].verb}`;
  return `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>${esc(title)} · berry-blocks</title>
<link rel="stylesheet" href="wizard.css">
</head>
<body>
<a class="skip-link" href="#main">Skip to content</a>
<header>
<div class="topbar">
<a class="brand" href="index.html">berry-blocks</a>
<span class="section">Plugins</span>
<a class="spacer" href="harness.html">Checks</a>
</div>
<p class="status-strip"><strong>Lab mode</strong> · writes only to this berry-blocks checkout · BerryWiki and ProgBlocks are never modified · nothing happens until you press an action button</p>
</header>
<div class="grid">
${rail(step, done)}
<main class="main" id="main">
<p class="kicker">${esc(kicker)}</p>
<h1>${esc(title)}</h1>
${body}
</main>
${context(ctx)}
</div>
</body>
</html>
`;
}

/** Preview-then-act buttons; on a preview screen the action becomes primary. */
function actions({ preview, act, actLabel, cancel = 'index.html', previewing = false }) {
  const prev = previewing
    ? `<a class="btn secondary" href="${preview}">Preview again</a>`
    : `<button class="btn secondary" type="submit" formaction="${preview}">Preview</button>`;
  const go = previewing
    ? `<a class="btn" href="${act}">${actLabel}</a>`
    : `<button class="btn" type="submit" formaction="${act}" disabled title="Preview first">${actLabel}</button>`;
  return `<div class="actions">${prev}${go}<a class="cancel" href="${cancel}">Cancel</a></div>
${previewing ? '' : '<p class="field-hint">The action stays unavailable until you have seen the preview.</p>'}`;
}

/** A list of file changes, then each file's full content. */
function changes(items) {
  const list = items.map((c) => `<li><span class="change-kind ${c.kind}">${c.kind === 'none' ? 'unchanged' : c.kind}</span><span class="path">${esc(c.path)}${c.note ? ` <span class="evidence">· ${esc(c.note)}</span>` : ''}</span></li>`).join('\n');
  const files = items.filter((c) => c.content).map((c, i) => `<details class="file"${i === 0 ? ' open' : ''}><summary>${esc(c.path)}</summary><pre>${c.content}</pre></details>`).join('\n');
  return `<ul class="changes">\n${list}\n</ul>\n${files}`;
}

/** Text field with label and hint. */
const field = (id, label, value, hint, mono = false, error = '') => `<div class="field"><label for="${id}">${label}</label><input type="text" id="${id}" name="${id}" value="${esc(value)}"${mono ? ' class="mono"' : ''}${error ? ' aria-invalid="true"' : ''} aria-describedby="${error ? `${id}-error ` : ''}${id}-hint">${error ? `<p class="field-error" id="${id}-error">${error}</p>` : ''}<p class="field-hint" id="${id}-hint">${hint}</p></div>`;

const DEED = esc(`;; SPDX-License-Identifier: MPL-2.0
(praxis-deed
  :schema-version "1.0.0"
  :canonical-name "progblocks-block"
  :beholding-chora #u5"estate/chora"
  (plugin
    :id "${P.id}"
    :id-profile C
    :id-domain "berry-blocks-plugin"
    :crate "${P.crate}"
    :claims "fenced code blocks whose info string has variant="
    :run-key "group"
    :profiles (static enhanced)
    :static-assets ()
    :enhanced-assets ("progblocks/prog-block.js")))`);

const pages = {};

pages['index.html'] = frame({
  title: 'Plugins',
  body: `<p class="lede">A plugin changes how certain blocks in a BerryWiki page are shown. Every plugin goes through the same four steps, always in this order, and every step shows you exactly what it will change before it changes anything.</p>
<table class="plugins-table">
<caption class="visually-hidden">Plugins and how far each has got</caption>
<thead><tr><th scope="col">Plugin</th><th scope="col">Minted</th><th scope="col">Provisioned</th><th scope="col">Configured</th><th scope="col">Harnessed</th></tr></thead>
<tbody><tr><th scope="row"><a href="harness-results.html">${P.display}</a></th><td class="state yes">✓ yes</td><td class="state yes">✓ ${P.commit.slice(0, 7)}</td><td class="state yes">✓ lab-wiki</td><td class="state yes">✓ 9 of 9 pass</td></tr></tbody>
</table>
<div class="actions"><a class="btn" href="mint.html">Mint a new plugin</a></div>
<h2>Screens in this prototype</h2>
<ul>
<li>Mint: <a href="mint.html">form</a> → <a href="mint-preview.html">preview</a> → <a href="mint-done.html">done</a></li>
<li>Provision: <a href="provision.html">form</a> → <a href="provision-preview.html">preview</a> → <a href="provision-done.html">done</a>; and <a href="provision-error.html">a refused commit</a></li>
<li>Configure: <a href="configure.html">form</a> → <a href="configure-preview.html">preview</a> → <a href="configure-done.html">done</a></li>
<li>Harness: <a href="harness.html">form</a> → <a href="harness-results.html">all pass</a>; and <a href="harness-failing.html">one failing</a></li>
</ul>`,
});

const mintForm = (previewing) => `<p class="lede">Minting creates a new, empty plugin in this repository: a Rust crate for its code and a manifest that says what it claims. Nothing is fetched and no wiki changes.</p>
<form method="get" action="mint-preview.html">
${field('name', 'Short name', P.name, 'Lowercase letters, digits and hyphens. Used for the crate and folder names.', true)}
${field('display', 'Display name', P.display, 'What readers and authors see.')}
${field('claims', 'What it claims in a page', 'variant', 'A fenced code block belongs to this plugin when its info string has this key, as in <code>```bash variant=macOS</code>.', true)}
${field('runkey', 'What groups blocks together', 'group', 'Consecutive claimed blocks with the same value of this key are rendered as one.', true)}
<fieldset class="field"><legend>Profiles</legend>
<div class="choice"><input type="checkbox" id="p-static" checked disabled><label for="p-static">Static <span class="fixed">always required</span></label><p class="field-hint">Plain HTML with no script, so BerryWiki can serve it. Every plugin must work here first.</p></div>
<div class="choice"><input type="checkbox" id="p-enhanced" name="enhanced" checked><label for="p-enhanced">Enhanced</label><p class="field-hint">Adds script on top of the static result, for hosts that allow it. Readers without script still get the static result.</p></div>
</fieldset>
<div class="field"><label for="licence">Licence</label><select id="licence" name="licence"><option selected>MPL-2.0</option><option>Apache-2.0</option><option>MIT</option></select><p class="field-hint">Must be compatible with MPL-2.0.</p></div>
${actions({ preview: 'mint-preview.html', act: 'mint-done.html', actLabel: 'Mint plugin', previewing })}
</form>`;

pages['mint.html'] = frame({ title: 'Mint a plugin', step: 0, done: 0, ctx: { minted: false, pinned: false, checks: 'none' }, body: mintForm(false) });
pages['mint-preview.html'] = frame({
  title: 'Mint a plugin', step: 0, done: 0, ctx: { minted: false, pinned: false, checks: 'none' },
  body: mintForm(true) + `<section class="preview" aria-labelledby="pv"><h2 id="pv">Preview: what minting will create</h2>
<p>Four new files. Nothing existing changes. The plugin's ID is derived from its name, so minting the same name again gives the same ID.</p>
${changes([
  { kind: 'create', path: `plugins/${P.name}/${P.name}.plugin_praxis.deed`, note: 'manifest', content: DEED },
  { kind: 'create', path: `crates/${P.crate}/Cargo.toml`, content: esc(`[package]\nname = "${P.crate}"\nversion.workspace = true\nlicense = "${P.licence}"\n\n[dependencies]\nberry-blocks-host = { path = "../berry-blocks-host" }`) },
  { kind: 'create', path: `crates/${P.crate}/src/lib.rs`, note: 'a Block that claims nothing until you write it' },
  { kind: 'create', path: `crates/${P.crate}/tests/conformance.rs`, note: 'BerryWiki pages must stay byte-identical' },
  { kind: 'modify', path: 'Cargo.toml', note: 'adds the crate to the workspace', content: esc('  members = [\n    "crates/berry-blocks-host",\n') + `<ins>${esc(`    "crates/${P.crate}",`)}</ins>\n` + esc('    "crates/berry-blocks-cli",\n  ]') },
])}</section>`,
});
pages['mint-done.html'] = frame({
  title: 'Minted', step: 0, done: 1, ctx: { pinned: false, checks: 'none' },
  body: `<div class="notice" role="status"><h2>${P.display} is minted</h2><p>Created 4 files and changed 1. Its ID is <span class="mono">${P.id}</span>.</p></div>
<p>The plugin exists but has no upstream code yet. Next, provision it: choose the exact commit of ProgBlocks it should use.</p>
<div class="actions"><a class="btn" href="provision.html">Continue to Provision</a><a class="cancel" href="index.html">Back to plugins</a></div>`,
});

const provForm = (previewing, commit = P.commit, commitError = '') => `<p class="lede">Provisioning fetches the plugin's upstream code at one exact commit. Pinning to a commit, not a branch, means the plugin never changes until you choose to move the pin.</p>
<form method="get" action="provision-preview.html">
${field('repo', 'Upstream repository', P.repo, 'Where the plugin\'s own code lives. It is read, never written.', true)}
${field('commit', 'Commit', commit, 'The full 40-character commit ID. A branch or tag name is refused because it can move.', true, commitError)}
${actions({ preview: 'provision-preview.html', act: 'provision-done.html', actLabel: 'Provision', previewing })}
</form>`;

pages['provision.html'] = frame({ title: `Provision ${P.display}`, step: 1, done: 1, ctx: { pinned: false, checks: 'none' }, body: provForm(false) });
pages['provision-preview.html'] = frame({
  title: `Provision ${P.display}`, step: 1, done: 1, ctx: { pinned: false, checks: 'none' },
  body: provForm(true) + `<section class="preview" aria-labelledby="pv"><h2 id="pv">Preview: what provisioning will do</h2>
<table class="checks"><caption class="visually-hidden">Checks run before anything is fetched</caption>
<thead><tr><th scope="col">Before fetching</th><th scope="col">Result</th></tr></thead><tbody>
<tr><td>The commit exists in the repository</td><td><span class="outcome pass">✓ Pass</span> <span class="evidence">${P.commit.slice(0, 7)}, "docs(a11y): screen-reader test script (#51)"</span></td></tr>
<tr><td>Its licence is compatible</td><td><span class="outcome pass">✓ Pass</span> <span class="evidence">MPL-2.0</span></td></tr>
<tr><td>The files the plugin needs are there</td><td><span class="outcome pass">✓ Pass</span> <span class="evidence">src/prog-block.js, src/prog-block.css · 21,391 bytes</span></td></tr>
</tbody></table>
${changes([
  { kind: 'modify', path: 'pins.kyaml', content: esc('  progblocks: {\n    repo: "https://github.com/metadatastician/progblocks",\n') + `<del>${esc('    commit: "(none)",')}</del>\n<ins>${esc(`    commit: "${P.commit}",`)}</ins>\n` + esc('  },') },
  { kind: 'create', path: 'vendor/progblocks/', note: 'a checkout of that commit; not committed to git' },
])}</section>`,
});
pages['provision-error.html'] = frame({
  title: `Provision ${P.display}`, step: 1, done: 1, ctx: { pinned: false, checks: 'none' },
  body: `<div class="error-banner" role="alert" aria-labelledby="err-h"><h2 id="err-h">Nothing was fetched or changed</h2><p>Fix these and preview again:</p><ul>
<li><a href="#commit">Commit</a>: <span class="mono">main</span> is a branch name, not a commit. Branches move, so the plugin could change without anyone choosing it.</li>
</ul></div>` + provForm(false, 'main', 'Error: main is a branch name, not a commit.'),
});
pages['provision-done.html'] = frame({
  title: 'Provisioned', step: 1, done: 2, ctx: { checks: 'none' },
  body: `<div class="notice" role="status"><h2>${P.display} is provisioned at ${P.commit.slice(0, 7)}</h2><p>Changed 1 file. The upstream code is in <span class="mono">vendor/progblocks/</span>.</p></div>
<p>Next, configure which wikis use it and how.</p>
<div class="actions"><a class="btn" href="configure.html">Continue to Configure</a><a class="cancel" href="index.html">Back to plugins</a></div>`,
});

const confForm = (previewing) => `<p class="lede">Configuring turns the plugin on for one wiki and chooses how its pages are delivered. Pages that do not use the plugin are not affected.</p>
<form method="get" action="configure-preview.html">
${field('wiki', 'Wiki folder', 'fixtures/lab-wiki', 'A BerryWiki folder or clone. It is read, never written.', true)}
<fieldset class="field"><legend>How pages are delivered</legend>
<div class="choice"><input type="radio" id="prof-static" name="profile" value="static"><label for="prof-static">Static</label><p class="field-hint">Every variant shown as an expandable section. No script. BerryWiki could serve this.</p></div>
<div class="choice"><input type="radio" id="prof-enh" name="profile" value="enhanced" checked><label for="prof-enh">Enhanced</label><p class="field-hint">The static result, upgraded to tabs with editable values when script runs. BerryWiki cannot serve this today; a documentation site can.</p></div>
</fieldset>
<fieldset class="field"><legend>Options</legend>
<div class="choice"><input type="checkbox" id="persist" name="persist" checked><label for="persist">Remember each reader's choice of variant</label><p class="field-hint">Stored only in the reader's own browser, and only the variant name. Enhanced profile only.</p></div>
</fieldset>
${actions({ preview: 'configure-preview.html', act: 'configure-done.html', actLabel: 'Save configuration', previewing })}
</form>`;

pages['configure.html'] = frame({ title: `Configure ${P.display}`, step: 2, done: 2, ctx: { checks: 'none' }, body: confForm(false) });
pages['configure-preview.html'] = frame({
  title: `Configure ${P.display}`, step: 2, done: 2, ctx: { checks: 'none' },
  body: confForm(true) + `<section class="preview" aria-labelledby="pv"><h2 id="pv">Preview: what saving will change</h2>
<p>Of the wiki's 4 pages, 2 use this plugin and will render differently. The other 2 stay exactly as BerryWiki renders them.</p>
${changes([
  { kind: 'create', path: 'wikis/lab-wiki.kyaml', content: esc(`# SPDX-License-Identifier: MPL-2.0\n{\n  wiki: "fixtures/lab-wiki",\n  profile: "enhanced",\n  plugins: [\n    {\n      name: "${P.name}",\n      options: {\n        persist: true,\n      },\n    },\n  ],\n}`) },
  { kind: 'none', path: 'fixtures/lab-wiki/', note: 'the wiki itself is never written' },
])}
<h3>Pages affected</h3>
<ul><li>Install ripgrep: 1 variant group (3 variants)</li><li>Upgrade ripgrep: 1 variant group (3 variants)</li></ul></section>`,
});
pages['configure-done.html'] = frame({
  title: 'Configured', step: 2, done: 3, ctx: { checks: 'none' },
  body: `<div class="notice" role="status"><h2>${P.display} is on for lab-wiki</h2><p>Created 1 file. Profile: enhanced, with readers' choices remembered.</p></div>
<p>Next, harness it: run every check against this wiki before anyone relies on it.</p>
<div class="actions"><a class="btn" href="harness.html">Continue to Harness</a><a class="cancel" href="index.html">Back to plugins</a></div>`,
});

const CHECKS = [
  ['BerryWiki pages stay identical', 'Every page without a claimed block renders byte-for-byte as BerryWiki renders it.', 'conformance'],
  ['Untrusted text stays text', 'Variant names, labels and code are escaped; nothing in a page becomes markup.', 'escaping'],
  ['Static pages have no script', 'Not one <code>&lt;script</code> in any static page.', 'static'],
  ['Accessible in light mode', 'axe-core, WCAG 2.0–2.2 A and AA plus best practice, in a real browser.', 'axe-light'],
  ['Accessible in dark mode', 'The same audit with the dark colour scheme.', 'axe-dark'],
  ['Readable without script', 'With JavaScript off, every variant and its code is visible.', 'nojs'],
  ['Upgrades with script', 'With JavaScript on, each group becomes tabs.', 'upgrade'],
  ['Remembers the reader\'s choice', 'A choice on one page is shown on the next.', 'persist'],
  ['Pins agree', 'The commit in pins.kyaml matches what was built.', 'pins'],
];
const EVIDENCE = {
  conformance: '11 BerryWiki fixture pages, both profiles',
  escaping: '<code>&lt;img src=x&gt;</code> as a variant name stayed text',
  static: '5 pages checked',
  'axe-light': '0 violations, 0 to review, 5 pages',
  'axe-dark': '0 violations, 0 to review, 5 pages',
  nojs: 'macOS, Windows, Linux shown with their code',
  upgrade: '3 tabs on Install ripgrep',
  persist: 'Chose Linux on Install, Upgrade opened on Linux',
  pins: `${P.commit.slice(0, 7)} in both`,
};

pages['harness.html'] = frame({
  title: `Harness ${P.display}`, step: 3, done: 3, ctx: { checks: 'none' },
  body: `<p class="lede">Harnessing runs the plugin against a configured wiki and checks it does no harm. Running checks changes nothing; it writes a report.</p>
<form method="get" action="harness-results.html">
<div class="field"><label for="wiki">Wiki</label><select id="wiki" name="wiki"><option selected>lab-wiki (enhanced)</option></select></div>
<fieldset class="field"><legend>Checks to run</legend>
${CHECKS.map(([t, h, k]) => `<div class="choice"><input type="checkbox" id="c-${k}" name="${k}" checked><label for="c-${k}">${t}</label><p class="field-hint">${h}</p></div>`).join('\n')}
</fieldset>
<div class="actions"><button class="btn" type="submit">Run checks</button><a class="cancel" href="index.html">Cancel</a></div>
</form>`,
});

/** The results table, optionally with the persistence check failing. */
function results(failPersist) {
  const rows = CHECKS.map(([t, , k]) => {
    const fail = failPersist && k === 'persist';
    const outcome = fail ? '<span class="outcome fail">✗ Fail</span>' : '<span class="outcome pass">✓ Pass</span>';
    const ev = fail ? 'Chose Linux on Install, Upgrade opened on macOS' : EVIDENCE[k];
    return `<tr><th scope="row">${t}</th><td>${outcome}</td><td class="evidence">${ev}</td></tr>`;
  }).join('\n');
  return `<table class="checks"><caption class="visually-hidden">Check results</caption><thead><tr><th scope="col">Check</th><th scope="col">Result</th><th scope="col">Evidence</th></tr></thead><tbody>
${rows}
</tbody></table>`;
}

pages['harness-results.html'] = frame({
  title: `Harness ${P.display}`, step: 3, done: 4,
  body: `<div class="notice" role="status"><h2>All 9 checks pass</h2><p>${P.display} at ${P.commit.slice(0, 7)} is safe to use on lab-wiki with the enhanced profile.</p></div>
${results(false)}
<p>Report saved as <span class="mono">reports/progblocks-lab-wiki-2026-10-05.kyaml</span>.</p>
<div class="actions"><a class="btn secondary" href="harness.html">Run again</a><a class="cancel" href="index.html">Back to plugins</a></div>`,
});
pages['harness-failing.html'] = frame({
  title: `Harness ${P.display}`, step: 3, done: 3, ctx: { checks: 'fail' },
  body: `<div class="error-banner" role="alert" aria-labelledby="err-h"><h2 id="err-h">1 of 9 checks fails</h2><p><a href="#persist-row">Remembers the reader's choice</a>: the choice made on one page was not shown on the next. lab-wiki keeps using the last version that passed until this is fixed.</p></div>
${results(true).replace('<tr><th scope="row">Remembers', '<tr id="persist-row"><th scope="row">Remembers')}
<h2>What this usually means</h2>
<p>The page is not asking for the choice to be remembered. Check that the first block in the group has <code>persist</code> in its info string, and that the configuration has "Remember each reader's choice" turned on.</p>
<div class="actions"><a class="btn" href="harness.html">Run again</a><a class="cancel" href="configure.html">Back to Configure</a></div>`,
});

mkdirSync(OUT, { recursive: true });
copyFileSync(new URL('./wizard.css', import.meta.url).pathname, OUT + 'wizard.css');
for (const [file, html] of Object.entries(pages)) writeFileSync(OUT + file, html);
console.log(`wrote ${Object.keys(pages).length} screens to ${OUT}`);
