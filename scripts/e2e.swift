// FILE: scripts/e2e.swift
// End-to-end check of M0 (PLAN.md §6, docs/M0.md). For each display it moves the pointer to a test region's
// corner and presses the real ⌃⇧2; MingShot runs with MINGSHOT_E2E_SELECT, so the overlay under the pointer makes
// that region's selection through the same code a mouse release runs (synthetic mouse drags don't reach WebKit on
// this Mac, in Safari either, so the drag itself is checked by hand). The PNG MingShot put on the clipboard must
// equal the same region of the Mac's own full-display screenshot (`screencapture -D`, what's on screen), taken just
// before and just after: pixels that changed in between are animation and aren't compared, every other pixel must
// be identical. `screencapture -R` (the ⌘⇧4 path) is reported for information: it differs from the screen itself
// by a few levels over glass and blur materials. Also checks that Esc cancels and leaves the clipboard alone.
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

/// (pixels that differ among those that held still between before and after, pixels that moved, largest difference)
func compare(_ before: Pixels, _ after: Pixels, _ ours: Pixels) -> (Int, Int, Int) {
    var diff = 0, moving = 0, worst = 0
    for i in stride(from: 0, to: ours.px.count, by: 4) {
        let still = (0..<3).allSatisfy { before.px[i + $0] == after.px[i + $0] }
        if !still { moving += 1; continue }
        let d = (0..<3).map { abs(Int(before.px[i + $0]) - Int(ours.px[i + $0])) }.max()!
        if d > 0 { diff += 1; worst = max(worst, d) }
    }
    return (diff, moving, worst)
}

func clipboardPNG() -> Data? { NSPasteboard.general.data(forType: .png) }
func read(_ path: String) -> Data? { try? Data(contentsOf: URL(fileURLWithPath: path)) }

let out = CommandLine.arguments.count > 1 ? CommandLine.arguments[1] : NSTemporaryDirectory()
try? FileManager.default.createDirectory(atPath: out, withIntermediateDirectories: true)

// The test region, relative to each display (points): matches MINGSHOT_E2E_SELECT in scripts/e2e.sh.
let sel = CGRect(x: 140, y: 180, width: 301, height: 187)

var ids = [CGDirectDisplayID](repeating: 0, count: 16)
var count: UInt32 = 0
CGGetActiveDisplayList(16, &ids, &count)

var failures = 0
for (n, id) in ids.prefix(Int(count)).enumerated() {
    let b = CGDisplayBounds(id)
    let scale = Double(CGDisplayCopyDisplayMode(id)?.pixelWidth ?? Int(b.width)) / b.width
    let name = "display \(id) (\(scale)x)"
    let crop = CGRect(x: sel.minX * scale, y: sel.minY * scale, width: sel.width * scale, height: sel.height * scale)
    let region = CGRect(x: b.minX + sel.minX, y: b.minY + sel.minY, width: sel.width, height: sel.height)
    let rArg = "\(Int(region.minX)),\(Int(region.minY)),\(Int(region.width)),\(Int(region.height))"
    let before = "\(out)/e2e-\(n)-before-D.png", after = "\(out)/e2e-\(n)-after-D.png"
    let beforeR = "\(out)/e2e-\(n)-before-R.png", ours = "\(out)/e2e-\(n)-mingshot.png"

    NSPasteboard.general.clearContents()
    _ = screencapture(["-D", "\(n + 1)"], before)
    _ = screencapture(["-R", rArg], beforeR)
    move(region.origin); sleepMs(150)
    hotkey()  // MingShot selects the region itself (MINGSHOT_E2E_SELECT)
    var png: Data? = nil
    for _ in 0..<40 { if let d = clipboardPNG() { png = d; break }; sleepMs(50) }
    move(CGPoint(x: region.maxX + 200, y: region.maxY + 120))  // pointer out of the region for the "after" shot
    sleepMs(300)
    _ = screencapture(["-D", "\(n + 1)"], after)

    guard let data = png else { print("FAIL \(name): nothing on the clipboard"); failures += 1; continue }
    try? data.write(to: URL(fileURLWithPath: ours))
    guard let m = pixels(data), let a = read(before).flatMap({ pixels($0, crop: crop) }),
          let z = read(after).flatMap({ pixels($0, crop: crop) }) else {
        print("FAIL \(name): couldn't decode"); failures += 1; continue
    }
    if (a.w, a.h) != (m.w, m.h) {
        print("FAIL \(name): MingShot \(m.w)x\(m.h) px, the screen's region \(a.w)x\(a.h) px"); failures += 1; continue
    }
    let (diff, moving, worst) = compare(a, z, m)
    var note = "-R not read"
    if let r = read(beforeR).flatMap({ pixels($0) }), (r.w, r.h) == (m.w, m.h) {
        let (rd, _, rw) = compare(r, r, m)
        note = rd == 0 ? "-R identical" : "-R: \(rd) px differ by up to \(rw) levels"
    }
    if diff == 0 {
        print("PASS \(name): \(m.w)x\(m.h) px identical to the screen (\(moving) px animating, not compared); \(note)")
    } else {
        print("FAIL \(name): \(diff) of \(m.w * m.h) px differ from the screen, up to \(worst) levels; see \(ours)")
        failures += 1
    }
    sleepMs(400)
}

// Esc cancels: overlay goes, clipboard untouched.
NSPasteboard.general.clearContents()
NSPasteboard.general.setString("untouched", forType: .string)
let b0 = CGDisplayBounds(ids[0])
move(CGPoint(x: b0.minX + sel.minX, y: b0.minY + sel.minY)); sleepMs(150)
hotkey(); sleepMs(150)  // before the test-mode selection (350 ms after the overlay is key)
escape(); sleepMs(700)
let untouched = NSPasteboard.general.string(forType: .string) == "untouched" && clipboardPNG() == nil
print(untouched ? "PASS Esc cancels and leaves the clipboard alone" : "FAIL Esc: the clipboard changed")
if !untouched { failures += 1 }

print(failures == 0 ? "\nE2E PASSED" : "\nE2E FAILED (\(failures))")
exit(failures == 0 ? 0 : 1)
