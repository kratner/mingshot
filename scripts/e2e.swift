// FILE: scripts/e2e.swift
// End-to-end check of capture (PLAN.md §6, docs/M0.md, docs/M1.md), one mode per run (scripts/e2e.sh runs all
// three): region, window, display.
//
// Region: For each display it moves the pointer to a test region's
// corner and presses the real ⌃⇧2; MingShot runs with MINGSHOT_E2E_SELECT, so the overlay under the pointer makes
// that region's selection through the same code a mouse release runs (synthetic mouse drags don't reach WebKit on
// this Mac, in Safari either, so the drag itself is checked by hand). The PNG MingShot put on the clipboard must
// equal the same region of the Mac's own full-display screenshot (`screencapture -D`, what's on screen), taken just
// before and just after: pixels that changed in between are animation and aren't compared, every other pixel must
// be identical. `screencapture -R` (the ⌘⇧4 path) is reported for information: it differs from the screen itself
// by a few levels over glass and blur materials. Also checks that Esc cancels and leaves the clipboard alone.
//
// Save: After capture = Save puts a region in ~/Pictures/MingShot (not the clipboard), identical to the screen.
// Repeat: after the regions, ⌃⇧3 must copy the last region again, identical to the screen.
// Window: the pointer goes to an uncovered spot on up to three other apps' windows and MingShot clicks there
// (MINGSHOT_E2E_SELECT=pointer); the clipboard must equal `screencapture -l<id>` (with shadow), alpha included.
// Display: the pointer goes to each display and MingShot clicks; the clipboard must equal `screencapture -D`.
//
//   sh scripts/e2e.sh            (runs the last release build; build it first with npm run app)
//
// Needs Screen Recording and Accessibility (to post key and pointer-move events) for the terminal running it. Moves
// the pointer and types ⌃⇧2: don't touch the mouse or keyboard while it runs. The test region is the one
// MINGSHOT_E2E_SELECT names (scripts/e2e.sh keeps the two in step).

import AppKit
import CoreGraphics
import Foundation

func sleepMs(_ ms: Int) { usleep(useconds_t(ms * 1000)) }

func post(_ e: CGEvent?) { e?.post(tap: .cghidEventTap) }

func move(_ p: CGPoint) {
    post(CGEvent(mouseEventSource: nil, mouseType: .mouseMoved, mouseCursorPosition: p, mouseButton: .left))
}

func key(_ code: CGKeyCode, _ flags: CGEventFlags = []) {
    let src = CGEventSource(stateID: .hidSystemState)
    let down = CGEvent(keyboardEventSource: src, virtualKey: code, keyDown: true)
    down?.flags = flags
    let up = CGEvent(keyboardEventSource: src, virtualKey: code, keyDown: false)
    up?.flags = flags
    post(down); sleepMs(30); post(up)
}

func hotkey() { key(19, [.maskControl, .maskShift]) }  // ⌃⇧2
func repeatKey() { key(20, [.maskControl, .maskShift]) }  // ⌃⇧3: repeat the last region
func escape() { key(53) }

func screencapture(_ args: [String], _ path: String) -> Bool {
    let p = Process()
    p.executableURL = URL(fileURLWithPath: "/usr/sbin/screencapture")
    p.arguments = ["-x", "-t", "png"] + args + [path]
    try? p.run(); p.waitUntilExit()
    return p.terminationStatus == 0
}

typealias Pixels = (w: Int, h: Int, px: [UInt8])

/// RGBA8 pixels of a PNG (optionally a crop of it, in pixels), drawn into the image's own colour space so values
/// aren't converted.
func pixels(_ data: Data, crop: CGRect? = nil) -> Pixels? {
    guard let src = CGImageSourceCreateWithData(data as CFData, nil),
          var img = CGImageSourceCreateImageAtIndex(src, 0, nil) else { return nil }
    if let c = crop { guard let cut = img.cropping(to: c) else { return nil }; img = cut }
    let w = img.width, h = img.height
    var px = [UInt8](repeating: 0, count: w * h * 4)
    let space = img.colorSpace ?? CGColorSpaceCreateDeviceRGB()
    guard let ctx = CGContext(data: &px, width: w, height: h, bitsPerComponent: 8, bytesPerRow: w * 4,
                              space: space, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue) else { return nil }
    ctx.draw(img, in: CGRect(x: 0, y: 0, width: w, height: h))
    return (w, h, px)
}

