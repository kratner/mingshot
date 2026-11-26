# MingShot — Plan

A screen capture and markup tool in the Snagit mould: click New, grab any part of the screen, mark it up in a
real editor, and find it again later. Mac first, then Windows. A Four Ounces tool, free, MIT.

Created 2026-11-16. Nothing built yet; M0 is next.

**Name (working):** MingShot. 明 (*míng*) is "bright, clear": the family's character (MingTab, MingDesk). A
screenshot is the clearest way to show someone what you mean. Alternatives in §8.

---

## 1. Why

Today on the Mac: ⌃⇧⌘4 → clipboard → open Preview → File ▸ New from Clipboard → Markup toolbar → save
somewhere → lose track of it. Preview's markup is thin (no numbered steps, no blur, no callouts, no re-editing
after save) and nothing remembers what you captured.

Snagit does all of it but costs a licence per machine per year, and its Windows and Mac editions differ. Windows'
Snipping Tool is quick but its editor is a pen and a highlighter.

MingShot: a small capture window with a New button (as in Snipping Tool), a capture overlay, an editor good enough for how-to docs and bug reports, canvases that turn several
screenshots into one diagram, metadata that remembers where each shot came from, and a library.
Same app on Mac and Windows. Free.

---

## 2. What it does (the Snagit features that matter)

Grouped by milestone (§6). **Bold** = in the first usable versions (M1–M4). Keith's three roadmap pillars
(2026-11-25, §7.7) are **drawing and annotation tools** (Editor, M2), **quick canvases** that combine several
screenshots with drawings into diagrams (Canvas, M3), and **metadata** captured with every shot plus choices
for what happens to it when you paste (Metadata, M1–M3).

**Capture**
- **A New button in the app's window starts a capture**, as in Windows' Snipping Tool (§7.6). The window is a
  small toolbar: **New**, a **mode** picker (region / window / full display) and a **delay** picker (none,
  3/5/10 s). Clicking New hides the MingShot window, then the screen freezes, so menus and hover states stay
  put while you select and MingShot itself isn't in the shot.
- The global hotkey (⌃⇧2, changeable) does the same as New, for when the window is buried.
- **Region** (drag), **window** (hover highlights the window under the pointer, click to take it, with or without
  its shadow), **full display**, **all displays**.
- **Loupe** with pixel position and size readout; arrow keys nudge the selection; Shift locks the aspect ratio.
- **Repeat last region** hotkey (the same rectangle again: good for before/after shots).
- **Delay timer** (3/5/10 s) for menus that close when you press a key.
- Scrolling capture: a long web page or document in one image (M6).
- ~~Video: record a region or window, with optional microphone and webcam; trim; export MP4 or GIF (was M6).~~ Out of scope (§7.5).
- Grab text (OCR): select a region and get its text on the clipboard instead of an image (M5).

