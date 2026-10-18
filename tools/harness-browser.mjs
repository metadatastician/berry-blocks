// SPDX-License-Identifier: MPL-2.0
// tools/harness-browser.mjs — the Harness step's browser checks, for any wiki.
//
//   CHROMIUM_PATH=/path/to/chrome bun tools/harness-browser.mjs STATIC_DIR ENHANCED_DIR
//
// Prints one line per check, `key<TAB>pass|fail<TAB>evidence`, for:
//   axe-light, axe-dark  axe-core (WCAG 2.0-2.2 A/AA + best practice) on every
//                        page of both profiles, in that colour scheme
//   no-js                with JavaScript off, every variant group shows each
//                        variant's name and non-empty code
//   upgrade              with JavaScript on, every <prog-block> has upgraded
// Unlike tools/check-pages.mjs it assumes nothing about which pages exist.
import { chromium } from 'playwright-core';
import { createServer } from 'node:http';
import { readFile, readdir } from 'node:fs/promises';
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

/** Lists the HTML pages of a rendered site. */
const pagesOf = async (dir) => (await readdir(dir)).filter((f) => f.endsWith('.html')).sort();

/** Prints one check line, with tabs and newlines removed from the evidence. */
const report = (key, ok, evidence) => console.log(`${key}\t${ok ? 'pass' : 'fail'}\t${String(evidence).replace(/[\t\n]+/g, ' ')}`);

const [staticDir, enhancedDir] = process.argv.slice(2).map((d) => resolve(d) + '/');
const axe = await readFile(new URL('../node_modules/axe-core/axe.min.js', import.meta.url), 'utf8');
const browser = await chromium.launch({ executablePath: process.env.CHROMIUM_PATH, args: ['--no-sandbox'] });
const sites = [];
for (const dir of [staticDir, enhancedDir]) {
  const server = await serve(dir);
  sites.push({ dir, server, base: `http://127.0.0.1:${server.address().port}/`, pages: await pagesOf(dir) });
}

for (const scheme of ['light', 'dark']) {
  const problems = [];
  let audited = 0;
  const ctx = await browser.newContext({ colorScheme: scheme, bypassCSP: true });
  const page = await ctx.newPage();
  for (const site of sites) {
    for (const file of site.pages) {
      await page.goto(site.base + file);
      if (await page.$('prog-block')) await page.waitForFunction(() => customElements.get('prog-block'), null, { timeout: 5000 }).catch(() => {});
      await page.addScriptTag({ content: axe });
      const r = await page.evaluate(() => axe.run(document, { runOnly: { type: 'tag', values: ['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa', 'wcag22aa', 'best-practice'] } }));
      for (const v of r.violations) problems.push(`${file}: ${v.id}`);
      for (const v of r.incomplete) problems.push(`${file}: needs review ${v.id}`);
      audited++;
    }
  }
  await ctx.close();
  report(`axe-${scheme}`, problems.length === 0, problems.length ? problems.slice(0, 5).join('; ') : `0 violations, 0 to review, ${audited} pages`);
}

{
  const ctx = await browser.newContext({ javaScriptEnabled: false });
  const page = await ctx.newPage();
  const problems = [];
  let groups = 0;
  for (const file of sites[1].pages) {
    await page.goto(sites[1].base + file);
    const found = await page.evaluate(() => [...document.querySelectorAll('.bb-variants')].map((g) =>
      [...g.querySelectorAll('.bb-variant')].map((v) => ({ name: v.querySelector('summary')?.textContent?.trim() ?? '', code: v.querySelector('pre')?.textContent?.trim() ?? '' }))));
    for (const g of found) {
      groups++;
      if (g.length === 0 || g.some((v) => !v.name || !v.code)) problems.push(`${file}: a variant group is empty or has a variant with no name or code`);
    }
  }
  await ctx.close();
  report('no-js', problems.length === 0, problems.length ? problems.slice(0, 5).join('; ') : `${groups} variant group(s) readable without script`);
}

{
  const ctx = await browser.newContext();
  const page = await ctx.newPage();
  let blocks = 0;
  const problems = [];
  for (const file of sites[1].pages) {
    await page.goto(sites[1].base + file);
    const count = await page.evaluate(() => document.querySelectorAll('prog-block').length);
    if (!count) continue;
    const ok = await page.waitForFunction(() => [...document.querySelectorAll('prog-block')].every((b) => b.shadowRoot), null, { timeout: 5000 }).then(() => true, () => false);
    blocks += count;
    if (!ok) problems.push(`${file}: a <prog-block> did not upgrade`);
  }
  await ctx.close();
  report('upgrade', problems.length === 0, problems.length ? problems.join('; ') : blocks ? `${blocks} <prog-block> element(s) upgraded` : 'no <prog-block> on these pages');
}

await browser.close();
for (const s of sites) s.server.close();
