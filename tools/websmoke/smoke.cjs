// A smoke test of the playable web link in a real browser: start a new
// game, pick the drill, tap the map (placing one or being refused, either
// of which buzzes the phone), and fail on any error the page throws.
//
//   node tools/websmoke/smoke.cjs SITE_DIR
//
// SITE_DIR holds web/ with mofact_web.js and mofact_web_bg.wasm beside
// index.html, as wasm-bindgen writes them. Needs Playwright with Chromium.
//
// It runs twice: as Chrome, and as a browser that lacks the optional parts
// of the web platform the game uses (no `navigator.vibrate`, as in Safari
// on iPhone, and no clipboard). Calling a missing one used to stop the
// game the moment anything was built.

const { chromium, devices } = require('playwright');
const http = require('node:http');
const { readFile } = require('node:fs/promises');
const { join, extname, normalize } = require('node:path');

const site = process.argv[2];
if (!site) {
  console.error('usage: node tools/websmoke/smoke.cjs SITE_DIR');
  process.exit(2);
}

const TYPES = {
  '.html': 'text/html',
  '.js': 'text/javascript',
  '.wasm': 'application/wasm',
  '.png': 'image/png',
  '.json': 'application/json',
};

function serve() {
  const server = http.createServer(async (req, res) => {
    const path = normalize(decodeURIComponent(new URL(req.url, 'http://x').pathname));
    try {
      const body = await readFile(join(site, path === '/' ? 'index.html' : path));
      res.writeHead(200, { 'content-type': TYPES[extname(path)] || 'application/octet-stream' });
      res.end(body);
    } catch {
      res.writeHead(404);
      res.end();
    }
  });
  return new Promise((ok) => server.listen(0, '127.0.0.1', () => ok(server)));
}

async function play(browser, port, { name, strip }) {
  const context = await browser.newContext({ ...devices['iPhone 13'] });
  const page = await context.newPage();
  const errors = [];
  page.on('pageerror', (e) => errors.push(e.message));
  page.on('console', (m) => {
    if (m.type() === 'error' && /panicked/.test(m.text())) errors.push(m.text());
  });
  if (strip) {
    await page.addInitScript(() => {
      delete Navigator.prototype.vibrate;
      Object.defineProperty(Navigator.prototype, 'clipboard', { get: () => undefined });
    });
  }
  await page.goto(`http://127.0.0.1:${port}/index.html?new`);
  await page.waitForFunction(() => !document.getElementById('status'), null, { timeout: 60_000 });
  await page.waitForTimeout(1000);
  const box = await page.locator('canvas').boundingBox();
  await page.keyboard.press('3');
  // Taps across the middle of the map: some land on ore, some don't.
  for (const [fx, fy] of [[0.5, 0.5], [0.3, 0.55], [0.7, 0.45], [0.5, 0.65]]) {
    await page.touchscreen.tap(box.x + box.width * fx, box.y + box.height * fy);
    await page.waitForTimeout(400);
  }
  // Long enough for a few more frames to run after the last tap.
  await page.waitForTimeout(1500);
  await context.close();
  if (errors.length) {
    console.error(`${name}: the page threw\n  ${errors.join('\n  ')}`);
    return false;
  }
  console.log(`${name}: built without an error`);
  return true;
}

(async () => {
  const server = await serve();
  const { port } = server.address();
  const browser = await chromium.launch({
    args: ['--use-gl=angle', '--use-angle=swiftshader', '--enable-unsafe-swiftshader'],
  });
  let ok = true;
  for (const run of [
    { name: 'chrome', strip: false },
    { name: 'no vibrate or clipboard (Safari)', strip: true },
  ]) {
    ok = (await play(browser, port, run)) && ok;
  }
  await browser.close();
  server.close();
  process.exit(ok ? 0 : 1);
})();
