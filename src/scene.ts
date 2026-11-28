// The editor's scene (M2, PLAN.md §2 "Editor"): annotations as plain data, in the capture's own pixel coordinates,
// plus undo/redo. No UI here, so the same model can be saved in the .mingshot sidecar, reopened, and later driven
// from the CLI or MCP server (PLAN.md §7.10). Unit tested in scene.test.ts (npm run test:ui).

export type Style = {
  /** CSS colour, e.g. '#E5484D'. */
  color: string;
  /** Stroke width in image pixels (text: font size). */
  width: number;
};

export type Arrow = { id: string; kind: 'arrow'; x1: number; y1: number; x2: number; y2: number; style: Style };
export type Box = { id: string; kind: 'rect'; x: number; y: number; w: number; h: number; style: Style };
export type Label = { id: string; kind: 'text'; x: number; y: number; text: string; style: Style };
export type Shape = Arrow | Box | Label;
export type Scene = Shape[];

let counter = 0;
/** A short id, unique within the scene. */
export function newId(scene: Scene): string {
  const taken = new Set(scene.map((s) => s.id));
  let id: string;
  do id = `s${(++counter).toString(36)}`;
  while (taken.has(id));
  return id;
}

export const add = (scene: Scene, shape: Shape): Scene => [...scene, shape];

export const remove = (scene: Scene, id: string): Scene => scene.filter((s) => s.id !== id);

export function update<T extends Shape>(scene: Scene, id: string, patch: Partial<T>): Scene {
  return scene.map((s) => (s.id === id ? ({ ...s, ...patch } as Shape) : s));
}

/** Moves a shape by (dx, dy) image pixels. */
export function move(scene: Scene, id: string, dx: number, dy: number): Scene {
  return scene.map((s) => {
    if (s.id !== id) return s;
    if (s.kind === 'arrow') return { ...s, x1: s.x1 + dx, y1: s.y1 + dy, x2: s.x2 + dx, y2: s.y2 + dy };
    return { ...s, x: s.x + dx, y: s.y + dy };
  });
}

/** A rectangle dragged in any direction, as top-left plus positive size. */
export function normalise(b: Box): Box {
  return {
    ...b,
    x: Math.min(b.x, b.x + b.w),
    y: Math.min(b.y, b.y + b.h),
    w: Math.abs(b.w),
    h: Math.abs(b.h),
  };
}

/** Too small to have been meant (a click, not a drag): not added. */
export function tooSmall(s: Shape, min: number): boolean {
  if (s.kind === 'arrow') return Math.hypot(s.x2 - s.x1, s.y2 - s.y1) < min;
  if (s.kind === 'rect') return Math.abs(s.w) < min || Math.abs(s.h) < min;
  return s.text.trim() === '';
}

// ---- Undo / redo ------------------------------------------------------------------------------------------------

export type History = { past: Scene[]; present: Scene; future: Scene[] };

export const start = (scene: Scene = []): History => ({ past: [], present: scene, future: [] });

/** A finished change: the old scene goes on the undo stack, redo is cleared. Unchanged scenes aren't recorded. */
export function commit(h: History, next: Scene): History {
  if (next === h.present) return h;
  return { past: [...h.past, h.present].slice(-200), present: next, future: [] };
}

export function undo(h: History): History {
  if (!h.past.length) return h;
  return { past: h.past.slice(0, -1), present: h.past[h.past.length - 1], future: [h.present, ...h.future] };
}

export function redo(h: History): History {
  if (!h.future.length) return h;
  return { past: [...h.past, h.present], present: h.future[0], future: h.future.slice(1) };
}

// ---- Styles ------------------------------------------------------------------------------------------------------

/** The palette: Four Ounces red first (PLAN.md §2 "Styles"), then high-contrast choices. */
export const COLOURS = ['#E5484D', '#C9A24A', '#2B7DE9', '#12A150', '#1A1916', '#FFFFFF'];
/** Stroke widths in points; multiplied by the capture's scale so they look the same on 1× and 2× captures. */
export const WIDTHS = { S: 2, M: 4, L: 8 } as const;
/** Text sizes in points for the same S / M / L buttons (Keith, 2026-11-28: S/M/L didn't change text). */
export const TEXT_SIZES = { S: 14, M: 18, L: 28 } as const;
