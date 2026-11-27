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
type Region = { display: number; rect: { x: number; y: number; w: number; h: number } };
type Captured = {
  width: number;
  height: number;
  bytes: number;
  copy_ms: number;
  timings: Timings;
  /** A dragged region (what Repeat takes again); null for a window or display. */
  region: Region | null;
  /** This was Repeat. */
  repeated: boolean;
  /** Where it went. */
  delivered: Delivered;
};
type Delivered = { copied: boolean; saved: string | null };
/** What MingShot recorded about a capture (metadata.rs); arrives just after the capture itself. */
type Meta = {
  captured_at: string;
  kind: string;
  source: {
    app: string;
    bundle_id: string | null;
    window_title: string;
    url: string | null;
    file_path: string | null;
  } | null;
};
type After = 'copy' | 'save' | 'both';

type Mode = 'region' | 'window' | 'display';

/// Per-viewer conveniences (mode, shadow, delay); the app works the same without them.
function stored(key: string): string | null {
  try {
    return localStorage.getItem(`mingshot.${key}`);
  } catch {
    return null;
  }
}
/// Test runs (e2e, bench) share this storage with the real app on the same Mac, so they never write to it: a test's
/// region once showed up as the user's "last region" (Keith, 2026-11-26).
const testRun: Promise<boolean> = invoke<boolean>('test_run').catch(() => false);

