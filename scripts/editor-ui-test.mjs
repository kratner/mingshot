// FILE: scripts/editor-ui-test.mjs
// Browser test of the editor (M2): headless Chromium, the editor on a mock of MingShot's Rust side (src/mock.ts),
// nothing on screen. Draws an arrow, a box and text; checks undo/redo, select + delete, and that Copy sends an
// annotation layer at the capture's exact pixel size with the shapes where they were drawn.
//
//   npm run test:editor        (builds the frontend first)
//
// Playwright comes from MingTab's install (PLAYWRIGHT=path/to/playwright/index.mjs to use another).
import { spawn } from 'node:child_process';
import path from 'node:path';
import assert from 'node:assert/strict';

const root = path.resolve(path.dirname(new URL(import.meta.url).pathname), '..');
const pw = process.env.PLAYWRIGHT ?? path.join(process.env.HOME, 'Projects/mingtab/node_modules/playwright/index.mjs');
const { chromium } = await import(pw);

const port = 4174;
const server = spawn('npx', ['vite', 'preview', '--port', String(port), '--strictPort'], { cwd: root, stdio: 'ignore' });
const stop = () => server.kill();
process.on('exit', stop);
await new Promise((r) => setTimeout(r, 1500));

const browser = await chromium.launch();
const page = await browser.newPage({ viewport: { width: 1200, height: 800 }, deviceScaleFactor: 1 });
const errors = [];
page.on('pageerror', (e) => errors.push(String(e)));
await page.goto(`http://localhost:${port}/?editor=1&mock=1`);
await page.waitForSelector('.editor-canvas canvas');

const status = () => page.locator('.editor-status span').first().innerText();
const box = await page.locator('.editor-canvas').boundingBox();
// The mock capture is 800×500 and fits at 1:1 in this window, so canvas points are image pixels.
assert.equal(Math.round(box.width), 800, 'canvas fitted at 1:1');
const at = (x, y) => [box.x + x, box.y + y];
async function drag(x1, y1, x2, y2) {
  await page.mouse.move(...at(x1, y1));
  await page.mouse.down();
  await page.mouse.move(...at((x1 + x2) / 2, (y1 + y2) / 2), { steps: 4 });
  await page.mouse.move(...at(x2, y2), { steps: 4 });
  await page.mouse.up();
  // Konva refreshes where shapes can be clicked on its next frame; a person never clicks faster than that.
  await page.waitForTimeout(80);
}
let failures = 0;
async function check(name, fn) {
  try {
    await fn();
    console.log(`PASS ${name}`);
  } catch (e) {
    failures++;
    console.log(`FAIL ${name}: ${e.message}`);
  }
}

await check('arrow, box and text are drawn', async () => {
  await page.keyboard.press('a');
  await drag(100, 100, 300, 200);
  await page.keyboard.press('r');
  await drag(400, 250, 600, 400);
  await page.keyboard.press('t');
  await page.mouse.click(...at(100, 350));
  await page.keyboard.type('Hello');
  await page.keyboard.press('Enter');
  assert.match(await status(), /3 annotations/);
});

await check('drawings reach the app as they change (nothing lost on close)', async () => {
  const last = await page.evaluate(() => window.__syncs[window.__syncs.length - 1]);
  assert.equal(last.annotations.length, 3);
});

await check('undo and redo', async () => {
  await page.keyboard.press('Meta+z');
  assert.match(await status(), /2 annotations/);
  await page.keyboard.press('Meta+Shift+z');
  assert.match(await status(), /3 annotations/);
});

await check('a click is not a shape', async () => {
  await page.keyboard.press('r');
  await page.mouse.click(...at(700, 450));
  assert.match(await status(), /3 annotations/);
});

await check('select the arrow and delete it', async () => {
  await page.keyboard.press('v');
  await page.mouse.click(...at(200, 150));
  await page.keyboard.press('Backspace');
  assert.match(await status(), /2 annotations/);
});

