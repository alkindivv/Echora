// Drive a web link's hover underline and preview card in the named
// verification instance through macOS input, capturing each state with the
// app's own Cmd+Shift+F12 screenshot.
//
//   scripts/run_native_verification.sh links --app="GPUI Capture (…)" \
//     --capture=/abs/frame.png --link=x,y [--away=x,y] [--scroll=px] [--output=dir]
//
// `--link` and `--away` are window points measured from the window's top-left
// corner (the coordinates of a capture divided by its DPR). `--scroll` first
// wheels the conversation under `--away` by that many points (positive
// scrolls back towards older content).
import AppKit
import ApplicationServices
import Foundation

func option(_ name: String, _ fallback: String) -> String {
    CommandLine.arguments.first(where: { $0.hasPrefix("--\(name)=") })?
        .dropFirst(name.count + 3).description ?? fallback
}
func pointOption(_ name: String, _ fallback: String) -> CGPoint {
    let parts = option(name, fallback).split(separator: ",").compactMap { Double($0) }
    precondition(parts.count == 2, "Pass --\(name)=x,y")
    return CGPoint(x: parts[0], y: parts[1])
}
let accessibility = AXIsProcessTrusted()
let postEvents = CGPreflightPostEventAccess()
if CommandLine.arguments.contains("--check-permissions") {
    let data = try JSONSerialization.data(withJSONObject: ["accessibility": accessibility, "postEvents": postEvents, "screenRecordingRequired": false, "appleEventsRequired": false], options: [.sortedKeys])
    print(String(data: data, encoding: .utf8)!)
    exit(accessibility && postEvents ? 0 : 2)
}
guard accessibility && postEvents else {
    fputs("Accessibility and input access are missing for the app running this driver. Grant them once in System Settings; this driver never requests a permission prompt.\n", stderr)
    exit(2)
}
let name = option("app", "")
guard !name.isEmpty else { fatalError("Pass --app= with the dedicated verification app's exact name") }
let output = URL(fileURLWithPath: option("output", "artifacts/link-preview-native"))
let capturePath = option("capture", "")
guard capturePath.hasPrefix("/") else { fatalError("Pass --capture=/absolute/frame.png matching the app's GPUI_CAPTURE_OUTPUT on the internal disk") }
let capture = URL(fileURLWithPath: capturePath)
let link = pointOption("link", "")
let away = pointOption("away", "40,400")
let scroll = Int32(option("scroll", "0")) ?? 0
let requestedPid = Int32(option("pid", "0")) ?? 0
guard let app = NSWorkspace.shared.runningApplications.first(where: {
    $0.localizedName == name && !$0.isTerminated && (requestedPid == 0 || $0.processIdentifier == requestedPid)
}) else {
    fatalError("Start the named verification app first")
}
let pid = app.processIdentifier
let root = AXUIElementCreateApplication(pid)
try FileManager.default.createDirectory(at: output, withIntermediateDirectories: true)