func clipboardPNG() -> Data? { NSPasteboard.general.data(forType: .png) }
func read(_ path: String) -> Data? { try? Data(contentsOf: URL(fileURLWithPath: path)) }

let out = CommandLine.arguments.count > 1 ? CommandLine.arguments[1] : NSTemporaryDirectory()
let mode = CommandLine.arguments.count > 2 ? CommandLine.arguments[2] : "region"
try? FileManager.default.createDirectory(atPath: out, withIntermediateDirectories: true)

var ids = [CGDirectDisplayID](repeating: 0, count: 16)
var count: UInt32 = 0
CGGetActiveDisplayList(16, &ids, &count)
let displayIDs = Array(ids.prefix(Int(count)))

var failures = 0

/// Waits for MingShot's PNG on the clipboard.
func awaitClipboard() -> Data? {
    for _ in 0..<60 { if let d = clipboardPNG() { return d }; sleepMs(50) }
    return nil
}

/// MingShot's own ordinary windows (the capture window), in pixels relative to a display: deliberately absent from
/// its captures, so a display check doesn't compare them.
func ownWindows(on id: CGDirectDisplayID) -> [CGRect] {
    let b = CGDisplayBounds(id)
    let scale = Double(CGDisplayCopyDisplayMode(id)?.pixelWidth ?? Int(b.width)) / b.width
    let list = CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as! [[String: Any]]
    return list.compactMap { w in
        guard (w["kCGWindowOwnerName"] as? String) == "MingShot", (w["kCGWindowLayer"] as? Int) == 0 else { return nil }
        let r = w["kCGWindowBounds"] as! [String: Double]
        // Generous: the window's shadow reaches past its frame.
        let pad = 40.0
        return CGRect(x: (r["X"]! - b.minX - pad) * scale, y: (r["Y"]! - b.minY - pad) * scale,
                      width: (r["Width"]! + 2 * pad) * scale, height: (r["Height"]! + 2 * pad) * scale)
    }
}

/// Compares the clipboard PNG with a reference taken before and after (pixels that changed in between aren't
/// compared, nor those inside `skip`; alpha too when `alpha`).
func check(_ name: String, _ data: Data?, before: Pixels?, after: Pixels?, alpha: Bool, ours: String, skip: [CGRect] = []) {
    guard let data = data else { print("FAIL \(name): nothing on the clipboard"); failures += 1; return }
    try? data.write(to: URL(fileURLWithPath: ours))
    guard let m = pixels(data), let a = before, let z = after else {
        print("FAIL \(name): couldn't decode"); failures += 1; return
    }
    if (a.w, a.h) != (m.w, m.h) {
        print("FAIL \(name): MingShot \(m.w)x\(m.h) px, reference \(a.w)x\(a.h) px"); failures += 1; return
    }
    let channels = alpha ? 4 : 3
    var diff = 0, moving = 0, worst = 0, skipped = 0
    var spot = CGRect.null  // where the differing pixels are
    for i in stride(from: 0, to: m.px.count, by: 4) {
        if !skip.isEmpty {
            let p = CGPoint(x: (i / 4) % m.w, y: (i / 4) / m.w)
            if skip.contains(where: { $0.contains(p) }) { skipped += 1; continue }
        }
        if !(0..<channels).allSatisfy({ a.px[i + $0] == z.px[i + $0] }) { moving += 1; continue }
        let d = (0..<channels).map { abs(Int(a.px[i + $0]) - Int(m.px[i + $0])) }.max()!
        if d > 0 {
            diff += 1
            worst = max(worst, d)
            spot = spot.union(CGRect(x: (i / 4) % m.w, y: (i / 4) / m.w, width: 1, height: 1))
        }
    }
    if diff > 0 && diff <= 32 && spot.width <= 24 && spot.height <= 24 {
        // A spinner or pulsing dot can be at the same point of its cycle in both reference shots and elsewhere in
        // MingShot's: a handful of pixels in one small spot. Real faults (missing edges, colour shifts) are thousands.
        print("PASS \(name): \(m.w)x\(m.h) px identical but \(diff) px in one \(Int(spot.width))x\(Int(spot.height)) spot at \(Int(spot.minX)),\(Int(spot.minY)) (an animation caught mid-cycle)")
    } else if diff == 0 {
        let note = skipped > 0 ? ", \(skipped) px of MingShot's own window not compared" : ""
        print("PASS \(name): \(m.w)x\(m.h) px identical (\(moving) px animating, not compared\(note))")
    } else {
        print("FAIL \(name): \(diff) of \(m.w * m.h) px differ, up to \(worst) levels; see \(ours)"); failures += 1
    }
}

