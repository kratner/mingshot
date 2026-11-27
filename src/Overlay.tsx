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
import { dragEnd, hex, lockRatio, nudgeFor, rectOf as rectOfAt, snap as snapAt } from './geometry';

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

const snap = (v: number) => snapAt(v, devicePixelRatio);
const rectOf = (a: Pt, b: Pt) => rectOfAt(a, b, devicePixelRatio);

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

/// Side of the magnifier's patch in device pixels (matches LOUPE in lib.rs), and how much it's enlarged.
const LOUPE = 15;
const ZOOM = 8;
const LOUPE_PX = LOUPE * ZOOM;

/// The magnifier: the frozen frame's pixels around `at`, enlarged, with a grid, the centre pixel outlined, and the
/// pointer's position (device pixels) and colour, or the selection's size while dragging. The pixels come from Rust,
/// a 15×15 patch per frame drawn.
function Loupe({ display, at, size }: { display: number; at: Pt; size: string | null }) {
  const canvas = useRef<HTMLCanvasElement>(null);
  const [colour, setColour] = useState<string | null>(null);
  const dpr = devicePixelRatio;
  const px = { x: Math.floor(at.x * dpr), y: Math.floor(at.y * dpr) };
  const wanted = useRef(px);
  wanted.current = px;
  const inFlight = useRef(false);

  useEffect(() => {
    // One request at a time; when it lands, ask again if the pointer has moved meanwhile.
    const fetchPatch = () => {
      if (inFlight.current) return;
      inFlight.current = true;
      const p = wanted.current;
      invoke<ArrayBuffer>('loupe', { display, x: p.x, y: p.y })
        .then((buf) => {
          const bytes = new Uint8ClampedArray(buf);
          const c = canvas.current;
          if (!c || bytes.length !== LOUPE * LOUPE * 4) return;
          const small = document.createElement('canvas');
          small.width = small.height = LOUPE;
          small.getContext('2d')!.putImageData(new ImageData(bytes, LOUPE, LOUPE), 0, 0);
          const g = c.getContext('2d')!;
          g.imageSmoothingEnabled = false;
          g.clearRect(0, 0, LOUPE_PX, LOUPE_PX);
          g.drawImage(small, 0, 0, LOUPE_PX, LOUPE_PX);
          g.strokeStyle = 'rgba(0,0,0,0.18)';
          g.lineWidth = 1;
          for (let i = 1; i < LOUPE; i++) {
            g.beginPath();
            g.moveTo(i * ZOOM + 0.5, 0);
            g.lineTo(i * ZOOM + 0.5, LOUPE_PX);
            g.moveTo(0, i * ZOOM + 0.5);
            g.lineTo(LOUPE_PX, i * ZOOM + 0.5);
            g.stroke();
          }
          const m = (LOUPE >> 1) * ZOOM;
          g.strokeStyle = '#C9A24A';
          g.lineWidth = 2;
          g.strokeRect(m, m, ZOOM, ZOOM);
          const k = ((LOUPE >> 1) * LOUPE + (LOUPE >> 1)) * 4;
          setColour(bytes[k + 3] ? hex(bytes[k], bytes[k + 1], bytes[k + 2]) : null);
        })
        .catch(() => {})
        .finally(() => {
          inFlight.current = false;
          const q = wanted.current;
          if (q.x !== p.x || q.y !== p.y) requestAnimationFrame(fetchPatch);
        });
    };
    requestAnimationFrame(fetchPatch);
  }, [display, px.x, px.y]);

  // Beside the pointer, down and to the right, flipped near the display's edges.
  const w = LOUPE_PX + 2;
  const left = at.x + 24 + w > innerWidth ? at.x - 24 - w : at.x + 24;
  const top = at.y + 24 + w + 40 > innerHeight ? at.y - 24 - w - 40 : at.y + 24;
  return (
    <div className="loupe" style={{ left, top }}>
      <canvas ref={canvas} width={LOUPE_PX} height={LOUPE_PX} style={{ width: LOUPE_PX, height: LOUPE_PX }} />
      <div className="loupe-read">
        {size ? (
          <>
            <div className="loupe-row">
              <span className="loupe-key">size</span>
              {size}
            </div>
            <div className="loupe-row">
              <span className="loupe-key">x</span>
              {px.x}
              <span className="loupe-key loupe-gap">y</span>
              {px.y}
            </div>
          </>
        ) : (
          <>
            <div className="loupe-row">
              <span className="loupe-key">x</span>
              {px.x}
              <span className="loupe-key loupe-gap">y</span>
              {px.y}
            </div>
            <div className="loupe-row">
              <span className="loupe-swatch" style={{ background: colour ?? 'transparent' }} />
              {colour ?? '—'}
            </div>
          </>
        )}
      </div>
    </div>
  );
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
  // is drawn (the dimming gives way to the selection box). Mouse events, not pointer events: Mojave's original
  // WebKit (Safari 12) has no pointer events.
  const dragFrom = useRef<Pt | null>(null);
  // Mid-drag adjustments (geometry.ts): arrow-key nudges in device pixels, and Shift's locked proportions.
  const nudge = useRef<Pt>({ x: 0, y: 0 });
  const ratio = useRef<number | null>(null);
  const mouse = useRef<Pt | null>(null);

  const reset = () => {
    dragFrom.current = null;
    nudge.current = { x: 0, y: 0 };
    ratio.current = null;
    setStart(null);
    setEnd(null);
  };

  /// Where the dragged corner is now (pointer + nudges, Shift lock), after `shift` changes the lock if it must.
  const corner = (shift: boolean): Pt | null => {
    const from = dragFrom.current;
    const at = mouse.current;
    if (!from || !at) return null;
    if (shift && ratio.current == null) {
      ratio.current = lockRatio(from, dragEnd(from, at, nudge.current, devicePixelRatio, null));
    } else if (!shift) {
      ratio.current = null;
    }
    return dragEnd(from, at, nudge.current, devicePixelRatio, ratio.current);
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
        return;
      }
      if (!dragFrom.current) return;
      const step = nudgeFor(e.key, e.altKey);
      if (step) {
        e.preventDefault();
        nudge.current = { x: nudge.current.x + step.x, y: nudge.current.y + step.y };
      }
      if (step || e.key === 'Shift') {
        const c = corner(e.shiftKey);
        if (c) setEnd(c);
      }
    };
    const onKeyUp = (e: KeyboardEvent) => {
      if (e.key === 'Shift' && dragFrom.current) {
        const c = corner(false);
        if (c) setEnd(c);
      }
    };
    const onBlur = () => reset();
    const move = (e: MouseEvent) => {
      const p = { x: e.clientX, y: e.clientY };
      mouse.current = p;
      setPointer(p);
      if (dragFrom.current) setEnd(corner(e.shiftKey) ?? p);
    };
    const up = (e: MouseEvent) => {
      if (e.button !== 0) return;
      mouse.current = { x: e.clientX, y: e.clientY };
      finishDrag(corner(e.shiftKey) ?? mouse.current);
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
    window.addEventListener('keyup', onKeyUp);
    window.addEventListener('blur', onBlur);
    window.addEventListener('mousemove', move);
    window.addEventListener('mouseup', up);
    return () => {
      offTargets.then((f) => f());
      offThawed.then((f) => f());
      offE2e.then((f) => f());
      window.removeEventListener('keydown', onKey);
      window.removeEventListener('keyup', onKeyUp);
      window.removeEventListener('blur', onBlur);
      window.removeEventListener('mousemove', move);
      window.removeEventListener('mouseup', up);
    };
  }, []);

  const onDown = (e: React.MouseEvent) => {
    if (e.button !== 0) return;
    const p = { x: e.clientX, y: e.clientY };
    mouse.current = p;
    nudge.current = { x: 0, y: 0 };
    ratio.current = null;
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
    <div className={bench ? 'overlay bench-overlay' : 'overlay'} onMouseDown={onDown}>
      {sel ? (
        <>
          <Shades r={sel} />
          <div className="selection" style={{ left: sel.x, top: sel.y, width: sel.w, height: sel.h }} />
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
      {(end ?? pointer) && !bench && (
        <Loupe
          display={display}
          at={sel && end ? end : (pointer as Pt)}
          size={sel ? `${Math.round(sel.w * dpr)} × ${Math.round(sel.h * dpr)}` : null}
        />
      )}
    </div>
  );
}
