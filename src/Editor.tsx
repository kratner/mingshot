// The editor window (M2, PLAN.md §2 "Editor"): the capture as a plain image underneath, and a transparent Konva
// stage over it holding the annotations, in the capture's own pixel coordinates (scene.ts). Copy and Save send only
// the annotation layer, rendered at the capture's full size, to Rust, which composites it over the untouched
// original (editor.rs), so every pixel no annotation covers stays exactly as captured.

import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import Konva from 'konva';
import { Arrow as KArrow, Layer, Rect as KRect, Stage, Text as KText, Transformer } from 'react-konva';
import {
  COLOURS,
  WIDTHS,
  add,
  commit,
  move,
  newId,
  normalise,
  redo,
  remove,
  start,
  tooSmall,
  undo,
  update,
  type History,
  type Label,
  type Scene,
  type Shape,
} from './scene';

type Info = { width: number; height: number; scale: number; title: string; annotations: Scene; saved: string | null };
type Tool = 'select' | 'arrow' | 'rect' | 'text';
type Delivered = { copied: boolean; saved: string | null };

const TOOLS: [Tool, string, string][] = [
  ['select', 'Select', 'V'],
  ['arrow', 'Arrow', 'A'],
  ['rect', 'Box', 'R'],
  ['text', 'Text', 'T'],
];
const TEXT_SIZE_PT = 18;
const FONT = '-apple-system, "Helvetica Neue", Helvetica, Arial, sans-serif';

