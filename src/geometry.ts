// Selection geometry for the overlay (M1, PLAN.md §2): where the dragged corner is, given the pointer, arrow-key
// nudges and a Shift aspect-ratio lock, and the selection rectangle snapped to device pixels. Pure functions, unit
// tested in geometry.test.ts (npm run test:ui).

export type Pt = { x: number; y: number };
export type Sel = { x: number; y: number; w: number; h: number };

/** A coordinate in points, snapped to the nearest device pixel. */
export const snap = (v: number, dpr: number) => Math.round(v * dpr) / dpr;

/** The selection between two corners, snapped to device pixels (what's drawn is exactly what's cropped). */
export function rectOf(a: Pt, b: Pt, dpr: number): Sel {
  const x = snap(Math.min(a.x, b.x), dpr);
  const y = snap(Math.min(a.y, b.y), dpr);
  return { x, y, w: snap(Math.max(a.x, b.x), dpr) - x, h: snap(Math.max(a.y, b.y), dpr) - y };
}

/** The proportions to keep when Shift goes down mid-drag: the selection's own, or square while it's a point. */
export function lockRatio(from: Pt, end: Pt): number {
  const w = Math.abs(end.x - from.x);
  const h = Math.abs(end.y - from.y);
  return w >= 1 && h >= 1 ? w / h : 1;
}

/**
 * The dragged corner: the pointer, moved by the arrow-key nudges (in device pixels), and with Shift held, pulled
 * onto the locked proportions (`ratio` = width / height) along whichever side the pointer leads with, keeping the
 * direction it's dragged in.
 */
export function dragEnd(from: Pt, pointer: Pt, nudge: Pt, dpr: number, ratio: number | null): Pt {
  let x = pointer.x + nudge.x / dpr;
  let y = pointer.y + nudge.y / dpr;
  if (ratio && ratio > 0) {
    const dx = x - from.x;
    const dy = y - from.y;
    const sx = dx < 0 ? -1 : 1;
    const sy = dy < 0 ? -1 : 1;
    if (Math.abs(dx) / ratio >= Math.abs(dy)) {
      y = from.y + (sy * Math.abs(dx)) / ratio;
    } else {
      x = from.x + sx * Math.abs(dy) * ratio;
    }
  }
  return { x, y };
}

/**
 * Arrow keys mid-drag: one device pixel, ten with Option. Null for any other key. Recognises the arrow by `key`
 * ("ArrowLeft", or the older "Left") or, failing that, by `keyCode` (37–40), since WebKit versions differ (Mojave's
 * original Safari 12 among them).
 */
export function nudgeFor(key: string, alt: boolean, keyCode = 0): Pt | null {
  const step = alt ? 10 : 1;
  const name = key.replace(/^Arrow/, '') || '';
  const byCode: Record<number, string> = { 37: 'Left', 38: 'Up', 39: 'Right', 40: 'Down' };
  const dir = ['Left', 'Right', 'Up', 'Down'].indexOf(name) >= 0 ? name : byCode[keyCode];
  switch (dir) {
    case 'Left':
      return { x: -step, y: 0 };
    case 'Right':
      return { x: step, y: 0 };
    case 'Up':
      return { x: 0, y: -step };
    case 'Down':
      return { x: 0, y: step };
    default:
      return null;
  }
}

/** The colour of an RGBA pixel as #RRGGBB. */
export function hex(r: number, g: number, b: number): string {
  return '#' + [r, g, b].map((v) => (v < 16 ? '0' : '') + v.toString(16).toUpperCase()).join('');
}