**Editor: drawing and annotation tools** (opens straight after a capture, or skip it and copy/save; the same
tools work on a canvas)
- **Arrow, line, rectangle, ellipse, freehand pen, highlighter.**
- **Text** and **callout** (speech bubble / box with a pointer).
- **Step numbers**: click, click, click → ①②③, renumber automatically when one is deleted.
- **Blur and pixelate** a region (passwords, emails, faces).
- **Crop, resize, canvas padding**; **border and drop shadow** on the whole image.
- Spotlight (dim everything except a region), magnify (a zoomed circle of a detail), stamps (✓ ✗ ! ?), cut out
  a horizontal or vertical strip (Snagit's Cut Out), simplify (M5).
- **Everything stays editable**: shapes are objects, not pixels, until you export. Undo/redo throughout.
- **Styles**: a small set of preset looks (Four Ounces red, high-contrast, subtle) plus the last-used style.

**Canvas: several screenshots + drawings → a diagram** (M3)
- **New canvas in one click** from the capture window (next to New), blank or at a preset size (16:9 slide,
  A4/Letter page, square, or fit to content as you add).
- **Capture straight onto it:** while a canvas is open, the capture window offers "Add to canvas", so each New
  drops the shot onto the canvas instead of opening the editor. Shoot five screens in a row, then arrange.
- **Paste or drop anything in:** ⌘V pastes the clipboard image; drag in captures from the library, image files
  or images from a browser. Each becomes an image object you can move, scale, crop in place, and give its own
  border and shadow.
- **Every editor tool works on the canvas** (arrows, shapes, text, callouts, step numbers, blur), across and
  between the images.
- **Diagram helpers:** connectors that attach to an image or shape and follow it when it moves; snapping and
  alignment guides; align and distribute; arrange a selection in a row, column or grid with even gaps; group;
  layer order; background colour or dot grid.
- **Saved as one editable `.mingshot` canvas file** (scene JSON with its images bundled), exported or copied
  as PNG or PDF like a single capture. A canvas is a library item like any capture.

**Metadata: captured with every shot, handled your way on paste**
- **Recorded at capture (M1):** date and time; source app (name and bundle id); window title; the page URL when
  the source is a browser; the document's file path when the app exposes it; display, scale and pixel size;
  the region's screen position. OCR text joins it in M5.
- **Kept with the image:** in the `.mingshot` sidecar and the library index, and optionally written into the
  PNG itself (XMP / PNG text chunks) so it travels with the file.
- **Choices when copying or pasting (M2, canvas in M3):** a default in settings plus a small menu on copy
  (and ⌥⌘C / ⌥⌘V for "ask me this time"):
  - *Image only*: metadata stripped. Nothing about your screen leaks.
  - *Image + caption*: a line such as "Chrome · Page title · URL · 25 Nov 2026 14:02" under the image (on a
    canvas it becomes an editable text object linked to the image).
  - *Image with embedded metadata*: the PNG carries it for apps that read XMP; looks the same everywhere else.
  - *Rich text*: image plus a linked caption, for Google Docs, Slack, Mail and Notes.
  - *Markdown*: image file plus `[title](url)` source line, for READMEs and docs.
- **On a canvas:** each pasted image keeps its own metadata, shown in an inspector; its caption can be toggled;
  a "Sources" list (every image's app, title and URL) can be added to the canvas or the export.
- Field-by-field switches (e.g. never include URLs) so the caption only shows what you want shared.

**Output**
- **Copy to clipboard** (the usual end point), **save as PNG/JPEG**, **drag the image out** of the editor into
  any app, PDF (M5). Metadata goes with it per the paste choice above.
- **Auto-save** every capture to the library so nothing is lost even if you only copied it.
- Share links: not hosted by us (zero-cost rule). "Save to a folder" covers Google Drive/Dropbox/iCloud folders
  that sync themselves.

**Library**
- **Every capture with date, source app and window title, size**; grid with search, tags, stars.
- Search includes the captured metadata (app, title, URL) and OCR'd text (M5): find "that error dialog" by what
  it said, or every shot taken from one site.
- Files live in a normal folder (`~/Pictures/MingShot`, changeable) as `.png` plus a `.mingshot` sidecar holding
  the editable objects, so the folder is useful without the app and backs up with everything else.

---

## 3. Native app, not a browser extension

A browser extension can't do the job:

- It can only capture **inside the browser**. Capturing the desktop needs the screen-share picker
  (`getDisplayMedia`) on every capture, which is slower than what you have now.
- Its keyboard shortcuts only work while Chrome is running, and it can't freeze the screen or highlight other
  apps' windows.

So MingShot is a desktop app. It does **not** have to go through the App Store:

- **Mac, direct download (recommended):** a signed and notarized `.dmg` from GitHub Releases (and a Homebrew
  cask). Signing needs the **Apple Developer Program, $99/year**, the same decision as MingDesk (§7). Until
  then, builds run on Keith's Macs with a free self-signed certificate.
- **Mac App Store (optional, later):** possible (ScreenCaptureKit works inside the sandbox), but the sandbox
  makes the folder-based library and global hotkeys fussier, and review adds a delay to every release. Revisit
  once the direct version is settled.
- **Windows:** an installer from GitHub Releases; the Microsoft Store is free to list in for individuals, so
  that's an easy second channel.

---

## 4. How it's built

- **Tauri 2 + React + TypeScript**, the same stack as MingDesk, styled with the Four Ounces tokens.
  - One codebase for Mac and Windows; the editor is the same everywhere.
  - Small download (a few MB, not Electron's 100+).
- **Capture core in Rust**, one implementation per OS behind a `Capture` trait:
  - **macOS:** ScreenCaptureKit (`screencapturekit` crate, which ships a Tauri 2 example). Needs the **Screen
    Recording** permission; window list and frames for hover-highlight come from the same API.
  - **Windows:** Windows Graphics Capture (`windows-capture` / `windows` crates); no permission prompt.
  - Overlay: one borderless, always-on-top, transparent window per display, showing the frozen frame; the
    selection UI is web code, the pixels are cropped in Rust at full Retina resolution.
- **Editor:** a canvas scene graph (Konva) where each annotation is an object with a type, geometry and style.
  The `.mingshot` sidecar is that list as JSON plus a reference to the original PNG, so re-opening a capture
  brings every arrow back editable.
- **Canvas:** the same Konva scene graph with image objects added, so editor and canvas share every tool.
  Connectors store the ids of the objects they attach to and re-route when those move.
- **Metadata:** frontmost app from `NSWorkspace`; window title from ScreenCaptureKit's window list; browser URL
  through the Accessibility API (the web area's `AXURL`; Safari, Chrome, Arc, Edge, Firefox), falling back to
  AppleScript for browsers that need it (an Automation permission prompt per browser, asked only when first
  used). Windows: UI Automation for the same fields. Written into PNGs as XMP / iTXt chunks. On copy, MingShot
  puts the chosen form on the clipboard (PNG, plus HTML/RTF for rich text, plus plain text for Markdown).
- **Library index:** SQLite (`rusqlite`) — captures, tags, OCR text (FTS5 for search). Rebuildable from the
  folder if it's ever lost.
- **OCR:** on-device and free: Apple Vision on the Mac, `Windows.Media.Ocr` on Windows. No cloud.
- ~~**Video (was M6):** ScreenCaptureKit stream → AVAssetWriter on the Mac; Windows Graphics Capture → Media
  Foundation on Windows. GIF via `gifski`.~~ Dropped (§7.5).
- **Shared with MingDesk:** window enumeration and the permission onboarding pattern. If both end up needing the
  same code, it moves into a small shared crate rather than being copied twice.

**Riskiest part:** the overlay has to appear instantly (< 150 ms from hotkey to frozen screen) and be exact on
mixed-DPI multi-monitor setups. M0 is a spike that proves this before anything else is built. If a webview
overlay can't hit it, the overlay becomes native (AppKit / Win32) and the editor stays web.

**Considered and not chosen:** SwiftUI (best on the Mac, but no Windows and a second stack to maintain);
Electron (heavy, and no better at capture).

---

## 5. Cost

- To users: free, no account, no telemetry, nothing uploaded. Donations through Ko-fi as with MingTab.
- To Keith: nothing until the Apple Developer Program ($99/yr), which MingDesk needs too, so one enrolment
  covers both. No servers.

---

## 6. Milestones

Each milestone ends with tests that ran plus what Keith checks by hand.

| | Milestone | Done when |
|---|---|---|
| **M0** | **Spike: New button → window hides → frozen overlay → region → PNG on the clipboard**, Mac only. Capture window with the New button (§7.6); hotkey triggers the same path. Screen Recording onboarding. | Click-to-overlay time measured, including hiding the window (target < 150 ms); the MingShot window never appears in a capture; a captured region compared pixel-for-pixel with `screencapture -R` on a Retina display and on an external display at a different scale. |
| **M1** | **Capture done properly + metadata:** window capture with hover highlight (with/without shadow), full display, loupe, nudge, repeat last region, delay timer; copy / save / open in editor choices. **Metadata recorded with every capture** (time, app, window title, browser URL, file path, display) into the sidecar and optionally the PNG. | Rust tests on selection-to-pixel maths per display scale; metadata checked against known windows in Safari, Chrome, Finder, VS Code and a no-title app; by hand on laptop alone and docked. |
| **M2** | **Drawing and annotation tools (editor):** arrow, line, shapes, pen, highlighter, text, callout, step numbers, blur/pixelate, crop, border and shadow, styles, undo; copy, save, drag out; `.mingshot` sidecar re-opens editable. **Metadata choices on copy:** image only / caption / embedded / rich text / Markdown, default in settings, per-copy menu. **First version Keith uses daily.** | Unit tests on the scene model and step renumbering; export snapshots compared against reference PNGs; each copy mode pasted into Google Docs, Slack, Mail, Notes and a Markdown file and checked; Playwright tests on the editor UI. |
| **M3** | **Canvas:** new canvas in one click, "Add to canvas" capture, paste/drop images in, every editor tool on the canvas, connectors that follow objects, snapping, align/distribute, row/column/grid arrange, groups, layers, background; per-image metadata, captions and a Sources list; save as one `.mingshot` canvas; PNG/PDF export. | Scene-model tests for connectors and arrange; export snapshots of reference canvases; by hand: a five-screenshot how-to diagram built in under five minutes. |
| **M4** | **Library:** auto-save (captures and canvases), grid, search by app/title/URL/date, tags, stars, re-open to edit, open folder. | Index rebuild from the folder test; by hand for a week. |
| **M5** | **Text and polish:** OCR "grab text", OCR'd library search, spotlight, magnify, stamps, cut out, simplify, PDF export. | OCR accuracy checked on a fixed set of screenshots. |
| **M6** | **Windows** port of M0–M5, installer. Scrolling capture on both. | M0–M4 tests on Windows; scrolling capture of a long page compared with a known full-page render. |
| ~~Video~~ | ~~Record region/window with mic and webcam, trim, MP4 and GIF (was M6).~~ **Dropped 2026-11-16 (§7.5): stills only.** | |
| later | Canvas templates (Snagit-style multi-image layouts), presets that capture and save in one keystroke, a hand-off to MingTab/MingDesk. | |

---

## 7. Decisions (Keith, 2026-11-16)

All settled; tracked in kratner/planning#1 (private).

1. **Name:** MingShot.
2. **Apple Developer Program ($99/yr):** not yet. Self-signed until MingShot or MingDesk is ready for other
   people; enrol then (one membership covers both).
3. **Default hotkey:** ⌃⇧2 (no clash with macOS's ⌘⇧3/4/5); changeable in settings. No takeover of ⌘⇧4.
4. **Repo:** public, MIT: <https://github.com/kratner/mingshot>.
5. **Video:** out of scope. Stills only; video stays with existing tools (⌘⇧5, Loom). M6 is dropped.
6. **Trigger (2026-11-25):** a capture is started by a **New button in the app's interface**, like Windows'
   Snipping Tool: a small window with New, a mode picker and a delay picker. The window hides itself before
   the screen freezes. The ⌃⇧2 hotkey from decision 3 stays as a shortcut to the same action.
7. **Roadmap pillars (2026-11-25):** the roadmap must include (a) drawing and annotation tools, (b) quick canvas
   creation: capture and paste several screenshots onto one canvas and combine them with drawings and
   annotations to build diagrams and explanatory illustrations, and (c) metadata captured with each shot, with
   options for handling it when pasting. Placed as M2, M3 and M1–M3 (§6); the library moves to M4.
   Open: the default paste mode. Proposed *image only* (nothing about your screen leaks by accident), with the
   caption one click away.

---

## 8. Names

All in the 明 family; none of these turned up an existing screenshot app in a quick search (2026-11-16).

| Name | Says | Note |
|---|---|---|
| **MingShot** | bright shot | Clear and short; the obvious one. |
| MingSnap | bright snap | Echoes Snagit/Snipping Tool; "snap" also means window snapping (MingDesk territory). |
| MingMark | bright mark | Leans on the markup editor, which is the real difference from the built-in tools. |
| MingLens | bright lens | Softer; fits if video and OCR become big. |

Recommendation: **MingShot**. It says what the hotkey does; MingMark is the runner-up if the editor turns out to
be the headline.