export default function Editor({ id }: { id: number }) {
  const [info, setInfo] = useState<Info | null>(null);
  const [imageUrl, setImageUrl] = useState<string | null>(null);
  const [history, setHistory] = useState<History>(start());
  const [tool, setTool] = useState<Tool>('arrow');
  const [colour, setColour] = useState(COLOURS[0]);
  const [weight, setWeight] = useState<keyof typeof WIDTHS>('M');
  const [selected, setSelected] = useState<string | null>(null);
  const [draft, setDraft] = useState<Shape | null>(null);
  const [typing, setTyping] = useState<{ id: string | null; x: number; y: number; value: string } | null>(null);
  const [fit, setFit] = useState(1);
  const [message, setMessage] = useState<string | null>(null);
  const [saved, setSaved] = useState<string | null>(null);
  const shapesLayer = useRef<Konva.Layer>(null);
  const transformer = useRef<Konva.Transformer>(null);
  const area = useRef<HTMLDivElement>(null);

  const scene = history.present;
  const scale = info?.scale ?? 2;
  const style = useMemo(() => ({ color: colour, width: WIDTHS[weight] * scale }), [colour, weight, scale]);

  // Load the document and the original image.
  useEffect(() => {
    invoke<Info>('editor_doc', { id }).then((d) => {
      setInfo(d);
      setSaved(d.saved);
      setHistory(start(Array.isArray(d.annotations) ? d.annotations : []));
      document.title = d.title;
    });
    invoke<ArrayBuffer>('editor_image', { id }).then((buf) => {
      setImageUrl(URL.createObjectURL(new Blob([buf], { type: 'image/png' })));
    });
  }, [id]);

  // Fit the capture in the window, never larger than one image pixel per screen pixel.
  useEffect(() => {
    if (!info) return;
    const measure = () => {
      const el = area.current;
      if (!el) return;
      const k = Math.min((el.clientWidth - 32) / info.width, (el.clientHeight - 32) / info.height, 1 / devicePixelRatio);
      setFit(Math.max(0.05, k));
    };
    measure();
    window.addEventListener('resize', measure);
    return () => window.removeEventListener('resize', measure);
  }, [info]);

  // The selection outline follows the selected shape.
  useEffect(() => {
    const tr = transformer.current;
    const layer = shapesLayer.current;
    if (!tr || !layer) return;
    const node = selected ? layer.findOne(`#${selected}`) : null;
    tr.nodes(node ? [node] : []);
    tr.getLayer()?.batchDraw();
  }, [selected, scene]);

  const change = useCallback((next: Scene) => setHistory((h) => commit(h, next)), []);

  // ---- Export ------------------------------------------------------------------------------------------------

  /** The annotations alone, rendered at the capture's full pixel size, as a PNG data URL (null when there are none). */
  const overlay = (): string | null => {
    const layer = shapesLayer.current;
    if (!info || !layer || scene.length === 0) return null;
    const container = document.createElement('div');
    const stage = new Konva.Stage({ container, width: info.width, height: info.height });
    stage.add(layer.clone());
    const url = stage.toDataURL({ pixelRatio: 1, mimeType: 'image/png' });
    stage.destroy();
    return url;
  };

  const exportAs = async (action: 'copy' | 'save') => {
    setSelected(null);
    try {
      const d = await invoke<Delivered>('editor_export', { id, overlay: overlay(), annotations: scene, action });
      if (d.saved) setSaved(d.saved);
      setMessage(action === 'copy' ? 'Copied to the clipboard' : `Saved ${fileName(d.saved ?? '')}`);
    } catch (e) {
      setMessage(String(e));
    }
  };

  // ---- Keyboard ----------------------------------------------------------------------------------------------

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (typing) return; // the text box has the keyboard
      const cmd = e.metaKey || e.ctrlKey;
      const k = e.key.toLowerCase();
      if (cmd && k === 'z') {
        e.preventDefault();
        setHistory((h) => (e.shiftKey ? redo(h) : undo(h)));
        setSelected(null);
      } else if (cmd && k === 'c') {
        e.preventDefault();
        exportAs('copy');
      } else if (cmd && k === 's') {
        e.preventDefault();
        exportAs('save');
      } else if ((e.key === 'Backspace' || e.key === 'Delete') && selected) {
        e.preventDefault();
        change(remove(scene, selected));
        setSelected(null);
      } else if (e.key === 'Escape') {
        setSelected(null);
        setDraft(null);
      } else if (!cmd) {
        const t = TOOLS.find(([, , key]) => key.toLowerCase() === k);
        if (t) setTool(t[0]);
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  });

  // ---- Drawing -----------------------------------------------------------------------------------------------

  const pointer = (stage: Konva.Stage | null) => stage?.getRelativePointerPosition() ?? { x: 0, y: 0 };

  const onDown = (e: Konva.KonvaEventObject<MouseEvent>) => {
    const stage = e.target.getStage();
    const p = pointer(stage);
    if (tool === 'select') {
      if (e.target === stage) setSelected(null);
      return;
    }
    if (tool === 'text') {
      // Keep the browser from moving focus on this click, or it takes it straight off the new text box.
      e.evt.preventDefault();
      setTyping({ id: null, x: p.x, y: p.y, value: '' });
      return;
    }
    const id = newId(scene);
    setDraft(
      tool === 'arrow'
        ? { id, kind: 'arrow', x1: p.x, y1: p.y, x2: p.x, y2: p.y, style }
        : { id, kind: 'rect', x: p.x, y: p.y, w: 0, h: 0, style },
    );
  };

  const onMove = (e: Konva.KonvaEventObject<MouseEvent>) => {
    if (!draft) return;
    const p = pointer(e.target.getStage());
    setDraft(draft.kind === 'arrow' ? { ...draft, x2: p.x, y2: p.y } : draft.kind === 'rect' ? { ...draft, w: p.x - draft.x, h: p.y - draft.y } : draft);
  };

  const onUp = () => {
    if (!draft) return;
    const done = draft.kind === 'rect' ? normalise(draft) : draft;
    if (!tooSmall(done, 3 * scale)) change(add(scene, done));
    setDraft(null);
  };

  const finishTyping = (keep: boolean) => {
    if (!typing) return;
    const text = typing.value.replace(/\s+$/, '');
    if (keep && text) {
      if (typing.id) change(update<Label>(scene, typing.id, { text }));
      else change(add(scene, { id: newId(scene), kind: 'text', x: typing.x, y: typing.y, text, style: { color: colour, width: TEXT_SIZE_PT * scale } }));
    } else if (keep && typing.id) {
      change(remove(scene, typing.id));
    }
    setTyping(null);
  };

  const dragEnd = (s: Shape) => (e: Konva.KonvaEventObject<DragEvent>) => {
    const node = e.target;
    const [dx, dy] = s.kind === 'arrow' ? [node.x(), node.y()] : [node.x() - s.x, node.y() - s.y];
    if (s.kind === 'arrow') node.position({ x: 0, y: 0 });
    change(move(scene, s.id, dx, dy));
  };

  // ---- Rendering ---------------------------------------------------------------------------------------------

  const node = (s: Shape) => {
    const common = {
      id: s.id,
      key: s.id,
      draggable: tool === 'select',
      onMouseDown: () => tool === 'select' && setSelected(s.id),
      onDragStart: () => setSelected(s.id),
      onDragEnd: dragEnd(s),
    };
    if (s.kind === 'arrow') {
      const w = s.style.width;
      return (
        <KArrow
          {...common}
          points={[s.x1, s.y1, s.x2, s.y2]}
          stroke={s.style.color}
          fill={s.style.color}
          strokeWidth={w}
          pointerLength={w * 3.2}
          pointerWidth={w * 3.2}
          lineCap="round"
          lineJoin="round"
          hitStrokeWidth={Math.max(w * 3, 12 * scale)}
        />
      );
    }
    if (s.kind === 'rect') {
      return (
        <KRect
          {...common}
          x={s.x}
          y={s.y}
          width={s.w}
          height={s.h}
          stroke={s.style.color}
          strokeWidth={s.style.width}
          cornerRadius={s.style.width}
          hitStrokeWidth={Math.max(s.style.width * 3, 12 * scale)}
        />
      );
    }
    return (
      <KText
        {...common}
        x={s.x}
        y={s.y}
        text={s.text}
        fontSize={s.style.width}
        fontFamily={FONT}
        fontStyle="bold"
        fill={s.style.color}
        visible={typing?.id !== s.id}
        onDblClick={() => setTyping({ id: s.id, x: s.x, y: s.y, value: s.text })}
      />
    );
  };

  if (!info || !imageUrl) return <div className="editor-loading">Opening…</div>;
  const w = info.width * fit;
  const h = info.height * fit;

  return (
    <div className="editor">
      <div className="editor-toolbar">
        <div className="segmented" role="group" aria-label="Tool">
          {TOOLS.map(([t, label, key]) => (
            <button key={t} className={t === tool ? 'seg on' : 'seg'} aria-pressed={t === tool} title={`${label} (${key})`} onClick={() => setTool(t)}>
              {label}
            </button>
          ))}
        </div>
        <div className="swatches" role="group" aria-label="Colour">
          {COLOURS.map((c) => (
            <button
              key={c}
              className={c === colour ? 'swatch on' : 'swatch'}
              style={{ background: c }}
              title={c}
              aria-pressed={c === colour}
              onClick={() => {
                setColour(c);
                if (selected) change(scene.map((s) => (s.id === selected ? { ...s, style: { ...s.style, color: c } } : s)));
              }}
            />
          ))}
        </div>
        <div className="segmented" role="group" aria-label="Line width">
          {(Object.keys(WIDTHS) as (keyof typeof WIDTHS)[]).map((k) => (
            <button key={k} className={k === weight ? 'seg on' : 'seg'} aria-pressed={k === weight} title={`${WIDTHS[k]} pt lines`} onClick={() => setWeight(k)}>
              {k}
            </button>
          ))}
        </div>
        <div className="segmented" role="group" aria-label="History">
          <button className="seg" disabled={!history.past.length} title="Undo (⌘Z)" onClick={() => setHistory(undo)}>
            Undo
          </button>
          <button className="seg" disabled={!history.future.length} title="Redo (⇧⌘Z)" onClick={() => setHistory(redo)}>
            Redo
          </button>
        </div>
        <div className="editor-spacer" />
        <button className="repeat" title="Copy the annotated image (⌘C)" onClick={() => exportAs('copy')}>
          Copy
        </button>
        <button className="new editor-save" title="Save to Pictures › MingShot (⌘S)" onClick={() => exportAs('save')}>
          Save
        </button>
      </div>

      <div className="editor-area" ref={area}>
        <div className="editor-canvas" style={{ width: w, height: h }}>
          <img src={imageUrl} width={w} height={h} alt="" draggable={false} />
          <Stage
            width={w}
            height={h}
            scaleX={fit}
            scaleY={fit}
            onMouseDown={onDown}
            onMouseMove={onMove}
            onMouseUp={onUp}
            style={{ cursor: tool === 'select' ? 'default' : tool === 'text' ? 'text' : 'crosshair' }}
          >
            <Layer ref={shapesLayer}>
              {scene.map(node)}
              {draft && node(draft)}
            </Layer>
            <Layer>
              <Transformer ref={transformer} resizeEnabled={false} rotateEnabled={false} borderStroke="#C9A24A" borderStrokeWidth={1.5} />
            </Layer>
          </Stage>
          {typing && (
            <textarea
              className="editor-text"
              autoFocus
              value={typing.value}
              style={{
                left: typing.x * fit,
                top: typing.y * fit,
                fontSize: TEXT_SIZE_PT * scale * fit,
                color: colour,
                fontFamily: FONT,
              }}
              onChange={(e) => setTyping({ ...typing, value: e.target.value })}
              onBlur={() => finishTyping(true)}
              onKeyDown={(e) => {
                if (e.key === 'Escape') finishTyping(false);
                else if (e.key === 'Enter' && !e.shiftKey) {
                  e.preventDefault();
                  finishTyping(true);
                }
              }}
            />
          )}
        </div>
      </div>

      <div className="editor-status">
        <span>
          {info.width} × {info.height} px · {scene.length} annotation{scene.length === 1 ? '' : 's'}
          {saved ? ` · ${fileName(saved)}` : ''}
        </span>
        {message && <span className="editor-message">{message}</span>}
      </div>
    </div>
  );
}

const fileName = (path: string) => path.slice(path.lastIndexOf('/') + 1);