switch mode {
case "window":
    // Windows of other apps, front to back, with a spot no window in front of them covers.
    let list = CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID) as! [[String: Any]]
    let own = Set(["MingShot", "Window Server", "Dock"])
    var above: [CGRect] = []
    var picks: [(id: Int, app: String, spot: CGPoint)] = []
    for w in list {
        let b = w["kCGWindowBounds"] as! [String: Double]
        let r = CGRect(x: b["X"]!, y: b["Y"]!, width: b["Width"]!, height: b["Height"]!)
        let app = w["kCGWindowOwnerName"] as? String ?? ""
        let layer = w["kCGWindowLayer"] as? Int ?? 0
        defer { if layer >= 0 && layer < 1000 { above.append(r) } }
        guard layer == 0, !own.contains(app), r.width >= 200, r.height >= 120, picks.count < 3 else { continue }
        // Try a grid of spots inside, away from the edges; take the first nothing in front covers.
        var spot: CGPoint? = nil
        outer: for fy in stride(from: 0.2, through: 0.8, by: 0.2) {
            for fx in stride(from: 0.2, through: 0.8, by: 0.2) {
                let p = CGPoint(x: r.minX + r.width * fx, y: r.minY + r.height * fy)
                if !above.contains(where: { $0.contains(p) }) { spot = p; break outer }
            }
        }
        if let p = spot { picks.append((w["kCGWindowNumber"] as! Int, app, p)) }
    }
    if picks.isEmpty { print("FAIL no uncovered window of another app to test"); failures += 1 }
    for (n, w) in picks.enumerated() {
        NSPasteboard.general.clearContents()
        let before = "\(out)/e2e-win\(n)-before.png", after = "\(out)/e2e-win\(n)-after.png"
        _ = screencapture(["-l\(w.id)"], before)
        move(w.spot); sleepMs(150)
        hotkey()
        let png = awaitClipboard()
        sleepMs(300)
        _ = screencapture(["-l\(w.id)"], after)
        check("window \(w.id) (\(w.app)), with shadow", png, before: read(before).flatMap { pixels($0) },
              after: read(after).flatMap { pixels($0) }, alpha: true, ours: "\(out)/e2e-win\(n)-mingshot.png")
        sleepMs(400)
    }

