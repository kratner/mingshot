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
  const [status, setStatus] = useState<Status>(demoStatus);
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
          <span className="plus">+</span>New
        </button>
        <div className="group">
          <span className="group-label">Mode</span>
          <Segmented
            value={mode}
            onChange={setMode}
            options={[
              ['region', 'Region', 'Drag a region'],
              ['window', 'Window', 'Click a window'],
              ['display', 'Display', 'Click a display'],
            ]}
          />
        </div>
        <div className="group">
          <span className="group-label">Delay</span>
          <Segmented
            value={delay}
            onChange={setDelay}
            options={[
              [0, 'Off', 'No delay'],
              [3, '3s', '3 seconds, to open a menu first'],
              [5, '5s', '5 seconds'],
              [10, '10s', '10 seconds'],
            ]}
          />
        </div>
        {mode === 'window' && (
          <div className="group">
            <span className="group-label">Shadow</span>
            <Segmented
              value={shadow}
              onChange={setShadow}
              options={[
                [true, 'On', "Include the window's drop shadow (transparent around it)"],
                [false, 'Off', 'Just the window'],
              ]}
            />
          </div>
        )}
      </div>
      <div className="status">
        {permitted === false ? (
          <div className="status-main warn">
            MingShot needs Screen Recording to see the screen.{' '}
            <button className="link" onClick={() => invoke<boolean>('request_permission').then(setPermitted)}>
              Allow…
            </button>
          </div>
        ) : (
          <StatusLine status={status} mode={mode} />
        )}
      </div>
    </div>
  );
}

/// A row of buttons, one per choice. Not a <select>: its pop-up menu doesn't open in Mojave's WebKit (Keith,
/// 2026-11-26), and one click per choice is quicker anyway.
function Segmented<T extends string | number | boolean>({
  value,
  onChange,
  options,
}: {
  value: T;
  onChange: (v: T) => void;
  options: [T, string, string][];
}) {
  return (
    <div className="segmented" role="group">
      {options.map(([v, label, title]) => (
        <button
          key={String(v)}
          className={v === value ? 'seg on' : 'seg'}
          aria-pressed={v === value}
          title={title}
          onClick={() => onChange(v)}
        >
          {label}
        </button>
      ))}
    </div>
  );
}

const HINTS: Record<Mode, string> = {
  region: 'New or ⌃⇧2, then drag a region. Esc cancels.',
  window: 'New or ⌃⇧2, then click a window (or drag a region). Esc cancels.',
  display: 'New or ⌃⇧2, then click a display (or drag a region). Esc cancels.',
};

function StatusLine({ status, mode }: { status: Status; mode: Mode }) {
  switch (status.kind) {
    case 'idle':
      return <div className="status-main muted">{HINTS[mode]}</div>;
    case 'cancelled':
      return (
        <>
          <div className="status-main">Cancelled</div>
          <div className="status-sub">Nothing was copied. {HINTS[mode]}</div>
        </>
      );
    case 'error':
      return <div className="status-main warn">{status.message}</div>;
    case 'copied': {
      const { captured: c } = status;
      const t = c.timings;
      const title = `Overlay up ${t.ui_ms} ms · your display frozen ${t.frozen_ms} ms · all ${t.displays} displays ${t.all_ms} ms · copied ${c.copy_ms} ms after the click`;
      return (
        <>
          <div className="status-main">
            <span className="tick">✓</span> Copied <strong>{c.width} × {c.height}</strong> px to the clipboard
          </div>
          <div className="status-sub timings" title={title}>
            Overlay {Math.round(t.ui_ms)} ms · screen frozen {Math.round(t.frozen_ms)} ms
          </div>
        </>
      );
    }
  }
}

/// `?demo=copied` / `?demo=cancelled`: shows that state, for looking at the design in a browser (no capture).
function demoStatus(): Status {
  const demo = new URLSearchParams(location.search).get('demo');
  if (demo === 'cancelled') return { kind: 'cancelled' };
  if (demo !== 'copied') return { kind: 'idle' };
  const timings = { ui_ms: 18.4, frozen_ms: 62.3, all_ms: 120.1, capture_ms: 107, displays: 3, delay_s: 0 };
  return { kind: 'copied', clickMs: null, captured: { width: 876, height: 666, bytes: 0, copy_ms: 9, timings } };
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
