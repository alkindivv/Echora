// Drive only the explicitly named verification instance through macOS input.
import AppKit
import ApplicationServices
import Foundation

func option(_ name: String, _ fallback: String) -> String {
    CommandLine.arguments.first(where: { $0.hasPrefix("--\(name)=") })?
        .dropFirst(name.count + 3).description ?? fallback
}
let name = option("app", "")
guard !name.isEmpty else { fatalError("Pass --app= with the dedicated verification app's exact name") }
let output = URL(fileURLWithPath: option("output", "artifacts/live-native"))
let capturePath = option("capture", "")
guard !capturePath.isEmpty else { fatalError("Pass --capture= matching the app's GPUI_CAPTURE_OUTPUT on the internal disk") }
let capture = URL(fileURLWithPath: capturePath)
let prompt = option("prompt", "TOOLS_STREAM")
let interruptAt = Double(option("interrupt", "0")) ?? 0
let resume = CommandLine.arguments.contains("--resume")
let duration = Double(option("duration", "25")) ?? 25
let draftAt = Double(option("draft-at", "2.4")) ?? 2.4
guard AXIsProcessTrusted(), let app = NSWorkspace.shared.runningApplications.first(where: {
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
    let modifiers = [(CGEventFlags.maskCommand, "command down"), (.maskShift, "shift down")]
        .filter { flags.contains($0.0) }.map { $0.1 }.joined(separator: ", ")
    let process = Process()
    process.executableURL = URL(fileURLWithPath: "/usr/bin/osascript")
    process.arguments = ["-e", "on run argv\n tell application \"System Events\"\n tell process (item 1 of argv) to set frontmost to true\n key code \(code) using {\(modifiers)}\n end tell\nend run", name]
    try! process.run()
    process.waitUntilExit()
    precondition(process.terminationStatus == 0, "Native key input failed")
    Thread.sleep(forTimeInterval: 0.04)
}
func text(_ input: String) {
    let process = Process()
    process.executableURL = URL(fileURLWithPath: "/usr/bin/osascript")
    process.arguments = ["-e", "on run argv\n tell application \"System Events\"\n tell process (item 1 of argv) to set frontmost to true\n keystroke (item 2 of argv)\n end tell\nend run", name, input]
    try! process.run()
    process.waitUntilExit()
    precondition(process.terminationStatus == 0, "Native text input failed")
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
func captureTime() -> Date? {
    (try? FileManager.default.attributesOfItem(atPath: capture.path)[.modificationDate]) as? Date
}
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
        let previous = captureTime()
        key(111, [.maskCommand, .maskShift])
        for _ in 0..<40 {
            Thread.sleep(forTimeInterval: 0.025)
            let current = captureTime()
            if current != previous, current != nil { break }
        }
        let current = captureTime()
        precondition(current != previous && current != nil, "The verification app did not write a fresh screenshot")
        let target = output.appendingPathComponent(label + ".png")
        if FileManager.default.fileExists(atPath: target.path) { try! FileManager.default.removeItem(at: target) }
        try! FileManager.default.copyItem(at: capture, to: target)
    }
    try! JSONSerialization.data(withJSONObject: timeline, options: [.prettyPrinted, .sortedKeys])
        .write(to: output.appendingPathComponent("timeline.json"))
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