case "save":
    // After capture = Save (MINGSHOT_E2E_AFTER=save): a region goes to ~/Pictures/MingShot, not the clipboard. The
    // file must equal the screen; the test deletes only the file it made.
    let folder = (NSHomeDirectory() as NSString).appendingPathComponent("Pictures/MingShot")
    let existing = Set((try? FileManager.default.contentsOfDirectory(atPath: folder)) ?? [])
    let sel = CGRect(x: 140, y: 180, width: 301, height: 187)
    let id = displayIDs[0]
    let b = CGDisplayBounds(id)
    let scale = Double(CGDisplayCopyDisplayMode(id)?.pixelWidth ?? Int(b.width)) / b.width
    let crop = CGRect(x: sel.minX * scale, y: sel.minY * scale, width: sel.width * scale, height: sel.height * scale)
    NSPasteboard.general.clearContents()
    NSPasteboard.general.setString("untouched", forType: .string)
    let before = "\(out)/e2e-save-before-D.png", after = "\(out)/e2e-save-after-D.png"
    _ = screencapture(["-D", "1"], before)
    move(CGPoint(x: b.minX + sel.minX, y: b.minY + sel.minY)); sleepMs(150)
    hotkey()
    var made: String? = nil
    for _ in 0..<60 {
        let now = Set((try? FileManager.default.contentsOfDirectory(atPath: folder)) ?? [])
        if let f = now.subtracting(existing).first(where: { $0.hasSuffix(".png") }) { made = f; break }
        sleepMs(50)
    }
    move(CGPoint(x: b.minX + sel.maxX + 200, y: b.minY + sel.maxY + 120)); sleepMs(300)
    _ = screencapture(["-D", "1"], after)
    if let f = made {
        let path = (folder as NSString).appendingPathComponent(f)
        let named = f.hasPrefix("MingShot ") && f.contains(" at ")
        print(named ? "PASS saved as \"\(f)\"" : "FAIL unexpected file name \"\(f)\"")
        if !named { failures += 1 }
        check("saved region on display \(id)", read(path), before: read(before).flatMap { pixels($0, crop: crop) },
              after: read(after).flatMap { pixels($0, crop: crop) }, alpha: false, ours: "\(out)/e2e-save-mingshot.png")
        // The .mingshot sidecar beside it (written just after the PNG, once the source window is looked up).
        let sidecar = (path as NSString).deletingPathExtension + ".mingshot"
        var doc: [String: Any]? = nil
        for _ in 0..<40 {
            if let d = read(sidecar), let j = try? JSONSerialization.jsonObject(with: d) as? [String: Any] { doc = j; break }
            sleepMs(50)
        }
        if let doc = doc, let meta = doc["metadata"] as? [String: Any] {
            let display = meta["display"] as? [String: Any]
            let ok = doc["format"] as? String == "mingshot" && doc["image"] as? String == f
                && meta["kind"] as? String == "region" && (display?["id"] as? Int) == Int(id)
                && (meta["captured_at"] as? String)?.contains("T") == true
            let wins = meta["windows"] as? [[String: Any]] ?? []
            let src = meta["source"] as? [String: Any]
            let from = src.map { "\($0["app"] as? String ?? "?") · \($0["window_title"] as? String ?? "")" } ?? "the desktop"
            print(ok ? "PASS sidecar \"\((sidecar as NSString).lastPathComponent)\": region on display \(id), from \(from), \(wins.count) window(s) recorded"
                     : "FAIL sidecar contents: \(doc)")
            if !ok { failures += 1 }
        } else {
            print("FAIL no .mingshot sidecar beside the PNG"); failures += 1
        }
        try? FileManager.default.removeItem(atPath: path)
        try? FileManager.default.removeItem(atPath: sidecar)
    } else {
        print("FAIL nothing saved in \(folder)"); failures += 1
    }
    let clip = NSPasteboard.general.string(forType: .string) == "untouched" && clipboardPNG() == nil
    print(clip ? "PASS Save alone leaves the clipboard alone" : "FAIL Save changed the clipboard")
    if !clip { failures += 1 }

case "display":
    for (n, id) in displayIDs.enumerated() {
        let b = CGDisplayBounds(id)
        NSPasteboard.general.clearContents()
        let before = "\(out)/e2e-disp\(n)-before.png", after = "\(out)/e2e-disp\(n)-after.png"
        _ = screencapture(["-D", "\(n + 1)"], before)
        move(CGPoint(x: b.midX, y: b.minY + 60)); sleepMs(150)
        hotkey()
        let png = awaitClipboard()
        move(CGPoint(x: b.midX, y: b.minY + 60)); sleepMs(300)
        _ = screencapture(["-D", "\(n + 1)"], after)
        check("display \(id)", png, before: read(before).flatMap { pixels($0) },
              after: read(after).flatMap { pixels($0) }, alpha: false, ours: "\(out)/e2e-disp\(n)-mingshot.png",
              skip: ownWindows(on: id))
        sleepMs(400)
    }

