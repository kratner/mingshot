// Builds MingShot's icons from branding/mingshot.svg (copied from kratner/four-ounces-brand,
// logo/products/mingshot.svg; change the glyph there first, then copy it here again).
//
//   node scripts/icons.mjs        (npm run icons)
//
// Writes:
//   branding/app-icon-1024.png   the tile on Apple's icon grid: 824 px tile centred on a 1024 canvas
//   src-tauri/icons/*            every app icon size, generated from that PNG by `tauri icon`
import { readFileSync, writeFileSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import path from 'node:path';
import { Resvg } from '@resvg/resvg-js';

const root = path.resolve(path.dirname(new URL(import.meta.url).pathname), '..');
const svg = readFileSync(path.join(root, 'branding/mingshot.svg'), 'utf8');
const tile = svg.replace(/^<svg[^>]*>/, '').replace(/<\/svg>\s*$/, '').replace(/<title>.*?<\/title>/, '');

const appIcon = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1024 1024">
  <g transform="translate(100 100) scale(${824 / 128})">${tile}</g></svg>`;
const appIconPath = path.join(root, 'branding/app-icon-1024.png');
writeFileSync(appIconPath, new Resvg(appIcon, { fitTo: { mode: 'width', value: 1024 } }).render().asPng());
console.log('wrote branding/app-icon-1024.png');

execFileSync('npx', ['tauri', 'icon', appIconPath], { cwd: root, stdio: 'inherit' });
