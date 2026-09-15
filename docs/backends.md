# Backend implementation guide

Implement capability traits individually. `Providers<O, S, P, T>` allows separate implementations for observation, semantic actions, pointer input, and text. A capability is unavailable until a provider implements it. Empty platform crates deliberately implement no traits and never panic with `todo!()`.

Native handles remain provider-owned. Reference namespaces change when a provider restarts. Never reinterpret a foreign reference or resolve a stale reference through a similar label or position. Receipts distinguish dispatch from verified consumption. Failures after possible native mutation must report unknown effects, without automatic retry.

## macOS

The initial provider links directly through the `objc2` framework crates. It requires no Swift helper or IPC. AX handles are retained inside a thread-bound `Accessibility` instance. `QuartzInput`, `SkyLightInput` and both capture providers are separate. `MacSession` composes them and maintains snapshot/frame history. Process-directed Quartz events are not SkyLight events and do not establish focus preservation or event consumption.

The Swift source under `native/macos` is a disposable test app only. It is not linked, launched, or required by the library or CLI.

Implemented extensions include role/name/native-attribute queries, field-level observation diffs, parameterized attribute reads, bounded attribute polling, physical key chords, pointer buttons/counts/modifiers/drag, native window lookup and hit-testing. The default capture route uses ScreenCaptureKit still screenshots on macOS 14+; the executable route is explicit. Frame mappings include native bounds, output pixel dimensions and a geometry fingerprint. SkyLight uses dynamically loaded symbols and window-local packets. See [its contract and limits](skylight.md).

The optional `unimation-overlay` executable owns its AppKit run loop and renders a separate cursor. `MacSession` can start it from an explicit path and queue the last successful SkyLight pointer position after dispatch. It remains optional; queue and render status are separate from input receipts. See [cursor research](reference-cursor-notes.md) for upstream patterns and their limitations.

Next work:

- AX observer ownership, destruction events, reference retirement and bounded native-handle retention.
- Complete native container/attributed-string/text-marker serialization and more parameter/set types.
- Streaming ScreenCaptureKit subscriptions and display-change lifecycle handling.
- Owned gesture leases, cancellation cleanup across provider failures and scoped execution verification.
- Validated SkyLight keyboard authentication and explicit focus-without-raise capabilities.
- Modal/owner relationships, Spaces, stronger window incarnation identity and focus-effect reporting.
- Optional worker isolation for hung/crashing native calls, and richer cursor lifecycle/arrival reporting.

## Windows

Start UIA on an owned MTA worker. Respect handler registration and removal ownership. Add UIA, MSAA/IA2 and Java Access Bridge as separate routes. Match native GUI-thread input routing and `AttachThreadInput` resource effects to the architecture. A timed-out COM call is not cancelled merely because Rust stopped awaiting it.

## Linux

Keep AT-SPI observation separate from X11, portal/libei, compositor-specific and uinput delivery. Negotiate Wayland devices and coordinate regions. No implicit switch from session-scoped delivery to global input. Handle session revocation and device removal during gestures.

## Android and iOS

Android initially uses ADB with explicit device identity and transport lifecycle. Preserve accessibility and display coordinates separately. For iOS, define deployment profiles for simulator, test runner, device services and private integrations; do not claim ordinary app permissions grant universal device control. Share the common traits, not macOS implementation assumptions.


## iOS and iPadOS Simulator implementation

The `ios` crate connects an explicitly selected, booted device in an explicit device set.
It shares one implementation across iPhone and iPad. `SimulatorAccessibility` loads
CoreSimulator and AccessibilityPlatformTranslation through direct Rust/objc2 calls.
A token-routed translator delegate sends accessibility requests to the selected guest.
There is no guest app, Swift helper, XCTest installation, or host-global input fallback.

`observe_frontmost` reads the guest application's translated hierarchy and retains native
element identities. `SemanticActions` invokes advertised actions. Complete native property
enumeration, HID touch/text, physical-device transport and screenshot-to-touch mapping
remain separate work. `complete=false` explicitly marks the selected attribute coverage.
A callback response deadline is not a deadline for the whole synchronous observation.

`Simctl` separately handles installed-runtime discovery, explicit device lifecycle,
launch and PNG capture. It invokes the installed CoreSimulator executable directly;
Xcode's wrapper may run first-launch installation even for a listing command. Operations
have bounded subprocess deadlines and preserve unknown effects after possible mutation.
The library does not delete arbitrary device-set directories on drop. The creator owns
shutdown, device deletion and removal of its temporary directory.

See [iOS session usage and validation](ios.md).
