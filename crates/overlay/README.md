# Agent cursor overlay

`overlay` provides a platform-independent cursor command protocol, animation interpolation and helper process controller. Its `unimation-overlay` executable currently renders on macOS using current objc2 AppKit bindings. Linux, Windows, iOS/iPadOS and Android renderer placeholders return `cursor_renderer_unavailable` without side effects. It starts hidden. It never posts input events or changes the shared cursor. Input delivery remains a separate provider.

Build with `cargo build -p overlay`. The development binary is `target/debug/unimation-overlay`; installed integrations should pass an explicit executable path.

Send one JSON object per stdin line:

```json
{"op":"configure","appearance":{"scale":1.0,"color":[0.16,0.18,0.22],"motion":"curved"}}
{"op":"move","x":400,"y":400,"duration_ms":250}
{"op":"show"}
{"op":"click","x":400,"y":400}
{"op":"hide"}
{"op":"quit"}
```

The macOS renderer interprets coordinates as Quartz desktop logical points, including negative display origins. `move` follows a bounded curved cubic path with a minimum-jerk time profile. It starts and ends at zero velocity and acceleration. The endpoint is exact. `motion: "straight"` keeps easing on a straight line; `motion: "reduced"` moves immediately and suppresses click animation. Duration defaults to 250 ms; zero moves immediately; maximum is 10000 ms. A new move replaces the pending visual motion. `click` moves immediately and draws a fading expanding ring around the tip; it does not click anything. Showing is explicit and independent of movement. EOF quits the helper. Diagnostics go to stderr. There is no frame acknowledgement protocol. The typed `CursorVisualization` implementation returns `CursorAcknowledgement::Queued` after local enqueueing, never `Rendered`. Spawn success also does not establish renderer readiness.

A main-run-loop timer animates at a requested 60 Hz while a worker reads stdin. The command queue is bounded at 256 entries. All AppKit objects remain on the main thread. This helper uses a transparent, borderless, nonactivating panel with `ignoresMouseEvents`, and an application activation policy of `Prohibited`. It neither makes the window key nor activates an application. The cursor body is roughly 16 logical points tall at scale 1, independently of the click-ring radius. Its rounded, notched arrowhead has a neutral charcoal fill, a white outline and a soft local shadow. The portable outline uses the Cua theme geometry; see [source and license](THIRD_PARTY_NOTICES.md). Scale is configurable from 0.5 to 3, and RGB components from 0 to 1. The transparent panel reserves room for the largest cursor and ripple. The midpoint of the rounded leading tip is the supplied coordinate at every scale.

Display conversion uses the main display's current Quartz height each tick. It preserves negative origins and does not assume the target lies on the main display. The panel joins Spaces and full-screen spaces where macOS permits. Space transitions and protected system screens are not validated. Each helper owns one cursor. Parent processes should close stdin when a session ends.

## Validation

Unit tests cover interpolation endpoints/midpoint, negative desktop coordinate conversion, and rejection of unknown command fields. A live show/move/pulse/hide cycle rendered a purple cursor at the requested coordinate. Foreground PID stayed 27200, and the shared pointer stayed at `(1420.046875, 811.5859375)` before and after the cycle. The helper exited successfully without stderr output. No input events were injected during this test.

## Library integration

`OverlayController` owns the platform-independent subprocess lifecycle and bounded queue. It implements `unimation::CursorVisualization` using the serialized `CursorCommand` type. `macos::overlay::OverlayController` remains a compatibility reexport. The controller can host any executable implementing the existing JSONL wire format; it does not select an input backend or inject events.

Native renderers own desktop coordinate conversion, windows and event loops. The shared `motion::sample` function supplies bounded curved motion; the original `interpolate` function remains available for compatibility. platform renderers decide when to draw each frame. A future renderer may report `Rendered` only when it can acknowledge presentation. The current stdin transport cannot do that.


## Updated renderer validation

The redesigned cursor and click ring were visually inspected on macOS at scale 1.5.
A show/move/click/hide/quit cycle exited with status 0 and no stderr output. Before
and after, foreground PID was 87424 and the shared pointer was exactly
`(699.37109375, 352.4296875)`. This validates visual-only operation for that cycle,
not every display arrangement. Unit tests cover curved-path bounds, exact endpoints,
reduced motion, invalid styling and negative desktop coordinate conversion.
Idle ticks no longer reposition an unchanged panel. The timer still requests 60 Hz;
no frame-rate or compositor-presentation guarantee is made.

The vector outline uses AppKit logical points, not screenshot pixels. AppKit rasterizes it at the destination screen backing scale. This avoids multiplying by DPI twice and keeps the apparent size consistent across Retina and 1x displays. The body was reduced independently of the ripple. Exact parity with the system pointer accessibility enlargement setting is not claimed. Mixed-DPI transitions still need live validation.
