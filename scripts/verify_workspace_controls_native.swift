// Drive only the named verification app using its accessibility tree.
import AppKit
import ApplicationServices
import Foundation

func option(_ name: String, _ fallback: String = "") -> String {
    CommandLine.arguments.first(where: { $0.hasPrefix("--\(name)=") })?.dropFirst(name.count + 3).description ?? fallback
}
let name = option("app")
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
let output = URL(fileURLWithPath: option("output", "artifacts/issues-23-24/native"))
let capturePath = option("capture")
guard capturePath.hasPrefix("/") else {
    fputs("Pass --capture=/absolute/frame.png matching the verification app's GPUI_CAPTURE_OUTPUT on the internal disk.\n", stderr)
    exit(2)
}
let capture = URL(fileURLWithPath: capturePath)
let inventory = NSWorkspace.shared.runningApplications.map { ["name": $0.localizedName ?? "", "pid": String($0.processIdentifier)] }
guard !name.isEmpty, let app = NSWorkspace.shared.runningApplications.first(where: { $0.localizedName == name }) else { fatalError("Named verification app missing") }
try FileManager.default.createDirectory(at: output, withIntermediateDirectories: true)
try JSONSerialization.data(withJSONObject: inventory, options: [.prettyPrinted]).write(to: output.appendingPathComponent("applications.json"))
let root = AXUIElementCreateApplication(app.processIdentifier)
func value(_ e: AXUIElement, _ key: String) -> CFTypeRef? { var result: CFTypeRef?; AXUIElementCopyAttributeValue(e, key as CFString, &result); return result }
func elements() -> [AXUIElement] {
    func descendants(_ e: AXUIElement) -> [AXUIElement] {
        (value(e, kAXChildrenAttribute) as? [AXUIElement] ?? []).flatMap { [$0] + descendants($0) }
    }
    return (value(root, kAXWindowsAttribute) as? [AXUIElement] ?? []).flatMap { [$0] + descendants($0) }
}
func title(_ e: AXUIElement) -> String { value(e, kAXTitleAttribute) as? String ?? value(e, kAXDescriptionAttribute) as? String ?? "" }
func find(_ label: String) -> AXUIElement {
    for _ in 0..<50 {
        if let e = elements().first(where: { title($0) == label || value($0, kAXIdentifierAttribute) as? String == label }) { return e }
        Thread.sleep(forTimeInterval: 0.05)
    }
    record("failure")
    fatalError("Missing accessibility control: \(label)")
}
func rect(_ e: AXUIElement) -> CGRect {
    var point = CGPoint.zero; var size = CGSize.zero
    if let v = value(e, kAXPositionAttribute) { AXValueGetValue(v as! AXValue, .cgPoint, &point) }
    if let v = value(e, kAXSizeAttribute) { AXValueGetValue(v as! AXValue, .cgSize, &size) }
    return CGRect(origin: point, size: size)
}
func key(_ code: Int, command: Bool = false, shift: Bool = false) {
    app.activate(options: [.activateAllWindows])
    Thread.sleep(forTimeInterval: 0.2)
    var flags: CGEventFlags = []
    if command { flags.insert(.maskCommand) }
    if shift { flags.insert(.maskShift) }
    for down in [true, false] {
        let event = CGEvent(keyboardEventSource: nil, virtualKey: CGKeyCode(code), keyDown: down)!
        event.flags = flags
        event.postToPid(app.processIdentifier)
        Thread.sleep(forTimeInterval: 0.08)
    }
    Thread.sleep(forTimeInterval: 0.2)
}
func text(_ text: String) {
    // Paste committed text so a composition-based input source cannot retain
    // the fixture text as an unfinished IME candidate.
    let pasteboard = NSPasteboard.general
    let saved = (pasteboard.pasteboardItems ?? []).map { item -> NSPasteboardItem in
        let copy = NSPasteboardItem()
        for type in item.types { if let data = item.data(forType: type) { copy.setData(data, forType: type) } }
        return copy
    }
    pasteboard.clearContents(); pasteboard.setString(text, forType: .string)
    key(9, command: true)
    pasteboard.clearContents(); pasteboard.writeObjects(saved)
    Thread.sleep(forTimeInterval: 0.2)
}
func click(_ e: AXUIElement, edge: Int = 0) {
    app.activate(options: [.activateAllWindows])
    Thread.sleep(forTimeInterval: 0.1)
    let r = rect(e); let p = CGPoint(x: edge < 0 ? r.minX + 2 : edge > 0 ? r.maxX - 2 : r.midX, y: r.midY)
    for type in [CGEventType.mouseMoved, .leftMouseDown, .leftMouseUp] {
        let event = CGEvent(mouseEventSource: nil, mouseType: type, mouseCursorPosition: p, mouseButton: .left)!
        event.flags = []
        event.post(tap: .cghidEventTap)
        Thread.sleep(forTimeInterval: 0.06)
    }
    Thread.sleep(forTimeInterval: 0.3)
}
var report: [[String: Any]] = []
func record(_ label: String) {
    let target = output.appendingPathComponent(label + ".png")
    // The app captures its own renderer, which needs no Screen Recording grant.
    try? FileManager.default.removeItem(at: capture)
    let metadata = URL(fileURLWithPath: capture.path + ".render.json")
    try? FileManager.default.removeItem(at: metadata)
    key(111, command: true, shift: true)
    for _ in 0..<200 {
        if FileManager.default.fileExists(atPath: capture.path) && FileManager.default.fileExists(atPath: metadata.path) { break }
        Thread.sleep(forTimeInterval: 0.05)
    }
    let data = try! Data(contentsOf: capture)
    precondition(NSBitmapImageRep(data: data) != nil, "App renderer did not write a fresh PNG")
    try! data.write(to: target)
    try! Data(contentsOf: metadata).write(to: output.appendingPathComponent(label + ".png.render.json"))
    let controls = elements().map { e -> [String: Any] in
        let r = rect(e)
        return ["role": value(e, kAXRoleAttribute) as? String ?? "", "title":title(e), "id":value(e, kAXIdentifierAttribute) as? String ?? "", "value":value(e, kAXValueAttribute) as? String ?? "", "rect":[r.minX,r.minY,r.width,r.height]]
    }
    report.append(["name":label,"controls":controls])
    try! JSONSerialization.data(withJSONObject: report, options: [.prettyPrinted, .sortedKeys]).write(to: output.appendingPathComponent("report.json"))
}
if option("scenario") == "capture" {
    record("00-capture")
    print("Verified app-owned capture without Screen Recording or Apple Events")
    exit(0)
}
func waitForBranches() {
    for _ in 0..<300 {
        let controls = elements()
        if !controls.contains(where: { title($0) == "Loading branches…" }) && controls.contains(where: { value($0, kAXRoleAttribute) as? String == "AXMenuItem" && title($0) != "Create and checkout new branch…" }) { return }
        Thread.sleep(forTimeInterval: 0.1)
    }
    record("failure")
    fatalError("Branch list did not finish loading")
}
func scrollBranches() {
    let menuRect = rect(find("Branches")); let p = CGPoint(x: menuRect.midX, y: menuRect.midY)
    CGEvent(mouseEventSource: nil, mouseType: .mouseMoved, mouseCursorPosition: p, mouseButton: .left)!.post(tap: .cghidEventTap)
    Thread.sleep(forTimeInterval: 0.2)
    for _ in 0..<45 {
        let event = CGEvent(scrollWheelEvent2Source: CGEventSource(stateID: .hidSystemState), units: .line, wheelCount: 1, wheel1: -10, wheel2: 0, wheel3: 0)!
        event.location = p
        // Quartz creates wheel events with timestamp zero; AppKit drops those.
        event.timestamp = DispatchTime.now().uptimeNanoseconds
        event.post(tap: .cgSessionEventTap)
        Thread.sleep(forTimeInterval: 0.03)
    }
    Thread.sleep(forTimeInterval: 0.3)
    record("09-scrolled")
    if !option("repository").isEmpty {
        precondition(menuRect.intersects(rect(find("topic/long-acceptance-branch-149"))), "Last branch was not reachable by scrolling")
    }
}
if option("scenario") == "scroll" {
    scrollBranches()
    exit(0)
}
app.activate(options: [.activateAllWindows]); Thread.sleep(forTimeInterval: 0.5)
key(53)
click(find("Start new chat in " + option("project", "Echora")))
Thread.sleep(forTimeInterval: 0.8)
_ = find("composer-branch")
record("00-initial")
click(find("composer-location"), edge: -1)
_ = find("New local worktree")
record("01-location-icon-edge")
key(53)
Thread.sleep(forTimeInterval: 0.3)
click(find("composer-location"), edge: 1)
_ = find("New local worktree")
record("02-location-padding-edge")
click(find("New local worktree"))
precondition(rect(find("composer-location")).width > 120, "Worktree mode was not selected")
record("03-worktree-selected")
click(find("composer-branch"))
waitForBranches()
record("04-worktree-base")
key(53)
click(find("composer-location")); click(find("Local"))
record("05-local-restored")
click(find("composer-branch"), edge: -1)
_ = find("Search branches")
waitForBranches()
record("06-branches")
text("no-such-native-acceptance-branch")
record("07-empty-search")
key(0, command: true); key(51)
precondition(value(find("Search branches"), kAXValueAttribute) as? String == "", "Search was not cleared")
record("08-search-cleared")
scrollBranches()
click(find("Create and checkout new branch…"))
_ = find("Branch name")
record("10-create-branch")
key(53)
precondition(!elements().contains { title($0) == "Branch name" }, "Escape did not close the dialog")
record("11-create-cancelled")
click(find("composer-branch"), edge: 1)
record("12-branch-padding-edge")
key(53)
click(find("composer-branch")); waitForBranches()
click(find("Create and checkout new branch…")); _ = find("Branch name")
click(find("Close dialog"))
precondition(!elements().contains { title($0) == "Branch name" }, "Close button did not dismiss the dialog")
click(find("composer-branch")); waitForBranches()
click(find("Create and checkout new branch…")); click(find("Set prefix"))
_ = find("Branch prefix")
record("12a-prefix-settings")
click(find("Back to app")); _ = find("composer-branch")
if !option("repository").isEmpty {
    let repository = option("repository")
    func git(_ args: [String]) -> String {
        let process = Process(); let pipe = Pipe()
        process.executableURL = URL(fileURLWithPath: "/Library/Developer/CommandLineTools/usr/bin/git")
        process.arguments = ["-C", repository] + args; process.standardOutput = pipe
        try! process.run(); let data = pipe.fileHandleForReading.readDataToEndOfFile(); process.waitUntilExit()
        precondition(process.terminationStatus == 0)
        return String(data: data, encoding: .utf8)!.trimmingCharacters(in: .whitespacesAndNewlines)
    }
    func waitFor(_ message: String, _ condition: () -> Bool) {
        for _ in 0..<300 { if condition() { return }; Thread.sleep(forTimeInterval: 0.1) }
        fatalError(message)
    }
    func switchTo(_ branch: String, pointer: Bool = false) {
        click(find("composer-branch")); _ = find("Search branches"); text(branch)
        precondition(value(find("Search branches"), kAXValueAttribute) as? String == branch, "Branch search text differs")
        let row = find(branch)
        if pointer { click(row) } else { key(125); key(36) }
        waitFor("Git did not switch to " + branch) { git(["branch", "--show-current"]) == branch }
    }
    switchTo("topic/long-acceptance-branch-149", pointer: true)
    record("13-real-branch-switch")
    click(find("composer-branch")); _ = find("Search branches")
    click(find("Create and checkout new branch…"))
    let branch = "codex/native-acceptance-" + String(Int(Date().timeIntervalSince1970))
    _ = find("Branch name"); key(0, command: true); text(branch); key(36)
    waitFor("New branch was not checked out") { git(["branch", "--show-current"]) == branch }
    record("14-real-branch-created")
    switchTo("main")
    _ = git(["branch", "-d", branch])
    let before = git(["worktree", "list", "--porcelain"])
    let previousPaths = Set(before.components(separatedBy: "\n").filter { $0.hasPrefix("worktree ") }.map { String($0.dropFirst(9)) })
    click(find("composer-location")); click(find("New local worktree"))
    key(53)
    precondition(before == git(["worktree", "list", "--porcelain"]), "Mode selection allocated a checkout")
    click(find("Chat input")); text("!pwd > native-acceptance-cwd.txt"); key(36)
    var target = ""
    waitFor("First send did not use a new worktree") {
        for line in git(["worktree", "list", "--porcelain"]).components(separatedBy: "\n") where line.hasPrefix("worktree ") {
            let path = String(line.dropFirst(9))
            guard !previousPaths.contains(path), let contents = try? String(contentsOfFile: path + "/native-acceptance-cwd.txt", encoding: .utf8) else { continue }
            if contents.trimmingCharacters(in: .whitespacesAndNewlines) == path { target = path; return true }
        }
        return false
    }
    precondition(git(["branch", "--show-current"]) == "main")
    record("15-first-send-worktree")
    try! JSONSerialization.data(withJSONObject: ["source":repository, "worktree":target, "sourceBranch":"main", "passed":true], options: [.prettyPrinted]).write(to: output.appendingPathComponent("operations.json"))
}
print("Verified native workspace menus; \(report.count) captures in \(output.path)")
