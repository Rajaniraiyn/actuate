# Provider composition and boundaries

`unimation` owns portable contracts and `Backend<O, I, C, V, A>`. Platform crates own
native bindings and connection details. The umbrella CLI selects an entry point.

| Component | Responsibility |
| --- | --- |
| `O`, observation | Scoped reads and semantic actions using the same native reference owner |
| `I`, input | Selected touch, pointer, keyboard or hardware-button capabilities |
| `C`, capture | Typed capture request and frame types |
| `V`, cursor | Visual rendering and its own acknowledgement, independent of input |
| `A`, applications | Provider-specific application identity and launch |

`Backend::new(observation).with_input(input).with_capture(capture)` consumes and returns
a backend with the new provider types. Missing components use `Unavailable`, which has
no capability implementations. Methods exist through trait bounds only when the selected
provider implements that capability. `ObservedInteraction` and `TouchKeyboard` express
useful combinations without forcing every backend to implement them. The older `Providers`
container remains available for existing integrations.

```rust
use unimation::{Backend, ObservationBudget, ObserveScope, TouchInput};

let mut backend = Backend::new(ios::SimulatorAccessibility::connect(udid, device_set)?)
    .with_input(ios::SimulatorHid::connect(udid, device_set)?);
let snapshot = backend.observe_scope(ios::SimulatorScope::Frontmost,
                                    ObservationBudget::default())?;
backend.touch(unimation::TouchAction::Tap {
    point: unimation::NormalizedPoint::new(0.5, 0.5)?,
})?;
```

This example uses native calls, not a protocol round trip. `NormalizedPoint` validates
finite values in [0,1] on construction and deserialization. Its coordinates refer to the
raw, unrotated input framebuffer. It cannot be constructed from arbitrary public fields.
A screenshot pixel or an AX point requires an explicit, validated conversion first.

The type system enforces capability combinations and some coordinate distinctions.
Permissions, changing UI state, stale native objects, OS compatibility and event consumption
still need runtime checks. A dispatch receipt alone never confirms a UI result.

## Shared session plumbing

`unimation::session::SnapshotHistory` retains the latest 32 observations behind `Arc`,
`FrameHistory` retains captures and invalidates them through `ActionEpoch` once any
mutation is dispatched, `wait_attribute` polls one native read, `session::render`
projects a snapshot for an output format, and `transport::serve` frames the JSONL
protocol (correlation ids, parse recovery, lazy text and compact projections) for every
platform session. `PointerAction::validate`, `Delivery::require_global`,
`MotionPlan::for_drag`/`walk`, `Receipt::dispatched` and `Rect::local_to_global` are the
route-independent checks every input provider shares. `NativeError::new`, `ElementRef::parse_short` and `image::png_dimensions`
replace the per-crate copies that existed before. Presentation and queries read native
attribute names through `schema::NativeSchema`, detected from the snapshot, so the AT-SPI
vocabulary renders with the same code as macOS AX without aliasing either.

## Platform-owned sessions

`ios::session::Session<B>` owns retained observations and typed request execution.
`execute(Request)` returns `Response`, including `Arc<Snapshot>` shared with its cache.
Callers can retain a snapshot without copying it or encoding it. Its full command set
uses a `SessionBackend` capability bound; simpler embeddings can use `Backend` directly.

`ios::jsonl` is a separate agent transport adapter. Serde is used for that external
boundary and the existing heterogeneous native attribute representation. There is no
JSON serialization between native providers. Neither an unsafe Rust memory dump nor an
unversioned native struct layout is an IPC protocol. A future binary adapter can share
the typed execution path without replacing JSON output for agents. JSON replies stream borrowed typed results
directly to the writer instead of copying a snapshot into another JSON value tree.

`unimation --provider apple-simulator session` delegates to the library. The optional `unimation-ios` binary
also belongs to the iOS crate; build it with `cargo build -p ios --features cli`.
Its parser uses the same workspace `usage` dependency as the umbrella CLI. It is not a
guest agent and is not needed by Rust callers. The backend does not upload an executable
into the simulator.

If a future provider requires a guest executable, it may embed a separately built,
versioned artifact with `include_bytes!`. Compression is a measurable tradeoff between
binary size, startup work and deployment requirements. It does not remove signing or
installation requirements. No helper artifact is embedded in the current direct backend.

## Native input and visual cursors

`TouchInput`, `HidKeyboard`, `HardwareButtons`, and `RelativePointerInput` are separate
contracts. `SimulatorHid` implements the first three through SimulatorKit/Indigo.
`SimulatorPointer` implements experimental native relative mouse input. It requires
explicit service activation and cleanup, and moves the guest's shared pointer if the
guest consumes its events. It is not a private per-agent cursor.

