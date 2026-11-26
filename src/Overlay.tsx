// The selection overlay for one display. It sits, transparent, over a native window showing the frozen frame, so
// all it draws is the dimming, the highlight or selection, and a label. It goes up the instant New is clicked,
// before the display has been read: the window is left out of the capture (not shareable while the displays are
// read), and the e2e test checks that its dimming changes nothing in the frame, glass and blur materials included.
// The frozen frame slides in under it a moment later. Coordinates are CSS pixels, which are points on this display;
// they're snapped to the display's device pixels so what you see selected is exactly what's cropped.
//
// Modes (the capture window's picker, PLAN.md §2): Region drags a rectangle. Window highlights the window under the
// pointer and a click takes it (Rust sends the windows on this display, front to back, as "targets"). Full display
// highlights the whole display and a click takes it. In every mode a drag still makes a region.

import { useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';

// Events addressed to this overlay only (the global `listen` hears every window's events).
const listen = <T,>(event: string, handler: (e: { payload: T }) => void) =>
  getCurrentWebviewWindow().listen<T>(event, handler);

type Pt = { x: number; y: number };
type Sel = { x: number; y: number; w: number; h: number };
type Mode = 'region' | 'window' | 'display';
type Target = Sel & { id: number; app: string; title: string };
type Targets = { mode: Mode; windows: Target[] };
/// What a click would take now: a window, or the whole display.
type Hover = { rect: Sel; label: string; window?: number };

const snap = (v: number) => Math.round(v * devicePixelRatio) / devicePixelRatio;

function rectOf(a: Pt, b: Pt): Sel {
  const x = snap(Math.min(a.x, b.x));
  const y = snap(Math.min(a.y, b.y));
  return { x, y, w: snap(Math.max(a.x, b.x)) - x, h: snap(Math.max(a.y, b.y)) - y };
}

/// Movement under this (points) between press and release is a click, not a drag.
const CLICK_SLOP = 4;

function hoverAt(t: Targets, p: Pt | null): Hover | null {
  if (t.mode === 'display') return { rect: { x: 0, y: 0, w: innerWidth, h: innerHeight }, label: 'This display' };
  if (t.mode !== 'window' || !p) return null;
  const w = t.windows.find((w) => p.x >= w.x && p.x < w.x + w.w && p.y >= w.y && p.y < w.y + w.h);
  if (!w) return null;
  return { rect: w, label: w.title ? `${w.app} — ${w.title}` : w.app, window: w.id };
}

/// The part of `r` on this display.
function clip(r: Sel): Sel {
  const x = Math.max(0, r.x);
  const y = Math.max(0, r.y);
  return { x, y, w: Math.min(innerWidth, r.x + r.w) - x, h: Math.min(innerHeight, r.y + r.h) - y };
}

/// Dims everything except `r`, with four plain rectangles (a huge box-shadow did the same but cost WebKit hundreds
/// of milliseconds to paint over a 4K display).
function Shades({ r }: { r: Sel }) {
  return (
    <>
      <div className="shade" style={{ left: 0, top: 0, right: 0, height: r.y }} />
      <div className="shade" style={{ left: 0, top: r.y + r.h, right: 0, bottom: 0 }} />
      <div className="shade" style={{ left: 0, top: r.y, width: r.x, height: r.h }} />
      <div className="shade" style={{ left: r.x + r.w, top: r.y, right: 0, height: r.h }} />
    </>
  );
}

function Highlight({ r }: { r: Sel }) {
  return <div className="highlight" style={{ left: r.x, top: r.y, width: r.w, height: r.h }} />;
}

/// A label just under `r` (or above it near the bottom of the display, or inside it when it fills the display).
function labelTop(r: Sel) {
  if (r.y + r.h + 30 <= innerHeight) return r.y + r.h + 6;
  if (r.y >= 30) return r.y - 26;
  return r.y + 10;
}

export default function Overlay({ display }: { display: number }) {
  const [start, setStart] = useState<Pt | null>(null);
  const [end, setEnd] = useState<Pt | null>(null);
  const [pointer, setPointer] = useState<Pt | null>(null);
  const [targets, setTargets] = useState<Targets>({ mode: 'region', windows: [] });
  const targetsRef = useRef(targets);
  targetsRef.current = targets;
  const busy = useRef(false);
  // Bench mode paints the overlay opaque magenta so the bench can prove no capture ever contains it.
  const [bench, setBench] = useState(false);
  // The drag is followed on the window, not on an element: the element under the pointer changes as the selection
  // is drawn (the dimming gives way to the selection box), and a pointer capture set on it would go with it.
  const dragFrom = useRef<Pt | null>(null);

  const reset = () => {
    dragFrom.current = null;
    setStart(null);
    setEnd(null);
  };

  const run = async (command: string, args: Record<string, unknown>) => {
    busy.current = true;
    try {
      await invoke(command, args);
    } finally {
      busy.current = false;
    }
  };

  const finishDrag = async (to: Pt) => {
    const from = dragFrom.current;
    reset();
    if (!from || busy.current) return;
    const sel = rectOf(from, to);
    if (Math.abs(to.x - from.x) < CLICK_SLOP && Math.abs(to.y - from.y) < CLICK_SLOP) {
      // A click: takes the window or display under it, in those modes; in region mode, keep selecting.
      const hover = hoverAt(targetsRef.current, from);
      if (hover?.window != null) await run('finish_window_capture', { window: hover.window });
      else if (hover) await run('finish_display_capture', { display });
      return;
    }
    if (sel.w < 1 || sel.h < 1) return;
    await run('finish_capture', { display, rect: sel });
  };

  useEffect(() => {
    invoke<boolean>('bench_opaque').then(setBench);
    const offTargets = listen<Targets>('targets', (e) => setTargets(e.payload));
    // Overlays are hidden, not closed, between captures: start the next one clean.
    const offThawed = listen('thawed', () => {
      reset();
      setPointer(null);
      setTargets({ mode: 'region', windows: [] });
    });
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        reset();
        invoke('cancel_capture');
      }
    };
    const onBlur = () => reset();
    const move = (e: PointerEvent) => {
      const p = { x: e.clientX, y: e.clientY };
      setPointer(p);
      if (dragFrom.current) setEnd(p);
    };
    const up = (e: PointerEvent) => {
      if (e.button === 0) finishDrag({ x: e.clientX, y: e.clientY });
    };
    // End-to-end test mode (scripts/e2e.swift, MINGSHOT_E2E_SELECT): the overlay under the pointer is told to make
    // the given selection, through the same path a mouse release takes. Synthetic mouse drags don't reach WebKit
    // on this Mac (Safari neither: docs/M0.md), so the real drag is checked by hand.
    const offE2e = listen<Sel>('e2e-select', (e) => {
      const r = e.payload;
      dragFrom.current = { x: r.x, y: r.y };
      setTimeout(() => finishDrag({ x: r.x + r.w, y: r.y + r.h }), 350);
    });
    window.addEventListener('keydown', onKey);
    window.addEventListener('blur', onBlur);
    window.addEventListener('pointermove', move);
    window.addEventListener('pointerup', up);
    return () => {
      offTargets.then((f) => f());
      offThawed.then((f) => f());
      offE2e.then((f) => f());
      window.removeEventListener('keydown', onKey);
      window.removeEventListener('blur', onBlur);
      window.removeEventListener('pointermove', move);
      window.removeEventListener('pointerup', up);
    };
  }, []);

  const onDown = (e: React.PointerEvent) => {
    if (e.button !== 0) return;
    const p = { x: e.clientX, y: e.clientY };
    dragFrom.current = p;
    setStart(p);
    setEnd(p);
  };

  const dragging =
    start && end && (Math.abs(end.x - start.x) >= CLICK_SLOP || Math.abs(end.y - start.y) >= CLICK_SLOP);
  const sel = dragging ? rectOf(start, end) : null;
  const hover = sel ? null : hoverAt(targets, start ?? pointer);
  const dpr = devicePixelRatio;
  return (
    <div className={bench ? 'overlay bench-overlay' : 'overlay'} onPointerDown={onDown}>
      {sel ? (
        <>
          <Shades r={sel} />
          <div className="selection" style={{ left: sel.x, top: sel.y, width: sel.w, height: sel.h }} />
          <div className="size" style={{ left: sel.x, top: labelTop(sel) }}>
            {Math.round(sel.w * dpr)} × {Math.round(sel.h * dpr)}
          </div>
        </>
      ) : hover ? (
        <>
          <Shades r={clip(hover.rect)} />
          <Highlight r={clip(hover.rect)} />
          <div className="size" style={{ left: clip(hover.rect).x + 8, top: labelTop(clip(hover.rect)) }}>
            {hover.label} · {Math.round(hover.rect.w * dpr)} × {Math.round(hover.rect.h * dpr)}
          </div>
        </>
      ) : (
        <div className="dim" />
      )}
      {!sel && !hover && pointer && (
        <>
          <div className="cross-h" style={{ top: snap(pointer.y) }} />
          <div className="cross-v" style={{ left: snap(pointer.x) }} />
        </>
      )}
    </div>
  );
}
