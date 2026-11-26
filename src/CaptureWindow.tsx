// The capture window (PLAN.md §2, §7.6): New, a mode picker and a delay picker, Snipping Tool style. New hides this
// window, freezes every display and lets you drag a region; the region goes on the clipboard as a PNG.

import { useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

type Timings = {
  ui_ms: number;
  frozen_ms: number;
  all_ms: number;
  capture_ms: number;
  displays: number;
  delay_s: number;
};
type Captured = { width: number; height: number; bytes: number; copy_ms: number; timings: Timings };

type Mode = 'region' | 'window' | 'display';

/// Per-viewer conveniences (mode, shadow, delay); the app works the same without them.
function stored(key: string): string | null {
  try {
    return localStorage.getItem(`mingshot.${key}`);
  } catch {
    return null;
  }
}
function store(key: string, value: string) {
  try {
    localStorage.setItem(`mingshot.${key}`, value);
  } catch {
    /* private storage unavailable: settings just aren't remembered */
  }
}

type Status =
  | { kind: 'idle' }
  | { kind: 'copied'; captured: Captured; clickMs: number | null }
  | { kind: 'cancelled' }
  | { kind: 'error'; message: string };

export default function CaptureWindow() {
  const [permitted, setPermitted] = useState<boolean | null>(null);
  const [delay, setDelay] = useState(() => Number(stored('delay') ?? 0));
  const [mode, setMode] = useState<Mode>(() => (stored('mode') as Mode) ?? 'region');
  const [shadow, setShadow] = useState(() => stored('shadow') !== 'false');
  const [status, setStatus] = useState<Status>({ kind: 'idle' });
  const [bench, setBench] = useState(false);
  // Click-to-overlay time as the page sees it: from the click to start_capture returning, which happens once the
  // overlay has been handed to the window server.
  const clickMs = useRef<number | null>(null);

  // The mode applies to ⌃⇧2 too, so Rust keeps it; this window remembers it between launches.
  useEffect(() => {
    store('mode', mode);
    store('shadow', String(shadow));
    store('delay', String(delay));
    invoke('set_mode', { mode, shadow });
  }, [mode, shadow, delay]);

  useEffect(() => {
    invoke<boolean>('permission').then(setPermitted);
    const offCaptured = listen<Captured>('captured', (e) =>
      setStatus({ kind: 'copied', captured: e.payload, clickMs: clickMs.current }),
    );
    const offCancelled = listen('cancelled', () => setStatus({ kind: 'cancelled' }));
    // Re-check the permission whenever the window comes back (after a trip to System Settings).
    const onFocus = () => invoke<boolean>('permission').then(setPermitted);
    window.addEventListener('focus', onFocus);
    invoke<number>('bench_runs').then((n) => {
      if (n > 0) {
        setBench(true);
        runBench(n);
      }
    });
    return () => {
      offCaptured.then((f) => f());
      offCancelled.then((f) => f());
      window.removeEventListener('focus', onFocus);
    };
  }, []);

  async function onNew() {
    clickMs.current = null;
    const t = performance.now();
    try {
      await invoke('start_capture', { delay });
      clickMs.current = delay > 0 ? null : Math.round((performance.now() - t) * 10) / 10;
    } catch (e) {
      setStatus({ kind: 'error', message: String(e) });
    }
  }

  if (bench) {
    return <div className="bench">MingShot bench: this window must never appear in a capture.</div>;
  }

  return (
    <div className="capture-window">
      <div className="toolbar">
        <button className="new" onClick={onNew} disabled={permitted === false} title="New capture (⌃⇧2)">
          <span className="plus">+</span> New
        </button>
        <label className="picker">
          <span>Mode</span>
          <select value={mode} onChange={(e) => setMode(e.target.value as Mode)}>
            <option value="region">Region</option>
            <option value="window">Window</option>
            <option value="display">Full display</option>
          </select>
        </label>
        {mode === 'window' && (
          <label className="check" title="Include the window's drop shadow (transparent around it)">
            <input type="checkbox" checked={shadow} onChange={(e) => setShadow(e.target.checked)} />
            Shadow
          </label>
        )}
        <label className="picker">
          <span>Delay</span>
          <select value={delay} onChange={(e) => setDelay(Number(e.target.value))}>
            <option value={0}>None</option>
            <option value={3}>3 s</option>
            <option value={5}>5 s</option>
            <option value={10}>10 s</option>
          </select>
        </label>
      </div>
      <div className="status">
        {permitted === false ? (
          <span className="warn">
            MingShot needs Screen Recording to see the screen.{' '}
            <button className="link" onClick={() => invoke<boolean>('request_permission').then(setPermitted)}>
              Allow…
            </button>
          </span>
        ) : (
          <StatusLine status={status} />
        )}
      </div>
    </div>
  );
}

function StatusLine({ status }: { status: Status }) {
  switch (status.kind) {
    case 'idle':
      return <span className="muted">New or ⌃⇧2, then drag a region (or click a window or display in those modes). Esc cancels.</span>;
    case 'cancelled':
      return <span className="muted">Cancelled.</span>;
    case 'error':
      return <span className="warn">{status.message}</span>;
    case 'copied': {
      const { captured: c, clickMs } = status;
      const t = c.timings;
      const title = `overlay up ${t.ui_ms} ms · your display frozen ${t.frozen_ms} ms · all ${t.displays} displays ${t.all_ms} ms · copy ${c.copy_ms} ms`;
      return (
        <span title={title}>
          Copied {c.width} × {c.height} px.{' '}
          <span className="muted">
            Overlay {t.ui_ms} ms, frozen {t.frozen_ms} ms{clickMs != null ? ` (page saw ${clickMs} ms)` : ''}
          </span>
        </span>
      );
    }
  }
}

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

/** Bench mode (MINGSHOT_BENCH=<runs>, docs/M0.md): the same path as clicking New, run `runs` times. */
async function runBench(runs: number) {
  await sleep(1500);
  const control = await invoke<number>('bench_control');
  const rows: { click: number; t: Timings; window: number; overlay: number }[] = [];
  for (let i = 0; i < runs; i++) {
    await sleep(600);
    const t0 = performance.now();
    const t = await invoke<Timings>('start_capture', { delay: 0 });
    const click = performance.now() - t0;
    const check = await invoke<{ window: number; overlay: number }>('bench_check');
    rows.push({ click, t, ...check });
    await sleep(250);
    await invoke('cancel_capture');
  }
  const stat = (xs: number[]) => {
    const s = [...xs].sort((a, b) => a - b);
    const q = (p: number) => s[Math.min(s.length - 1, Math.floor(p * s.length))];
    return `min ${s[0].toFixed(1)} · median ${q(0.5).toFixed(1)} · p90 ${q(0.9).toFixed(1)} · max ${s[s.length - 1].toFixed(1)} ms`;
  };
  const leaks = rows.filter((r) => r.window > 0).length;
  const overlayMax = Math.max(...rows.map((r) => r.overlay));
  const report = [
    `MingShot bench: ${runs} runs, ${rows[0]?.t.displays} displays`,
    `positive control (window left up): ${control} magenta px ${control > 0 ? '(detector works)' : '(DETECTOR FOUND NOTHING)'}`,
    `New → overlay up (Rust):                     ${stat(rows.map((r) => r.t.ui_ms))}`,
    `New → pointer's display frozen (Rust, M0):   ${stat(rows.map((r) => r.t.frozen_ms))}`,
    `New → every display frozen (Rust):           ${stat(rows.map((r) => r.t.all_ms))}`,
    `click → start_capture returned (page, all):  ${stat(rows.map((r) => r.click))}`,
    `  capture (last display):                    ${stat(rows.map((r) => r.t.capture_ms))}`,
    `capture window in a frame: ${leaks} of ${runs} runs${leaks ? ' (max ' + Math.max(...rows.map((r) => r.window)) + ' px)' : ''}`,
    `overlay in a frame: largest magenta share of any display ${(overlayMax * 100).toFixed(2)}% (opaque overlay ≈ 100%)`,
  ].join('\n');
  await invoke('bench_done', { report });
}
