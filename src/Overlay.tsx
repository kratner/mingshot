// The selection overlay for one display. It sits, transparent, over a native window showing the frozen frame, so
// all it draws is the dimming, the selection and its size. It goes up the instant New is clicked, before the display
// has been read: the window is left out of the capture (not shareable while the displays are read), and the e2e
// test checks that its dimming changes nothing in the frame, glass and blur materials included. The frozen frame
// slides in under it a moment later. Coordinates are CSS pixels, which are points on this
// display; they're snapped to the display's device pixels so what you see selected is exactly what's cropped.

import { useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';

// Events addressed to this overlay only (the global `listen` hears every window's events).
const listen = <T,>(event: string, handler: (e: { payload: T }) => void) =>
  getCurrentWebviewWindow().listen<T>(event, handler);

type Pt = { x: number; y: number };
type Sel = { x: number; y: number; w: number; h: number };

const snap = (v: number) => Math.round(v * devicePixelRatio) / devicePixelRatio;

function rectOf(a: Pt, b: Pt): Sel {
  const x = snap(Math.min(a.x, b.x));
  const y = snap(Math.min(a.y, b.y));
  return { x, y, w: snap(Math.max(a.x, b.x)) - x, h: snap(Math.max(a.y, b.y)) - y };
}

export default function Overlay({ display }: { display: number }) {
  const [start, setStart] = useState<Pt | null>(null);
  const [end, setEnd] = useState<Pt | null>(null);
  const [pointer, setPointer] = useState<Pt | null>(null);
  const busy = useRef(false);
  // Bench mode paints the overlay opaque magenta so the bench can prove no capture ever contains it.
  const [bench, setBench] = useState(false);

  const reset = () => {
    setStart(null);
    setEnd(null);
  };

  useEffect(() => {
    invoke<boolean>('bench_opaque').then(setBench);
    // Overlays are hidden, not closed, between captures: start the next one clean.
    const offThawed = listen('thawed', () => {
      dragFrom.current = null;
      reset();
    });
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        reset();
        invoke('cancel_capture');
      }
    };
    window.addEventListener('keydown', onKey);
    // The window is hidden, not closed, between captures: clear any half-drawn selection when it goes.
    const onBlur = () => {
      dragFrom.current = null;
      reset();
    };
    window.addEventListener('blur', onBlur);
    return () => {
      offThawed.then((f) => f());
      window.removeEventListener('keydown', onKey);
      window.removeEventListener('blur', onBlur);
    };
  }, []);

  // The drag is followed on the window, not on an element: the element under the pointer changes as the selection
  // is drawn (the dimming gives way to the selection box), and a pointer capture set on it would go with it.
  const dragFrom = useRef<Pt | null>(null);
  const finishDrag = async (to: Pt) => {
    const from = dragFrom.current;
    dragFrom.current = null;
    if (!from || busy.current) return;
    const sel = rectOf(from, to);
    reset();
    if (sel.w < 2 || sel.h < 2) return; // a click, not a drag: keep selecting
    busy.current = true;
    try {
      await invoke('finish_capture', { display, rect: sel });
    } finally {
      busy.current = false;
    }
  };

  useEffect(() => {
    const move = (e: PointerEvent) => {
      const p = { x: e.clientX, y: e.clientY };
      setPointer(p);
      if (dragFrom.current) setEnd(p);
    };
    const up = (e: PointerEvent) => {
      if (e.button === 0) finishDrag({ x: e.clientX, y: e.clientY });
    };
    window.addEventListener('pointermove', move);
    window.addEventListener('pointerup', up);
    // End-to-end test mode (scripts/e2e.swift, MINGSHOT_E2E_SELECT): the overlay under the pointer is told to make
    // the given selection, through the same path a mouse release takes. Synthetic mouse drags don't reach WebKit
    // on this Mac (Safari neither: docs/M0.md), so the real drag is checked by hand.
    const offE2e = listen<{ x: number; y: number; w: number; h: number }>('e2e-select', (e) => {
      const r = e.payload;
      dragFrom.current = { x: r.x, y: r.y };
      setTimeout(() => finishDrag({ x: r.x + r.w, y: r.y + r.h }), 350);
    });
    return () => {
      offE2e.then((f) => f());
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

  const sel = start && end ? rectOf(start, end) : null;
  const dpr = devicePixelRatio;
  return (
    <div
      className={bench ? 'overlay bench-overlay' : 'overlay'}
      onPointerDown={onDown}
    >
      {sel ? (
        <>
          {/* Four plain rectangles dim everything outside the selection (a huge box-shadow did the same but cost
              WebKit hundreds of milliseconds to paint over a 4K display). */}
          <div className="shade" style={{ left: 0, top: 0, right: 0, height: sel.y }} />
          <div className="shade" style={{ left: 0, top: sel.y + sel.h, right: 0, bottom: 0 }} />
          <div className="shade" style={{ left: 0, top: sel.y, width: sel.x, height: sel.h }} />
          <div className="shade" style={{ left: sel.x + sel.w, top: sel.y, right: 0, height: sel.h }} />
          <div className="selection" style={{ left: sel.x, top: sel.y, width: sel.w, height: sel.h }} />
          <div
            className="size"
            style={{ left: sel.x, top: sel.y + sel.h + 6 > innerHeight - 24 ? sel.y - 26 : sel.y + sel.h + 6 }}
          >
            {Math.round(sel.w * dpr)} × {Math.round(sel.h * dpr)}
          </div>
        </>
      ) : (
        <div className="dim" />
      )}
      {!sel && pointer && (
        <>
          <div className="cross-h" style={{ top: snap(pointer.y) }} />
          <div className="cross-v" style={{ left: snap(pointer.x) }} />
        </>
      )}
    </div>
  );
}
