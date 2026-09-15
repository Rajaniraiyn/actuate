// Disposable UI for native integration tests. Never used by the library backend.
import AppKit
final class Probe: NSObject {
    let field: NSTextField
    weak var window: NSWindow?
    var dynamic: NSTextField?
    init(_ field: NSTextField) { self.field = field }
    @objc func clicked() { field.stringValue = "clicked" }
    @objc func checked(_ sender: NSButton) { field.stringValue = sender.state == .on ? "checked" : "unchecked" }
    @objc func slid(_ sender: NSSlider) { field.stringValue = "slider:\(Int(sender.doubleValue.rounded()))" }
    @objc func picked(_ sender: NSPopUpButton) { field.stringValue = sender.titleOfSelectedItem ?? "" }
    @objc func dialog() {
        guard let window else { return }
        let alert = NSAlert()
        alert.messageText = "Unimation modal test"
        alert.addButton(withTitle: "Dismiss")
        alert.beginSheetModal(for: window) { _ in self.field.stringValue = "dismissed" }
    }
    @objc func addItem() {
        guard dynamic == nil else { return }
        let item = NSTextField(labelWithString: "Dynamic item")
        item.frame = NSRect(x: 310, y: 100, width: 170, height: 24)
        item.setAccessibilityIdentifier("unimation-dynamic-item")
        window?.contentView?.addSubview(item)
        dynamic = item
    }
    @objc func removeItem() { dynamic?.removeFromSuperview(); dynamic = nil }
}
final class ScrollProbe: NSView {
    var field: NSTextField!
    override func scrollWheel(with event: NSEvent) { field.stringValue = "scrolled" }
}
final class EventProbe: NSView {
    var field: NSTextField!
    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }
    override func mouseDown(with event: NSEvent) { field.stringValue = "mouse:\(event.clickCount):\(event.modifierFlags.contains(.shift))" }
    override func mouseDragged(with event: NSEvent) { field.stringValue = "dragged" }
    override func rightMouseDown(with event: NSEvent) { field.stringValue = "right" }
    override func otherMouseDown(with event: NSEvent) { field.stringValue = "middle" }
    override func draw(_ dirtyRect: NSRect) {
        NSColor.systemTeal.setFill()
        NSBezierPath(roundedRect: bounds, xRadius: 12, yRadius: 12).fill()
        ("Input probe" as NSString).draw(at: NSPoint(x: 20, y: 70), withAttributes: [.foregroundColor:NSColor.white])
    }
}
let app = NSApplication.shared
app.setActivationPolicy(.accessory)
let window = NSWindow(contentRect: NSRect(x: 200, y: 200, width: 500, height: 420), styleMask: [.titled, .closable, .resizable], backing: .buffered, defer: false)
window.title = "Unimation test fixture"
let field = NSTextField(frame: NSRect(x: 20, y: 370, width: 460, height: 24))
field.stringValue = "initial"
field.setAccessibilityIdentifier("unimation-test-field")
let probe = Probe(field)
probe.window = window
func button(_ title: String, _ identifier: String, _ selector: Selector, _ y: CGFloat) -> NSButton {
    let button = NSButton(title: title, target: probe, action: selector)
    button.frame = NSRect(x: 20, y: y, width: 260, height: 30)
    button.setAccessibilityIdentifier(identifier)
    window.contentView!.addSubview(button)
    return button
}
let click = button("Test click", "unimation-test-button", #selector(Probe.clicked), 320)
let checkbox = NSButton(checkboxWithTitle: "Test checkbox", target: probe, action: #selector(Probe.checked))
checkbox.frame = NSRect(x: 20, y: 280, width: 260, height: 24)
checkbox.setAccessibilityIdentifier("unimation-test-checkbox")
let slider = NSSlider(value: 20, minValue: 0, maxValue: 100, target: probe, action: #selector(Probe.slid))
slider.frame = NSRect(x: 20, y: 240, width: 260, height: 24)
slider.setAccessibilityIdentifier("unimation-test-slider")
let popup = NSPopUpButton(frame: NSRect(x: 20, y: 200, width: 260, height: 28), pullsDown: false)
popup.addItems(withTitles: ["Alpha", "Beta", "Gamma"])
popup.target = probe
popup.action = #selector(Probe.picked)
popup.setAccessibilityIdentifier("unimation-test-popup")
let dialog = button("Show dialog", "unimation-test-dialog", #selector(Probe.dialog), 160)
let add = button("Add item", "unimation-add-item", #selector(Probe.addItem), 120)
let remove = button("Remove item", "unimation-remove-item", #selector(Probe.removeItem), 80)
let scroll = ScrollProbe(frame: NSRect(x: 20, y: 10, width: 260, height: 35))
scroll.field = field
scroll.setAccessibilityElement(true)
scroll.setAccessibilityRole(.scrollArea)
scroll.setAccessibilityIdentifier("unimation-test-scroll")
let events = EventProbe(frame: NSRect(x: 310, y: 160, width: 170, height: 180))
events.field = field
// Custom AX button geometry exposes coordinates; native event behavior is implemented above.
events.setAccessibilityElement(true)
events.setAccessibilityRole(.button)
events.setAccessibilityIdentifier("unimation-test-events")
for view in [field, checkbox, slider, popup, scroll, events] as [NSView] { window.contentView!.addSubview(view) }
window.makeKeyAndOrderFront(nil)
app.activate()
app.run()