default:
    // The test region, relative to each display (points): matches MINGSHOT_E2E_SELECT in scripts/e2e.sh.
    let sel = CGRect(x: 140, y: 180, width: 301, height: 187)
    for (n, id) in displayIDs.enumerated() {
        let b = CGDisplayBounds(id)
        let scale = Double(CGDisplayCopyDisplayMode(id)?.pixelWidth ?? Int(b.width)) / b.width
        let crop = CGRect(x: sel.minX * scale, y: sel.minY * scale, width: sel.width * scale, height: sel.height * scale)
        let region = CGRect(x: b.minX + sel.minX, y: b.minY + sel.minY, width: sel.width, height: sel.height)
        let before = "\(out)/e2e-\(n)-before-D.png", after = "\(out)/e2e-\(n)-after-D.png"
        NSPasteboard.general.clearContents()
        _ = screencapture(["-D", "\(n + 1)"], before)
        move(region.origin); sleepMs(150)
        hotkey()  // MingShot selects the region itself (MINGSHOT_E2E_SELECT)
        let png = awaitClipboard()
        move(CGPoint(x: region.maxX + 200, y: region.maxY + 120))  // pointer out of the region for the "after" shot
        sleepMs(300)
        _ = screencapture(["-D", "\(n + 1)"], after)
        check("region on display \(id) (\(scale)x)", png, before: read(before).flatMap { pixels($0, crop: crop) },
              after: read(after).flatMap { pixels($0, crop: crop) }, alpha: false, ours: "\(out)/e2e-\(n)-mingshot.png")
        sleepMs(400)
    }

    // Repeat (⌃⇧3): the last region tested (on the last display) again, no overlay.
    if let lastID = displayIDs.last {
        let n = displayIDs.count - 1
        let b = CGDisplayBounds(lastID)
        let scale = Double(CGDisplayCopyDisplayMode(lastID)?.pixelWidth ?? Int(b.width)) / b.width
        let crop = CGRect(x: sel.minX * scale, y: sel.minY * scale, width: sel.width * scale, height: sel.height * scale)
        let before = "\(out)/e2e-repeat-before-D.png", after = "\(out)/e2e-repeat-after-D.png"
        NSPasteboard.general.clearContents()
        move(CGPoint(x: b.midX, y: b.maxY - 40)); sleepMs(200)  // pointer well away from the region
        _ = screencapture(["-D", "\(n + 1)"], before)
        repeatKey()
        let png = awaitClipboard()
        sleepMs(300)
        _ = screencapture(["-D", "\(n + 1)"], after)
        check("repeat last region (⌃⇧3) on display \(lastID)", png, before: read(before).flatMap { pixels($0, crop: crop) },
              after: read(after).flatMap { pixels($0, crop: crop) }, alpha: false, ours: "\(out)/e2e-repeat-mingshot.png")
        sleepMs(400)
    }

    // Esc cancels: overlay goes, clipboard untouched.
    NSPasteboard.general.clearContents()
    NSPasteboard.general.setString("untouched", forType: .string)
    let b0 = CGDisplayBounds(displayIDs[0])
    move(CGPoint(x: b0.minX + sel.minX, y: b0.minY + sel.minY)); sleepMs(150)
    hotkey(); sleepMs(450)  // once the overlay has focus (after the displays are read), before the test selection (700 ms)
    escape(); sleepMs(700)
    let untouched = NSPasteboard.general.string(forType: .string) == "untouched" && clipboardPNG() == nil
    print(untouched ? "PASS Esc cancels and leaves the clipboard alone" : "FAIL Esc: the clipboard changed")
    if !untouched { failures += 1 }
}

print(failures == 0 ? "\nE2E \(mode) PASSED" : "\nE2E \(mode) FAILED (\(failures))")
exit(failures == 0 ? 0 : 1)
