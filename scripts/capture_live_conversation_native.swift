// Drive only the explicitly named verification instance through macOS input.
import AppKit
import ApplicationServices
import Foundation

func option(_ name: String, _ fallback: String) -> String {
    CommandLine.arguments.first(where: { $0.hasPrefix("--\(name)=") })?
        .dropFirst(name.count + 3).description ?? fallback
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
let output = URL(fileURLWithPath: option("output", "artifacts/live-native"))
let capturePath = option("capture", "")
guard capturePath.hasPrefix("/") else { fatalError("Pass --capture=/absolute/frame.png matching the app's GPUI_CAPTURE_OUTPUT on the internal disk") }
let capture = URL(fileURLWithPath: capturePath)
let prompt = option("prompt", "TOOLS_STREAM")
let interruptAt = Double(option("interrupt", "0")) ?? 0
let resume = CommandLine.arguments.contains("--resume")
let duration = Double(option("duration", "25")) ?? 25
let draftAt = Double(option("draft-at", "2.4")) ?? 2.4
guard let app = NSWorkspace.shared.runningApplications.first(where: {
    $0.localizedName == name
}) else { fatalError("Start the named verification app and enable accessibility first") }
let pid = app.processIdentifier
let root = AXUIElementCreateApplication(pid)
try FileManager.default.createDirectory(at: output, withIntermediateDirectories: true)

func value(_ element: AXUIElement, _ key: String) -> CFTypeRef? {
    var result: CFTypeRef?
    AXUIElementCopyAttributeValue(element, key as CFString, &result)
    return result
}
func elements() -> [AXUIElement] {
    func children(_ e: AXUIElement) -> [AXUIElement] {
        let rows = value(e, kAXChildrenAttribute) as? [AXUIElement] ?? []
        return rows.flatMap { [$0] + children($0) }
    }
    return (value(root, kAXChildrenAttribute) as? [AXUIElement] ?? [])
        .filter { value($0, kAXRoleAttribute) as? String == kAXWindowRole }
        .flatMap { [$0] + children($0) }
}
func button() -> AXUIElement {
    for _ in 0..<40 {
        if let found = elements().first(where: { value($0, kAXIdentifierAttribute) as? String == "composer-send" }) { return found }
        RunLoop.current.run(until: Date().addingTimeInterval(0.05))
    }
    fatalError("No composer-send accessibility target")
}
func rect(_ element: AXUIElement) -> CGRect {
    var origin = CGPoint.zero
    var size = CGSize.zero
    if let v = value(element, kAXPositionAttribute) { AXValueGetValue(v as! AXValue, .cgPoint, &origin) }
    if let v = value(element, kAXSizeAttribute) { AXValueGetValue(v as! AXValue, .cgSize, &size) }
    return CGRect(origin: origin, size: size)
}
func key(_ code: CGKeyCode, _ flags: CGEventFlags = []) {
    app.activate(options: [.activateAllWindows])
    Thread.sleep(forTimeInterval: 0.2)
    for down in [true, false] {
        let event = CGEvent(keyboardEventSource: nil, virtualKey: code, keyDown: down)!
        event.flags = flags
        event.postToPid(pid)
        Thread.sleep(forTimeInterval: 0.08)
    }
    Thread.sleep(forTimeInterval: 0.2)
}
func text(_ input: String) {
    let pasteboard = NSPasteboard.general
    let saved = (pasteboard.pasteboardItems ?? []).map { item -> NSPasteboardItem in
        let copy = NSPasteboardItem()
        for type in item.types { if let data = item.data(forType: type) { copy.setData(data, forType: type) } }
        return copy
    }
    pasteboard.clearContents()
    pasteboard.setString(input, forType: .string)
    key(9, [.maskCommand])
    pasteboard.clearContents()
    pasteboard.writeObjects(saved)
    Thread.sleep(forTimeInterval: 0.12)
}
func pointer(_ type: CGEventType, _ point: CGPoint) {
    let event = CGEvent(mouseEventSource: nil, mouseType: type, mouseCursorPosition: point, mouseButton: .left)!
    event.flags = []
    event.post(tap: .cghidEventTap)
    Thread.sleep(forTimeInterval: 0.04)
}
func click(_ point: CGPoint) {
    pointer(.mouseMoved, point)
    pointer(.leftMouseDown, point)
    pointer(.leftMouseUp, point)
}
func editor() -> AXUIElement {
    for _ in 0..<40 {
        if let found = elements().first(where: { value($0, kAXRoleAttribute) as? String == kAXTextFieldRole }) { return found }
        RunLoop.current.run(until: Date().addingTimeInterval(0.05))
    }
    fatalError("No chat editor accessibility target")
}
func focusEditor() {
    let r = rect(editor())
    click(CGPoint(x: r.minX + 20, y: r.minY + 12))
}
var timeline: [[String: Any]] = []
func record(_ label: String, screenshot: Bool = true) {
    let send = button()
    let r = rect(send)
    timeline.append([
        "name": label, "time": Date().timeIntervalSince1970,
        "button": value(send, kAXTitleAttribute) as? String ?? "",
        "rect": [r.minX, r.minY, r.width, r.height],
        "draft": value(editor(), kAXValueAttribute) as? String ?? "",
        "controls": elements().filter { value($0, kAXRoleAttribute) as? String == kAXButtonRole }
            .compactMap { value($0, kAXTitleAttribute) as? String },
    ])
    if screenshot {
        try? FileManager.default.removeItem(at: capture)
        let metadata = URL(fileURLWithPath: capture.path + ".render.json")
        try? FileManager.default.removeItem(at: metadata)
        key(111, [.maskCommand, .maskShift])
        for _ in 0..<200 {
            if FileManager.default.fileExists(atPath: capture.path) && FileManager.default.fileExists(atPath: metadata.path) { break }
            Thread.sleep(forTimeInterval: 0.05)
        }
        let data = try! Data(contentsOf: capture)
        precondition(NSBitmapImageRep(data: data) != nil, "The verification app did not write a fresh PNG")
        let target = output.appendingPathComponent(label + ".png")
        try! data.write(to: target)
        try! Data(contentsOf: metadata).write(to: output.appendingPathComponent(label + ".png.render.json"))
    }
    try! JSONSerialization.data(withJSONObject: timeline, options: [.prettyPrinted, .sortedKeys])
        .write(to: output.appendingPathComponent("timeline.json"))
}

if option("scenario", "") == "capture" {
    record("00-capture")
    print("Verified app-owned capture without Screen Recording or Apple Events")
    exit(0)
}
app.activate(options: [.activateAllWindows])
Thread.sleep(forTimeInterval: 0.4)
focusEditor()
key(0, .maskCommand)
key(51)
record("00-empty")
focusEditor()
text(prompt)
precondition(value(editor(), kAXValueAttribute) as? String == prompt, "The prompt was not entered")
record("01-ready")
let send = rect(button())
let center = CGPoint(x: send.midX, y: send.midY)
pointer(.mouseMoved, center)
record("02-hover")
pointer(.leftMouseDown, center)
record("03-pressed")
pointer(.leftMouseUp, center)
// The screenshot shortcut between down/up can cancel the native click.
if value(editor(), kAXValueAttribute) as? String == prompt { click(center) }
let started = Date()
var drafted = false
var stopped = false
var resumed = false
var i = 0
while Date().timeIntervalSince(started) < duration {
    Thread.sleep(forTimeInterval: 0.2)
    record(String(format: "live-%03d", i), screenshot: i % 5 == 0)
    let elapsed = Date().timeIntervalSince(started)
    if draftAt > 0 && elapsed > draftAt && !drafted {
        drafted = true
        focusEditor()
        text("Draft follow-up")
        record("running-with-draft")
        key(0, .maskCommand)
        key(51)
        record("running-after-clear")
    }
    if interruptAt > 0 && elapsed > interruptAt && !stopped {
        stopped = true
        let r = rect(button())
        click(CGPoint(x: r.maxX - 2, y: r.midY))
        record("stopping")
        Thread.sleep(forTimeInterval: 0.5)
        record("stopped")
        if !resume { break }
    } else if stopped && resume && !resumed {
        resumed = true
        AXUIElementPerformAction(button(), kAXPressAction as CFString)
        record("resuming")
    }
    i += 1
}
record("99-final")
print("Captured \(timeline.count) native states to \(output.path)")
