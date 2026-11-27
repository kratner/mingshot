// A stand-in for MingShot's Rust side, so the editor can be tested in a plain browser (scripts/editor-ui-test.mjs):
// index.html?editor=1&mock=1. Never loaded in the app (main.tsx only imports it outside Tauri).
import { mockIPC } from '@tauri-apps/api/mocks';

export const MOCK_W = 800;
export const MOCK_H = 500;

/** A test image: a grey gradient with a grid, so the canvas has something to show. */
async function testImage(): Promise<ArrayBuffer> {
  const c = document.createElement('canvas');
  c.width = MOCK_W;
  c.height = MOCK_H;
  const g = c.getContext('2d')!;
  const grad = g.createLinearGradient(0, 0, MOCK_W, MOCK_H);
  grad.addColorStop(0, '#dddddd');
  grad.addColorStop(1, '#888888');
  g.fillStyle = grad;
  g.fillRect(0, 0, MOCK_W, MOCK_H);
  g.strokeStyle = '#ffffff';
  for (let x = 0; x < MOCK_W; x += 50) g.strokeRect(x, 0, 50, MOCK_H);
  const blob: Blob = await new Promise((r) => c.toBlob((b) => r(b!), 'image/png'));
  return blob.arrayBuffer();
}

/** `array`: binary replies come back as plain arrays of numbers, as through older WebKits' IPC fallback (Mojave). */
export async function installMock(mode: string) {
  const png = await testImage();
  const image = mode === 'array' ? Array.from(new Uint8Array(png)) : png;
  const exports: unknown[] = [];
  (window as unknown as { __exports: unknown[] }).__exports = exports;
  mockIPC((cmd, args) => {
    switch (cmd) {
      case 'editor_doc':
        return { width: MOCK_W, height: MOCK_H, scale: 2, title: 'Mock capture', annotations: [], saved: null };
      case 'editor_image':
        return image;
      case 'editor_export':
        exports.push(args);
        return { copied: true, saved: null, editing: true };
      default:
        return null;
    }
  });
}
