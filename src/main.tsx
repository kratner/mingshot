import React from 'react';
import ReactDOM from 'react-dom/client';
import CaptureWindow from './CaptureWindow';
import Overlay from './Overlay';
import Editor from './Editor';
import './four-ounces-tokens.css';
import './app.css';

// One bundle, three kinds of window: the capture window, one selection overlay per display
// (index.html?overlay=<display id>, made by the Rust side).
// (index.html?overlay=<display id>, made by the Rust side), and editor windows (index.html?editor=<id>).
const params = new URLSearchParams(location.search);
const overlay = params.get('overlay');
const editor = params.get('editor');
if (overlay) document.documentElement.classList.add('is-overlay');

// Browser test of the editor (scripts/editor-ui-test.mjs): a mock of the Rust side, only outside Tauri.
const mock = params.get('mock') && !('__TAURI_INTERNALS__' in window) ? import('./mock').then((m) => m.installMock()) : Promise.resolve();

mock.then(() => ReactDOM.createRoot(document.getElementById('root') as HTMLElement).render(
  <React.StrictMode>
    {overlay ? <Overlay display={Number(overlay)} /> : editor ? <Editor id={Number(editor)} /> : <CaptureWindow />}
  </React.StrictMode>,
));