await check('Copy sends the annotation layer at full size, shapes in place', async () => {
  await page.keyboard.press('Meta+c');
  await page.waitForFunction(() => window.__exports && window.__exports.length > 0);
  const result = await page.evaluate(async () => {
    const args = window.__exports[0];
    const img = new Image();
    img.src = args.overlay;
    await img.decode();
    const c = document.createElement('canvas');
    c.width = img.width;
    c.height = img.height;
    const g = c.getContext('2d');
    g.drawImage(img, 0, 0);
    const alpha = (x, y) => g.getImageData(x, y, 1, 1).data[3];
    // The text: any opaque pixel in its first line.
    let text = 0;
    for (let x = 100; x < 160; x++) for (let y = 352; y < 375; y++) text = Math.max(text, alpha(x, y));
    return {
      action: args.action,
      kinds: args.annotations.map((a) => a.kind).sort(),
      size: [img.width, img.height],
      boxEdge: alpha(400, 320),
      boxInside: alpha(500, 320),
      deletedArrow: alpha(200, 150),
      text,
      empty: alpha(750, 50),
    };
  });
  assert.equal(result.action, 'copy');
  assert.deepEqual(result.size, [800, 500], 'overlay is the capture size');
  assert.deepEqual(result.kinds, ['rect', 'text']);
  assert.ok(result.boxEdge > 200, `box edge drawn (alpha ${result.boxEdge})`);
  assert.equal(result.boxInside, 0, 'inside the box stays clear');
  assert.equal(result.deletedArrow, 0, 'the deleted arrow is gone');
  assert.ok(result.text > 200, `text drawn (alpha ${result.text})`);
  assert.equal(result.empty, 0, 'untouched areas are transparent');
});

const lastSync = () => page.evaluate(() => window.__syncs[window.__syncs.length - 1].annotations);

await check('a box resizes with its corner handle', async () => {
  await page.keyboard.press('v');
  await page.mouse.click(...at(400, 320)); // the box's left edge
  await drag(600, 400, 650, 440); // its bottom-right handle
  const rect = (await lastSync()).find((a) => a.kind === 'rect');
  assert.ok(Math.abs(rect.w - 250) < 6 && Math.abs(rect.h - 190) < 6, `box now ${rect.w}×${rect.h}`);
});

await check("an arrow's end moves with its handle", async () => {
  await page.keyboard.press('a');
  await drag(100, 460, 300, 460);
  await page.keyboard.press('v');
  await page.mouse.click(...at(200, 460));
  await drag(300, 460, 350, 420);
  const arrow = (await lastSync()).find((a) => a.kind === 'arrow');
  assert.ok(Math.abs(arrow.x2 - 350) < 4 && Math.abs(arrow.y2 - 420) < 4 && Math.abs(arrow.x1 - 100) < 2, JSON.stringify(arrow));
});

await check('Save turns off once saved and back on after a change', async () => {
  const save = page.getByRole('button', { name: 'Save', exact: true });
  await save.click();
  await page.waitForFunction(() => window.__exports.some((e) => e.action === 'save'));
  await page.waitForFunction(() => document.querySelector('button.editor-save').disabled);
  assert.match(await status(), /Saved in Pictures › MingShot/);
  await page.keyboard.press('r');
  await drag(500, 50, 560, 90);
  assert.equal(await save.isDisabled(), false);
});

await check('the capture shows when binary replies come as arrays (Mojave)', async () => {
  const p2 = await browser.newPage({ viewport: { width: 1200, height: 800 }, deviceScaleFactor: 1 });
  await p2.goto(`http://localhost:${port}/?editor=1&mock=array`);
  await p2.waitForSelector('.editor-canvas img');
  const size = await p2.evaluate(async () => {
    const img = document.querySelector('.editor-canvas img');
    await img.decode();
    return [img.naturalWidth, img.naturalHeight];
  });
  assert.deepEqual(size, [800, 500]);
  await p2.close();
});

await check('Copy and Save stay on screen in a narrow editor', async () => {
  const p3 = await browser.newPage({ viewport: { width: 700, height: 500 }, deviceScaleFactor: 1 });
  await p3.goto(`http://localhost:${port}/?editor=1&mock=1`);
  await p3.waitForSelector('.editor-canvas canvas');
  for (const name of ['Copy', 'Save']) {
    const b = await p3.getByRole('button', { name, exact: true }).boundingBox();
    assert.ok(b && b.x >= 0 && b.x + b.width <= 700 && b.y + b.height <= 500, `${name} at ${JSON.stringify(b)}`);
  }
  await p3.close();
});

await check('no script errors', async () => {
  assert.deepEqual(errors, []);
});

await browser.close();
stop();
console.log(failures ? `\nEDITOR UI FAILED (${failures})` : '\nEDITOR UI PASSED');
process.exit(failures ? 1 : 0);
