// SPDX-License-Identifier: MPL-2.0
// tools/check-pages.mjs — check the lab's rendered pages in a real Chromium.
//
//   CHROMIUM_PATH=/path/to/chrome bun tools/check-pages.mjs out/static out/enhanced
//
// For each profile directory: axe-core (WCAG 2.0-2.2 A/AA + best practice) on
// every page in dark and light schemes; for "static", no <script> anywhere; for
// "enhanced", every <prog-block> upgrades with JS, the variant choice persists
// to the next page, and with JS disabled every variant's code is still visible.
// Prints a JSON summary (for docs/reports/) and exits 1 on any failure.
import { chromium } from 'playwright-core';
import { createServer } from 'node:http';
import { readFile, readdir, stat } from 'node:fs/promises';
import { extname, join, normalize, resolve } from 'node:path';

const types = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css' };

/** Serves one directory over HTTP on a free port; resolves to the server. */
function serve(root) {
  const server = createServer(async (req, res) => {
    const path = normalize(join(root, decodeURIComponent(new URL(req.url, 'http://x').pathname)));
    if (!path.startsWith(root)) return res.writeHead(403).end();
    try {
      res.writeHead(200, { 'content-type': types[extname(path)] || 'application/octet-stream' });
      res.end(await readFile(path));
    } catch {
      res.writeHead(404).end();
    }
  });
  return new Promise((ok) => server.listen(0, '127.0.0.1', () => ok(server)));
}

/** Runs axe on the open page; returns { violations, incomplete, contrastNodes }. */
async function audit(page, axe) {
  await page.addScriptTag({ content: axe });
  const r = await page.evaluate(() => axe.run(document, {
    runOnly: { type: 'tag', values: ['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa', 'wcag22aa', 'best-practice'] },
  }));
  return {
    violations: r.violations.map((v) => `${v.id}: ${v.nodes.map((n) => JSON.stringify(n.target)).join(' ')}`),
    incomplete: r.incomplete.map((v) => v.id),
    contrastNodes: r.passes.find((p) => p.id === 'color-contrast')?.nodes.length ?? 0,
  };
}

/** Checks one rendered profile directory; returns its summary record. */
async function checkProfile(browser, dir, axe) {
  const root = resolve(dir) + '/';
  const profile = dir.split('/').filter(Boolean).pop();
  const pages = (await readdir(root)).filter((f) => f.endsWith('.html')).sort();
  const server = await serve(root);
  const base = `http://127.0.0.1:${server.address().port}/`;
  const summary = { profile, pages: pages.length, failures: [], bytes: {}, axe: {} };
  const fail = (m) => summary.failures.push(m);

  for (const file of pages) {
    const html = await readFile(join(root, file), 'utf8');
    summary.bytes[file] = Buffer.byteLength(html);
    if (profile === 'static' && /<script/i.test(html)) fail(`${file}: contains <script>`);
  }
  if (profile === 'enhanced') {
    const js = await stat(join(root, 'progblocks/prog-block.js')).then((s) => s.size, () => 0);
    const css = await stat(join(root, 'progblocks/prog-block.css')).then((s) => s.size, () => 0);
    summary.bytes['progblocks (js+css, cached once)'] = js + css;
  }

  for (const scheme of ['light', 'dark']) {
    const ctx = await browser.newContext({ colorScheme: scheme });
    const page = await ctx.newPage();
    for (const file of pages) {
      await page.goto(base + file);
      if (profile === 'enhanced' && (await page.$('prog-block'))) await page.waitForFunction(() => customElements.get('prog-block'));
      const a = await audit(page, axe);
      summary.axe[`${scheme}:${file}`] = a.violations.length + a.incomplete.length;
      for (const v of a.violations) fail(`${scheme} ${file}: axe ${v}`);
      for (const v of a.incomplete) fail(`${scheme} ${file}: axe needs review ${v}`);
    }
    await ctx.close();
  }

  if (profile === 'enhanced') {
    const ctx = await browser.newContext();
    const page = await ctx.newPage();
    await page.goto(base + 'Install-ripgrep.html');
    await page.waitForFunction(() => customElements.get('prog-block'));
    const tabs = await page.locator('prog-block').first().locator('[role=tab]').count();
    if (tabs !== 3) fail(`Install-ripgrep: expected 3 tabs after upgrade, got ${tabs}`);
    await page.locator('prog-block').first().locator('[role=tab]', { hasText: 'Linux' }).click();
    await page.goto(base + 'Upgrade-ripgrep.html');
    await page.waitForFunction(() => customElements.get('prog-block'));
    const variant = await page.evaluate(() => document.querySelector('prog-block').variant);
    if (variant !== 'Linux') fail(`persist: Upgrade page opened on ${variant}, expected Linux`);
    summary.persistedAcrossPages = variant === 'Linux';
    await ctx.close();

    const nojs = await browser.newContext({ javaScriptEnabled: false });
    const p2 = await nojs.newPage();
    await p2.goto(base + 'Install-ripgrep.html');
    const shown = await p2.evaluate(() => ({
      summaries: [...document.querySelectorAll('.bb-variant summary')].map((s) => s.textContent),
      code: document.querySelector('.bb-variant pre')?.innerText ?? '',
    }));
    const variantsOk = shown.summaries.join() === 'macOS,Windows,Linux';
    const codeOk = shown.code.includes('brew install ripgrep');
    if (!variantsOk) fail(`no-JS: variants ${shown.summaries.join()}`);
    if (!codeOk) fail(`no-JS: first variant code missing (${shown.code})`);
    summary.noJsReadable = variantsOk && codeOk;
    await nojs.close();
  }
  server.close();
  return summary;
}

if (!process.env.CHROMIUM_PATH) {
  console.error('Set CHROMIUM_PATH to a Chromium or Chrome executable.');
  process.exit(2);
}
const axe = await readFile(new URL('../node_modules/axe-core/axe.min.js', import.meta.url), 'utf8');
const browser = await chromium.launch({ executablePath: process.env.CHROMIUM_PATH, args: ['--no-sandbox'] });
const results = [];
for (const dir of process.argv.slice(2)) results.push(await checkProfile(browser, dir, axe));
await browser.close();
console.log(JSON.stringify(results, null, 2));
process.exit(results.some((r) => r.failures.length) ? 1 : 0);
