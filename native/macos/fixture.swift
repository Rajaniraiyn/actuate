// Disposable UI used by tests/macos_e2e.py. This is not a backend dependency.
import AppKit
final class Probe: NSObject {
    let field: NSTextField
    init(_ field: NSTextField) { self.field = field }
    @objc func clicked() { field.stringValue = "clicked" }
}
final class ScrollProbe: NSView {
    var field: NSTextField!
    override func scrollWheel(with event: NSEvent) { field.stringValue = "scrolled" }
}
let app = NSApplication.shared
app.setActivationPolicy(.accessory)
let window = NSWindow(contentRect: NSRect(x: 200, y: 200, width: 300, height: 160), styleMask: [.titled], backing: .buffered, defer: false)
window.title = "Unimation test fixture"
let field = NSTextField(frame: NSRect(x: 20, y: 110, width: 260, height: 24))
field.stringValue = "initial"
field.setAccessibilityIdentifier("unimation-test-field")
let probe = Probe(field)
let button = NSButton(title: "Test click", target: probe, action: #selector(Probe.clicked))
button.frame = NSRect(x: 20, y: 60, width: 260, height: 30)
button.setAccessibilityIdentifier("unimation-test-button")
let scroll = ScrollProbe(frame: NSRect(x: 20, y: 10, width: 260, height: 35))
scroll.field = field
scroll.setAccessibilityElement(true)
scroll.setAccessibilityRole(.scrollArea)
scroll.setAccessibilityIdentifier("unimation-test-scroll")
window.contentView!.addSubview(field)
window.contentView!.addSubview(button)
window.contentView!.addSubview(scroll)
window.makeKeyAndOrderFront(nil)
app.activate()
app.run()
