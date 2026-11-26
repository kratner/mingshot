import React from 'react';
import ReactDOM from 'react-dom/client';
import CaptureWindow from './CaptureWindow';
import Overlay from './Overlay';
import './four-ounces-tokens.css';
import './app.css';

// One bundle, two kinds of window: the capture window, and one selection overlay per display
// (index.html?overlay=<display id>, made by the Rust side).
const overlay = new URLSearchParams(location.search).get('overlay');
if (overlay) document.documentElement.classList.add('is-overlay');

ReactDOM.createRoot(document.getElementById('root') as HTMLElement).render(
  <React.StrictMode>{overlay ? <Overlay display={Number(overlay)} /> : <CaptureWindow />}</React.StrictMode>,
);
