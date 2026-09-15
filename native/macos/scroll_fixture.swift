// Disposable scrolling fixture. No library backend depends on this executable.
import AppKit

final class FlippedDocument: NSView {
    override var isFlipped: Bool { true }
}
final class ScrollActions: NSObject {
    let status: NSTextField
    init(_ status: NSTextField) { self.status = status }
    @objc func clicked(_ sender: NSButton) {
        status.stringValue = "clicked:\(sender.tag)"
    }
}
let app = NSApplication.shared
app.setActivationPolicy(.accessory)
let window = NSWindow(contentRect: NSRect(x: 250, y: 230, width: 440, height: 420),
    styleMask: [.titled, .closable], backing: .buffered, defer: false)
window.title = "Unimation scrolling fixture"
let status = NSTextField(labelWithString: "initial")
status.frame = NSRect(x: 20, y: 380, width: 400, height: 24)
status.setAccessibilityIdentifier("scroll-status")
let actions = ScrollActions(status)
let scroll = NSScrollView(frame: NSRect(x: 20, y: 20, width: 400, height: 340))
scroll.hasVerticalScroller = true
scroll.borderType = .bezelBorder
scroll.setAccessibilityIdentifier("scroll-viewport")
let document = FlippedDocument(frame: NSRect(x: 0, y: 0, width: 370, height: 1440))
for index in 0..<24 {
    let button = NSButton(title: "Row \(index)", target: actions, action: #selector(ScrollActions.clicked))
    button.frame = NSRect(x: 15, y: CGFloat(index * 60 + 10), width: 320, height: 32)
    button.tag = index
    button.setAccessibilityIdentifier("scroll-row-\(index)")
    document.addSubview(button)
}
scroll.documentView = document
window.contentView!.addSubview(scroll)
window.contentView!.addSubview(status)
window.makeKeyAndOrderFront(nil)
app.activate(ignoringOtherApps: true)
app.run()