func value(_ element: AXUIElement, _ key: String) -> CFTypeRef? {
    var result: CFTypeRef?
    AXUIElementCopyAttributeValue(element, key as CFString, &result)
    return result
}
func windowOrigin() -> CGPoint {
    let windows = value(root, kAXWindowsAttribute) as? [AXUIElement] ?? []
    guard let window = windows.first, let position = value(window, kAXPositionAttribute) else {
        fatalError("The verification app has no window")
    }
    var origin = CGPoint.zero
    AXValueGetValue(position as! AXValue, .cgPoint, &origin)
    return origin
}
func raiseWindow() {
    app.activate(options: [.activateAllWindows])
    AXUIElementSetAttributeValue(root, kAXFrontmostAttribute as CFString, kCFBooleanTrue)
    if let window = (value(root, kAXWindowsAttribute) as? [AXUIElement])?.first {
        AXUIElementPerformAction(window, kAXRaiseAction as CFString)
    }
}
func key(_ code: CGKeyCode, _ flags: CGEventFlags = []) {
    raiseWindow()
    Thread.sleep(forTimeInterval: 0.1)
    for down in [true, false] {
        let event = CGEvent(keyboardEventSource: nil, virtualKey: code, keyDown: down)!
        event.flags = flags
        event.timestamp = DispatchTime.now().uptimeNanoseconds
        event.postToPid(pid)
        Thread.sleep(forTimeInterval: 0.08)
    }
}
func move(_ point: CGPoint) {
    let origin = windowOrigin()
    let screen = CGPoint(x: origin.x + point.x, y: origin.y + point.y)
    let event = CGEvent(mouseEventSource: nil, mouseType: .mouseMoved, mouseCursorPosition: screen, mouseButton: .left)!
    event.flags = []
    event.timestamp = DispatchTime.now().uptimeNanoseconds
    event.post(tap: .cghidEventTap)
    Thread.sleep(forTimeInterval: 0.04)
}
var timeline: [[String: Any]] = []
func record(_ label: String) {
    try? FileManager.default.removeItem(at: capture)
    let metadata = URL(fileURLWithPath: capture.path + ".render.json")
    try? FileManager.default.removeItem(at: metadata)
    key(111, [.maskCommand, .maskShift])
    for _ in 0..<200 {
        if FileManager.default.fileExists(atPath: capture.path) && FileManager.default.fileExists(atPath: metadata.path) { break }
        Thread.sleep(forTimeInterval: 0.05)
    }
    guard FileManager.default.fileExists(atPath: capture.path), FileManager.default.fileExists(atPath: metadata.path) else {
        fputs("The app did not write a fresh capture for \(label); verify its capture path and keyboard focus.\n", stderr)
        exit(2)
    }
    let data = try! Data(contentsOf: capture)
    precondition(NSBitmapImageRep(data: data) != nil, "The verification app did not write a fresh PNG")
    try! data.write(to: output.appendingPathComponent(label + ".png"))
    try! Data(contentsOf: metadata).write(to: output.appendingPathComponent(label + ".png.render.json"))
    let origin = windowOrigin()
    let pointer = CGEvent(source: nil)!.location
    timeline.append(["name": label, "time": Date().timeIntervalSince1970,
                     "windowOrigin": [origin.x, origin.y], "pointer": [pointer.x, pointer.y],
                     "frontmostPid": NSWorkspace.shared.frontmostApplication?.processIdentifier ?? 0,
                     "targetPid": pid])
    try! JSONSerialization.data(withJSONObject: timeline, options: [.prettyPrinted, .sortedKeys])
        .write(to: output.appendingPathComponent("timeline.json"))
}

raiseWindow()
Thread.sleep(forTimeInterval: 0.5)
move(away)
Thread.sleep(forTimeInterval: 0.5)
if scroll != 0 {
    var remaining = scroll
    while remaining != 0 {
        let step = max(-40, min(40, remaining))
        let event = CGEvent(scrollWheelEvent2Source: nil, units: .pixel, wheelCount: 1, wheel1: step, wheel2: 0, wheel3: 0)!
        let origin = windowOrigin()
        event.location = CGPoint(x: origin.x + away.x, y: origin.y + away.y)
        event.timestamp = DispatchTime.now().uptimeNanoseconds
        event.post(tap: .cgSessionEventTap)
        remaining -= step
        Thread.sleep(forTimeInterval: 0.02)
    }
    Thread.sleep(forTimeInterval: 0.8)
}
record("00-default")
move(link)
Thread.sleep(forTimeInterval: 0.3)
// Inside the 0.7 s delay: underline only.
record("01-hover-underline")
Thread.sleep(forTimeInterval: 3.5)
record("02-hover-card")
// Down into the card: it stays open.
move(CGPoint(x: link.x, y: link.y + 30))
Thread.sleep(forTimeInterval: 0.6)
record("03-card-hovered")
move(away)
Thread.sleep(forTimeInterval: 0.5)
record("04-closed")
move(link)
Thread.sleep(forTimeInterval: 1.2)
key(53)
Thread.sleep(forTimeInterval: 0.3)
record("05-escape")
move(away)
let defaultFrame = try Data(contentsOf: output.appendingPathComponent("00-default.png"))
let hoveredFrame = try Data(contentsOf: output.appendingPathComponent("02-hover-card.png"))
// Compare the link and card area, so a blinking composer caret cannot make
// two unchanged hover captures look like a successful interaction.
let info = try JSONSerialization.jsonObject(with: Data(contentsOf: output.appendingPathComponent("00-default.png.render.json"))) as! [String: Any]
let dpr = info["dpr"] as! Double
func previewRegion(_ data: Data) -> Data {
    let image = NSBitmapImageRep(data: data)!.cgImage!
    let region = CGRect(x: max(0, link.x - 70) * dpr, y: max(0, link.y - 20) * dpr,
                        width: 440 * dpr, height: 350 * dpr)
        .intersection(CGRect(x: 0, y: 0, width: image.width, height: image.height))
    return NSBitmapImageRep(cgImage: image.cropping(to: region)!).representation(using: .png, properties: [:])!
}
guard previewRegion(defaultFrame) != previewRegion(hoveredFrame) else {
    fputs("Hover capture is identical to the default frame; native link interaction was not verified.\n", stderr)
    exit(2)
}
print("Captured \(timeline.count) link preview states to \(output.path)")
