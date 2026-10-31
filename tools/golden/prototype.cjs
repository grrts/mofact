// Renders the prototype's sprites, as drawn by the playable page's canvas
// code (web/index.html), into crates/artkit/golden/ as PNGs. The art kit's
// tests compare its tiny-skia drawings against these (T17).
//
//   node tools/golden/prototype.cjs [px]
//
// Needs Playwright with Chromium. The page's constants and drawing sections
// are run on their own, without the engine or the UI. Text (the hub's "HUB" label) is left out:
// the game draws labels with the UI font instead.

const { chromium } = require('playwright');
const { mkdirSync, readFileSync, writeFileSync } = require('node:fs');
const { join } = require('node:path');

const root = join(__dirname, '..', '..');
const out = join(root, 'crates', 'artkit', 'golden');
const px = Number(process.argv[2] || 64);

// name, tiles across, what to draw. `e` is the entity at tile (0, 0).
const SPRITES = [
  ['belt', 1, 'drawBelt({x:0,y:0,dir:0,items:[]}, 0, 0, 1)'],
  ['belt-mid', 1, 'drawBelt({x:0,y:0,dir:0,items:[]}, 1, 0, 1)'],
  ['drill', 1, 'ore[0] = 1; drawBuilding({type:"drill",x:0,y:0,dir:0,id:1}, 0, 0, 1)'],
  ['drill-copper', 1, 'ore[0] = 2; drawBuilding({type:"drill",x:0,y:0,dir:0,id:1}, 0, 0, 1)'],
  ['furnace', 1, 'drawBuilding({type:"furnace",x:0,y:0,dir:0,id:1}, 0, 0, 1)'],
  ['furnace-iron', 1, 'drawBuilding({type:"furnace",x:0,y:0,dir:0,id:1,inType:"iron-ore",work:true}, 0, 0, 1)'],
  ['assembler-gear', 1, 'drawBuilding({type:"assembler",x:0,y:0,dir:0,id:1,recipe:"gear"}, 0, 0, 1)'],
  ['assembler-circuit', 1, 'drawBuilding({type:"assembler",x:0,y:0,dir:0,id:1,recipe:"circuit"}, 0, 0, 1)'],
  ['hub', 3, 'ctx.fillText = () => {}; drawBuilding({type:"hub",x:0,y:0,dir:0,id:1}, 0, 0, 1)'],
  ...['iron-ore', 'iron-plate', 'gear', 'wire', 'circuit'].map(
    (item) => [`item-${item}`, 1, `drawItem("${item}", vw / 2, vh / 2, vw * 0.6)`],
  ),
];

(async () => {
// The page's constants and drawing code, lifted out of its closure with a
// still camera, so they can be called one sprite at a time.
const html = readFileSync(join(root, 'web', 'index.html'), 'utf8');
const section = (from, to) => html.slice(html.indexOf(`// ---------- ${from}`), html.indexOf(`// ---------- ${to}`));
const script = [
  section('constants', 'state'),
  'let ore, ctx, vw = 0, vh = 0, dpr = 1;',
  'const cam = { x: 0, y: 0, z: 1 };',
  'const w2sx = wx => (wx - cam.x) * cam.z + vw / 2;',
  'const w2sy = wy => (wy - cam.y) * cam.z + vh / 2;',
  'const s2w = (sx, sy) => [(sx - vw / 2) / cam.z + cam.x, (sy - vh / 2) / cam.z + cam.y];',
  section('drawing', 'tools & input'),
  `window.render = (px, tiles, draw) => {
    const c = document.createElement('canvas');
    c.width = c.height = px * tiles;
    ctx = c.getContext('2d');
    vw = vh = px * tiles;
    cam.z = px; cam.x = cam.y = tiles / 2;
    ore = new Uint8Array(W * H);
    Math.random = () => 1; // no smoke puffs
    eval(draw);
    return c.toDataURL('image/png');
  };`,
].join('\n');

const browser = await chromium.launch();
const page = await browser.newPage();
page.on('pageerror', (e) => { throw e; });
await page.setContent(`<!doctype html><body><script>${script}</script></body>`);
mkdirSync(out, { recursive: true });
for (const [name, tiles, draw] of SPRITES) {
  const url = await page.evaluate(([px, tiles, draw]) => window.render(px, tiles, draw), [px, tiles, draw]);
  writeFileSync(join(out, `${name}.png`), Buffer.from(url.split(',')[1], 'base64'));
  console.log(`${name}.png`);
}
await browser.close();
})();
