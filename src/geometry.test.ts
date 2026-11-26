// Unit tests for geometry.ts. Run: npm run test:ui (Node strips the types itself).
import assert from 'node:assert/strict';
import { dragEnd, hex, lockRatio, nudgeFor, rectOf } from './geometry.ts';

const o = { x: 100, y: 100 };
const none = { x: 0, y: 0 };

// Snapping: at 2× a half point is a whole pixel; at 1× it rounds.
assert.deepEqual(rectOf(o, { x: 110.5, y: 120.25 }, 2), { x: 100, y: 100, w: 10.5, h: 20.5 });
assert.deepEqual(rectOf({ x: 110.4, y: 120 }, o, 1), { x: 100, y: 100, w: 10, h: 20 });

// Nudges are device pixels: one arrow press at 2× moves half a point.
assert.deepEqual(dragEnd(o, { x: 150, y: 150 }, { x: 1, y: -2 }, 2, null), { x: 150.5, y: 149 });
assert.deepEqual(dragEnd(o, { x: 150, y: 150 }, { x: 10, y: 0 }, 1, null), { x: 160, y: 150 });

// Shift: square from a point, width leads when the pointer went further across.
assert.equal(lockRatio(o, o), 1);
assert.deepEqual(dragEnd(o, { x: 160, y: 120 }, none, 1, 1), { x: 160, y: 160 });
// Height leads when the pointer went further down; direction kept (up and left).
assert.deepEqual(dragEnd(o, { x: 90, y: 40 }, none, 1, 1), { x: 40, y: 40 });
// Locked 2:1 stays 2:1.
assert.equal(lockRatio(o, { x: 300, y: 200 }), 2);
assert.deepEqual(dragEnd(o, { x: 300, y: 300 }, none, 1, 2), { x: 500, y: 300 });
assert.deepEqual(dragEnd(o, { x: 500, y: 120 }, none, 1, 2), { x: 500, y: 300 });

// Arrow keys.
assert.deepEqual(nudgeFor('ArrowLeft', false), { x: -1, y: 0 });
assert.deepEqual(nudgeFor('ArrowDown', true), { x: 0, y: 10 });
assert.equal(nudgeFor('a', false), null);

assert.equal(hex(201, 162, 74), '#C9A24A');
assert.equal(hex(0, 15, 255), '#000FFF');

console.log('geometry: all tests passed');
