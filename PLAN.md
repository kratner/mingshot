# MingShot — Plan

A screen capture and markup tool in the Snagit mould: press a key, grab any part of the screen, mark it up in a
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

MingShot: one hotkey, a capture overlay, an editor good enough for how-to docs and bug reports, and a library.
Same app on Mac and Windows. Free.

---

## 2. What it does (the Snagit features that matter)

Grouped by milestone (§6). **Bold** = in the first usable version (M1–M3).

**Capture**
- **One global hotkey opens the overlay** (default ⌃⇧2, changeable). The screen freezes at that instant, so
  menus and hover states stay put while you select.
- **Region** (drag), **window** (hover highlights the window under the pointer, click to take it, with or without
  its shadow), **full display**, **all displays**.
- **Loupe** with pixel position and size readout; arrow keys nudge the selection; Shift locks the aspect ratio.
- **Repeat last region** hotkey (the same rectangle again: good for before/after shots).
- **Delay timer** (3/5/10 s) for menus that close when you press a key.
- Scrolling capture: a long web page or document in one image (M5).
- Video: record a region or window, with optional microphone and webcam; trim; export MP4 or GIF (M6).
- Grab text (OCR): select a region and get its text on the clipboard instead of an image (M4).

**Editor** (opens straight after a capture, or skip it and copy/save)
- **Arrow, line, rectangle, ellipse, freehand pen, highlighter.**
- **Text** and **callout** (speech bubble / box with a pointer).
- **Step numbers**: click, click, click → ①②③, renumber automatically when one is deleted.
- **Blur and pixelate** a region (passwords, emails, faces).
- **Crop, resize, canvas padding**; **border and drop shadow** on the whole image.
- Spotlight (dim everything except a region), magnify (a zoomed circle of a detail), stamps (✓ ✗ ! ?), cut out
  a horizontal or vertical strip (Snagit's Cut Out), simplify (M4).
- **Everything stays editable**: shapes are objects, not pixels, until you export. Undo/redo throughout.
- **Styles**: a small set of preset looks (Four Ounces red, high-contrast, subtle) plus the last-used style.

**Output**
- **Copy to clipboard** (the usual end point), **save as PNG/JPEG**, **drag the image out** of the editor into
  any app, PDF (M4).
- **Auto-save** every capture to the library so nothing is lost even if you only copied it.
- Share links: not hosted by us (zero-cost rule). "Save to a folder" covers Google Drive/Dropbox/iCloud folders
  that sync themselves.

**Library**
- **Every capture with date, source app and window title, size**; grid with search, tags, stars.
- Search includes OCR'd text (M4): find "that error dialog" by what it said.
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
- **Library index:** SQLite (`rusqlite`) — captures, tags, OCR text (FTS5 for search). Rebuildable from the
  folder if it's ever lost.
- **OCR:** on-device and free: Apple Vision on the Mac, `Windows.Media.Ocr` on Windows. No cloud.
- **Video (M6):** ScreenCaptureKit stream → AVAssetWriter on the Mac; Windows Graphics Capture → Media
  Foundation on Windows. GIF via `gifski`.
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
| **M0** | **Spike: hotkey → frozen overlay → region → PNG on the clipboard**, Mac only. Screen Recording onboarding. Menu-bar icon. | Hotkey-to-overlay time measured (target < 150 ms); a captured region compared pixel-for-pixel with `screencapture -R` on a Retina display and on an external display at a different scale. |
| **M1** | **Capture done properly:** window capture with hover highlight (with/without shadow), full display, loupe, nudge, repeat last region, delay timer; copy / save / open in editor choices. | Rust tests on selection-to-pixel maths per display scale; by hand on laptop alone and docked. |
| **M2** | **Editor:** arrow, line, shapes, pen, highlighter, text, callout, step numbers, blur/pixelate, crop, border and shadow, styles, undo; copy, save, drag out; `.mingshot` sidecar re-opens editable. | Unit tests on the scene model and step renumbering; export snapshots compared against reference PNGs; Playwright tests on the editor UI. |
| **M3** | **Library:** auto-save, grid, search by app/title/date, tags, stars, re-open to edit, open folder. **First version Keith uses daily.** | Index rebuild from the folder test; by hand for a week. |
| **M4** | **Text and polish:** OCR "grab text", OCR'd library search, spotlight, magnify, stamps, cut out, simplify, PDF export. | OCR accuracy checked on a fixed set of screenshots. |
| **M5** | **Windows** port of M0–M4, installer. Scrolling capture on both. | M0–M3 tests on Windows; scrolling capture of a long page compared with a known full-page render. |
| **M6** | **Video:** record region/window with mic and webcam, trim, MP4 and GIF. | A recorded test pattern checked for dropped frames and A/V sync. |
| later | Templates (Snagit's multi-image layouts), presets that capture and save in one keystroke, a hand-off to MingTab/MingDesk. | |

---

## 7. Decisions needed (Keith)

1. **Name:** MingShot, or one of §8?
2. **Apple Developer Program ($99/yr):** enrol now (covers MingShot and MingDesk), or keep self-signed until
   one of them is ready for other people?
3. **Default hotkey:** ⌃⇧2 (doesn't clash with macOS's ⌘⇧3/4/5), or take over ⌘⇧4 / Print Screen when
   MingShot is running, as Snagit and Snipping Tool can?
4. ~~Repo~~ **Decided 2026-11-16:** public, MIT: <https://github.com/kratner/mingshot>. Decisions are tracked in
   kratner/planning (private).
5. **Video (M6):** keep it in scope, or leave video to existing tools and stop at stills?

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