`CursorVisualization` has separate command and status types. The `overlay` library owns
the common command format, animation and child-process controller. The AppKit renderer
is its macOS implementation and the layer-shell renderer its Linux implementation; the
latter also runs in-process as `overlay::linux::LayerCursor`. Windows, Android and iOS
renderer modules currently return explicit unavailable errors. A queued visual command
does not confirm that a frame rendered, and drawing never changes an input result.

Native mouse input and a visual overlay can be selected independently. Real iPad mouse
support can display a system pointer, as described by [Apple](https://support.apple.com/en-gb/105004).
iPhone pointer-device support uses [AssistiveTouch](https://support.apple.com/en-gb/111775).
App-facing `GCMouse` APIs consume mouse events; they do not by themselves create a virtual
system-wide mouse. See Apple's [keyboard and mouse API explanation](https://developer.apple.com/videos/play/wwdc2020/10617/).

## Host boundaries and lifecycle

The macOS native crate and the current iOS Simulator host crate use one crate-root
macOS gate. Native overlay rendering has one module gate. This avoids repeating platform
conditions throughout implementations. The portable `unimation` and `overlay` contracts
remain usable outside macOS. A future guest-side iOS implementation needs its own target
boundary, distinct from the macOS Simulator host.

Temporary simulator creation and cleanup remain in test scripts. Connecting the library
to a running simulator does not acquire ownership of its lifecycle or delete its data.


## Physical iOS capture

With `ios/physical`, `PhysicalCapture` implements only `Capture`. Composing it with
`Backend::new(Unavailable).with_capture(capture)` provides capture without pretending
that observation or input exists. `PhysicalDevices` exposes async discovery/capture;
its sync adapter reuses one Tokio runtime and rejects nested runtime entry. Device
identity is explicit, and transport IDs are resolved again after reconnect.

## Evidence-based waits

`unimation::wait::wait_for_query` accepts any `ObserveScope` provider, a caller-chosen
scope, query, `Present`/`Absent` condition, timeout and observation limit. The report
retains the last full snapshot, elapsed time and observed evidence. Incomplete
coverage cannot prove absence. Synchronous native calls may exceed the deadline;
a late result is retained but is not reported as a wait completed within budget.
Native errors propagate unchanged and no input operation is retried.

macOS accepts an explicit application PID as its observation scope;
iOS Simulator uses its existing `SimulatorScope`. This wait API currently belongs
to the Rust library, not a new CLI command or a claimed notification provider.


## Shared motion and persistent connections

`unimation::motion::MotionPlan` generates timed absolute samples;
`RelativeMotionPlan` generates bounded integer HID reports with exact total counts.
Both use the same eased Bézier geometry as the overlay. Coordinate units belong to
the caller; Android mouse acceleration means HID counts are not screen pixels.
Android and simulator mouse adapters expose `move_smooth`; simulator touch exposes
`touch_path` with full normalized-coordinate validation before contact.

A live Android `Pointer` also implements `CommandTransport`. For example,
`Android::new(&mut pointer).capture_png()` opens a logical shell stream on the
same ADB connection while the HID stream remains alive. No second TLS connection
is needed. Borrowed transports allow wrappers to share a session without taking
ownership or reconnecting. The HID example demonstrates this composition.

Keep the CLI's JSONL `session` process open for an entire task. All requests in
that process reuse its selected connection; EOF tears it down. Separate one-shot
CLI processes currently create separate connections. Named-session reuse across
independent invocations is a future CLI feature, not an implicit existing daemon.
Any reconnect layer must invalidate uncertain stream state and never replay an
input operation whose effect is unknown.

Idle float is an overlay-only drawing offset. It never dispatches HID reports or
changes input targets. Reduced motion disables it. The Linux renderer shares the
same configuration and motion geometry; the Windows renderer remains a stub.


## Desktop cursor providers

`OverlayController` implements `CursorVisualization` independently of input traits.
Start the native `unimation-overlay` executable with an explicit path and inject that
controller wherever a visual provider is needed. The same typed commands configure
appearance, desktop/window scope, movement, click feedback and visibility on macOS,
Windows, Wayland and X11. A queued command is not a rendered-frame acknowledgement or
proof that input reached its target. Global input and a decorative cursor remain
separate capabilities.

Windows uses a layered click-through window and X11 an input-empty Shape window; both
track window scope by polling without raising the target, so atomic foreign-window
attachment is not guaranteed. See [Windows overlay](windows-overlay.md) and
[Linux](linux.md#visual-cursor) for ownership, workspace, clipping and rendering limits.