function store(key: string, value: string) {
  testRun.then((test) => {
    if (test) return;
    try {
      localStorage.setItem(`mingshot.${key}`, value);
    } catch {
      /* private storage unavailable: settings just aren't remembered */
    }
  });
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
  const [after, setAfter] = useState<After>(() => (stored('after') as After) ?? 'copy');
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
    store('after', after);
    invoke('set_mode', { mode, shadow, after });
  }, [mode, shadow, delay, after]);

  // The last dragged region, for Repeat (⌃⇧3); remembered between launches.
  const [lastRegion, setLastRegion] = useState<Region | null>(() => {
    try {
      return JSON.parse(stored('lastRegion') ?? 'null');
    } catch {
      return null;
    }
  });

  // The last capture's metadata, and whether Accessibility (web addresses, file paths) is allowed.
  const [meta, setMeta] = useState<Meta | null>(null);
  const [axAllowed, setAxAllowed] = useState(true);

  useEffect(() => {
    invoke<boolean>('permission').then(setPermitted);
    invoke<boolean>('accessibility', { request: false }).then(setAxAllowed);
    invoke('set_last_region', { region: lastRegion }).catch(() => {});
    const offMeta = listen<Meta>('metadata', (e) => setMeta(e.payload));
    const offCaptured = listen<Captured>('captured', (e) => {
      setMeta(null);
      setStatus({ kind: 'copied', captured: e.payload, clickMs: clickMs.current });
      if (e.payload.region) {
        setLastRegion(e.payload.region);
        store('lastRegion', JSON.stringify(e.payload.region));
      }
    });
    const offError = listen<string>('capture-error', (e) => setStatus({ kind: 'error', message: e.payload }));
    const offCancelled = listen('cancelled', () => setStatus({ kind: 'cancelled' }));
    // Re-check the permission whenever the window comes back (after a trip to System Settings).
    const onFocus = () => {
      invoke<boolean>('permission').then(setPermitted);
      invoke<boolean>('accessibility', { request: false }).then(setAxAllowed);
    };
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
      offError.then((f) => f());
      offMeta.then((f) => f());
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

  async function onRepeat() {
    try {
      await invoke('repeat_region');
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
        <button
          className="repeat"
          onClick={onRepeat}
          disabled={permitted === false || !lastRegion}
          title={
            lastRegion
              ? `Take the last region again: ${Math.round(lastRegion.rect.w)} × ${Math.round(lastRegion.rect.h)} pt (⌃⇧3)`
              : 'Repeat the last region (⌃⇧3): drag a region first'
          }
        >
          ↻ Repeat
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
        <div className="group">
          <span className="group-label">After capture</span>
          <Segmented
            value={after}
            onChange={setAfter}
            options={[
              ['copy', 'Copy', 'Put it on the clipboard'],
              ['save', 'Save', 'Save a PNG in Pictures › MingShot'],
              ['both', 'Both', 'Copy and save'],
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
          <StatusLine
            status={status}
            mode={mode}
            onDelivered={(d) =>
              setStatus((st) => (st.kind === 'copied' ? { ...st, captured: { ...st.captured, delivered: d } } : st))
            }
            onError={(message) => setStatus({ kind: 'error', message })}
          />
        )}
        {status.kind === 'copied' && meta?.source && (
          <SourceLine
            meta={meta}
            axAllowed={axAllowed}
            onAllow={() => invoke<boolean>('accessibility', { request: true }).then(setAxAllowed)}
          />
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
  region: 'New or ⌃⇧2, then drag a region. Esc cancels. Repeat (⌃⇧3) takes the last region again.',
  window: 'New or ⌃⇧2, then click a window (or drag a region). Esc cancels.',
  display: 'New or ⌃⇧2, then click a display (or drag a region). Esc cancels.',
};

/// Where the capture came from: app and window title, and the page or file when MingShot may ask (Accessibility).
function SourceLine({ meta, axAllowed, onAllow }: { meta: Meta; axAllowed: boolean; onAllow: () => void }) {
  const src = meta.source!;
  // Remember that the permission was missing for this capture, so granting it says so instead of the link just
  // disappearing (Keith, 2026-11-26).
  const [askedFor, setAskedFor] = useState(!axAllowed);
  useEffect(() => {
    if (!axAllowed) setAskedFor(true);
  }, [axAllowed]);
  const where = src.url ?? (src.file_path ? fileName(src.file_path) : null);
  return (
    <div className="status-sub source" title={[src.app, src.window_title, src.url ?? src.file_path].filter(Boolean).join('\n')}>
      from <strong>{src.app}</strong>
      {src.window_title && <> · {src.window_title}</>}
      {where && <> · {where}</>}
      {!where && !axAllowed && (
        <>
          {' '}
          <button className="link quiet" onClick={onAllow} title="Needs Accessibility; macOS will ask">
            Add web addresses and file paths…
          </button>
        </>
      )}
      {!where && axAllowed && askedFor && (
        <span className="ax-on"> · Web addresses and file paths: on, from the next capture</span>
      )}
    </div>
  );
}

/// The file's name, from its full path.
const fileName = (path: string) => path.slice(path.lastIndexOf('/') + 1);

function StatusLine({
  status,
  mode,
  onDelivered,
  onError,
}: {
  status: Status;
  mode: Mode;
  onDelivered: (d: Delivered) => void;
  onError: (message: string) => void;
}) {
  const act = (action: 'copy' | 'save' | 'reveal' | 'open') =>
    invoke<Delivered>('last_capture', { action }).then(onDelivered, (e) => onError(String(e)));
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
      const d = c.delivered;
      const size = (
        <strong>
          {c.width} × {c.height}
        </strong>
      );
      return (
        <>
          <div className="status-main">
            <span className="tick">✓</span>{' '}
            {d.copied && d.saved ? (
              <>Copied and saved {size} px</>
            ) : d.saved ? (
              <>Saved {size} px to Pictures › MingShot</>
            ) : (
              <>Copied {size} px to the clipboard</>
            )}
            <span className="actions">
              {!d.copied && (
                <button className="link" onClick={() => act('copy')}>
                  Copy
                </button>
              )}
              {!d.saved && (
                <button className="link" onClick={() => act('save')} title="Save a PNG in Pictures › MingShot">
                  Save
                </button>
              )}
              {d.saved && (
                <button className="link" onClick={() => act('reveal')} title={d.saved}>
                  Show in Finder
                </button>
              )}
              <button className="link" onClick={() => act('open')} title="Open in your image app (saves it first)">
                Open
              </button>
            </span>
          </div>
          {c.repeated ? (
            <div className="status-sub">
              Same region as last time{d.saved ? ` · ${fileName(d.saved)}` : ''}
            </div>
          ) : (
            <div className="status-sub timings" title={title}>
              {d.saved ? `${fileName(d.saved)} · ` : ''}overlay {Math.round(t.ui_ms)} ms · frozen{' '}
              {Math.round(t.frozen_ms)} ms
            </div>
          )}
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
  return {
    kind: 'copied',
    clickMs: null,
    captured: {
      width: 876,
      height: 666,
      bytes: 0,
      copy_ms: 9,
      timings,
      region: null,
      repeated: false,
      delivered: { copied: true, saved: '/Users/you/Pictures/MingShot/MingShot 2026-11-26 at 17.42.05.png' },
    },
  };
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
