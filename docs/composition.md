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

`unimation ios-session` delegates to the library. The optional `unimation-ios` binary
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
is its macOS implementation. Linux, Windows, Android and iOS renderer modules currently
return explicit unavailable errors. A queued visual command does not confirm that a
frame rendered, and drawing never changes an input result.

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
