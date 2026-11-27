// Unit tests for scene.ts. Run: npm run test:ui.
import assert from 'node:assert/strict';
import { add, commit, move, newId, normalise, redo, remove, start, tooSmall, undo, update } from './scene.ts';
import type { Arrow, Box, Scene } from './scene.ts';

const style = { color: '#E5484D', width: 8 };
const arrow: Arrow = { id: 'a', kind: 'arrow', x1: 0, y1: 0, x2: 100, y2: 50, style };
const box: Box = { id: 'b', kind: 'rect', x: 50, y: 60, w: -40, h: -20, style };

// Adding, updating, moving, removing never mutate the scene they're given.
const s0: Scene = [];
const s1 = add(s0, arrow);
const s2 = add(s1, box);
assert.equal(s0.length, 0);
assert.equal(s2.length, 2);
assert.deepEqual(move(s2, 'a', 10, -5)[0], { ...arrow, x1: 10, y1: -5, x2: 110, y2: 45 });
assert.deepEqual(move(s2, 'b', 1, 1)[1], { ...box, x: 51, y: 61 });
assert.equal((update<Box>(s2, 'b', { w: 5 })[1] as Box).w, 5);
assert.deepEqual(remove(s2, 'a'), [box]);
assert.equal(s2[0], arrow);

// A rectangle dragged up and left becomes top-left plus positive size.
assert.deepEqual(normalise(box), { ...box, x: 10, y: 40, w: 40, h: 20 });

// Clicks aren't shapes.
assert.equal(tooSmall({ ...arrow, x2: 2, y2: 2 }, 6), true);
assert.equal(tooSmall(arrow, 6), false);
assert.equal(tooSmall({ id: 't', kind: 'text', x: 0, y: 0, text: '  ', style }, 6), true);

// Ids are unique within a scene.
const ids = new Set<string>();
let scene: Scene = [];
for (let i = 0; i < 50; i++) {
  const id = newId(scene);
  assert.ok(!ids.has(id));
  ids.add(id);
  scene = add(scene, { ...arrow, id });
}

// Undo and redo walk the history; a new change clears redo; no-op commits aren't recorded.
let h = start();
h = commit(h, s1);
h = commit(h, s2);
assert.equal(commit(h, h.present), h);
h = undo(h);
assert.equal(h.present, s1);
h = undo(h);
assert.deepEqual(h.present, []);
assert.equal(undo(h), h);
h = redo(h);
assert.equal(h.present, s1);
h = commit(h, remove(s1, 'a'));
assert.equal(h.future.length, 0);
assert.equal(redo(h), h);

console.log('scene: all tests passed');
