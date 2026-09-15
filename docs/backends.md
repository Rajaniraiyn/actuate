# Backend implementation guide

Implement capability traits individually. `Providers<O, S, P, T>` allows separate implementations for observation, semantic actions, pointer input, and text. A capability is unavailable until a provider implements it. Empty platform crates deliberately implement no traits and never panic with `todo!()`.

Native handles remain provider-owned. Reference namespaces change when a provider restarts. Never reinterpret a foreign reference or resolve a stale reference through a similar label or position. Receipts distinguish dispatch from verified consumption. Failures after possible native mutation must report unknown effects, without automatic retry.

## macOS

The initial provider links directly through the `objc2` framework crates. It requires no Swift helper or IPC. AX handles are retained inside a thread-bound `Accessibility` instance. `QuartzInput` is separate. Process-directed Quartz events are not SkyLight events and do not establish focus preservation or event consumption.

The Swift source under `native/macos` is a disposable test app only. It is not linked, launched, or required by the library or CLI.

Next work:

- AX observer ownership and run-loop integration, destruction events, reference retirement, diff coverage.
- Typed native values, complete CF container/attributed-string/text-marker serialization, parameterized attribute queries and typed set operations.
- ScreenCaptureKit capture and display/window coordinate transforms with mapping revisions.
- Keyboard chords and owned gesture leases, cancellation cleanup and scoped execution receipts.
- SkyLight symbol discovery, authenticated targeted packets and window-local coordinates as an independent input provider.
- Native window identity, modal/owner relationships, Spaces and focus effects.
- Optional process isolation around the same traits when callers need crash containment.

## Windows

Start UIA on an owned MTA worker. Respect handler registration and removal ownership. Add UIA, MSAA/IA2 and Java Access Bridge as separate routes. Match native GUI-thread input routing and `AttachThreadInput` resource effects to the architecture. A timed-out COM call is not cancelled merely because Rust stopped awaiting it.

## Linux

Keep AT-SPI observation separate from X11, portal/libei, compositor-specific and uinput delivery. Negotiate Wayland devices and coordinate regions. No implicit switch from session-scoped delivery to global input. Handle session revocation and device removal during gestures.

## Android and iOS

Android initially uses ADB with explicit device identity and transport lifecycle. Preserve accessibility and display coordinates separately. For iOS, define deployment profiles for simulator, test runner, device services and private integrations; do not claim ordinary app permissions grant universal device control. Share the common traits, not macOS implementation assumptions.
