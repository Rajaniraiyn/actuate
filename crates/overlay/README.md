# Agent cursor overlay

`overlay` provides a platform-independent cursor command protocol, animation interpolation and helper process controller. Its `unimation-overlay` executable renders on macOS using current objc2 AppKit bindings and on Linux through a Wayland layer-shell surface; the Linux renderer is also available in-process as `overlay::linux::LayerCursor`. Windows, iOS/iPadOS and Android renderer placeholders return `cursor_renderer_unavailable` without side effects. It starts hidden. It never posts input events or changes the shared cursor. Input delivery remains a separate provider.

Build with `cargo build -p overlay`. The development binary is `target/debug/unimation-overlay`; installed integrations should pass an explicit executable path.

Send one JSON object per stdin line:

```json
{"op":"configure","appearance":{"scale":1.0,"color":[0.16,0.18,0.22],"motion":"curved","idle":{"style":"bob","amplitude":0.65,"period_ms":2400}}}
{"op":"move","x":400,"y":400,"duration_ms":250}
{"op":"show"}
{"op":"click","x":400,"y":400}
{"op":"hide"}
{"op":"quit"}
```

The macOS renderer interprets coordinates as Quartz desktop logical points, including negative display origins. `move` follows a bounded curved cubic path with a minimum-jerk time profile. It starts and ends at zero velocity and acceleration. The endpoint is exact. `motion: "straight"` keeps easing on a straight line; `motion: "reduced"` moves immediately and suppresses click and idle animation. Duration defaults to 250 ms; zero moves immediately; maximum is 10000 ms. A new move replaces the pending visual motion. `click` moves immediately and draws a fading expanding ring around the tip; it does not click anything. Showing is explicit and independent of movement. EOF quits the helper. Diagnostics go to stderr. There is no frame acknowledgement protocol. The typed `CursorVisualization` implementation returns `CursorAcknowledgement::Queued` after local enqueueing, never `Rendered`. Spawn success also does not establish renderer readiness.

A main-run-loop timer animates at a requested 60 Hz while a worker reads stdin. The command queue is bounded at 256 entries. All AppKit objects remain on the main thread. This helper uses a transparent, borderless, nonactivating panel with `ignoresMouseEvents`, and an application activation policy of `Prohibited`. It neither makes the window key nor activates an application. The cursor body is roughly 16 logical points tall at scale 1, independently of the click-ring radius. Its rounded, notched arrowhead has a neutral charcoal fill, a white outline and a soft local shadow. The portable outline uses the Cua theme geometry; see [source and license](THIRD_PARTY_NOTICES.md). Scale is configurable from 0.5 to 3, and RGB components from 0 to 1. The transparent panel reserves room for the largest cursor and ripple. The midpoint of the rounded leading tip is the supplied coordinate at every scale while moving or clicking. During idle decoration, the glyph floats by at most 0.65 logical points by default; the semantic target and click-ring center remain fixed.

Display conversion uses the main display's current Quartz height each tick. It preserves negative origins and does not assume the target lies on the main display. Space membership depends on the explicit scope described below. Space transitions and protected system screens are not validated. Each helper owns one cursor. Parent processes should close stdin when a session ends.

## Validation

Unit tests cover interpolation endpoints/midpoint, negative desktop coordinate conversion, and rejection of unknown command fields. A live show/move/pulse/hide cycle rendered a purple cursor at the requested coordinate. Foreground PID stayed 27200, and the shared pointer stayed at `(1420.046875, 811.5859375)` before and after the cycle. The helper exited successfully without stderr output. No input events were injected during this test.

## Library integration

`OverlayController` owns the platform-independent subprocess lifecycle and bounded queue. It implements `unimation::CursorVisualization` using the serialized `CursorCommand` type. `macos::overlay::OverlayController` remains a compatibility reexport. The controller can host any executable implementing the existing JSONL wire format; it does not select an input backend or inject events.

Native renderers own desktop coordinate conversion, windows and event loops. The shared `unimation::motion::sample` function supplies bounded curved motion; there is no separate interpolation implementation. Platform renderers decide when to draw each frame. A future renderer may report `Rendered` only when it can acknowledge presentation. The current stdin transport cannot do that.


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

## Idle decoration

`appearance.idle` configures `style` (`bob` or `off`), `amplitude` (0..=2 logical points) and `period_ms` (800..=10000). Bob is enabled by default, with a 0.65-point amplitude and a 2400-ms period. It starts after 600 ms without activity and fades in over one second. It stops during movement or a click ring, when hidden, and whenever motion is reduced. Idle displacement changes only the drawn glyph. It never enters a motion plan, moves the system cursor, changes the semantic target, or repositions the panel. Other platforms accept the shared configuration but their native renderer stubs remain unavailable.

Tests cover bounded deterministic idle offsets, disabled/reduced behavior and invalid settings. A live macOS show/move/idle/click/hide cycle completed with no errors; its screenshot was inspected. The foreground PID and shared pointer position were unchanged before and after. Frame pacing has not been measured.

## Window scope and desktop scope

The portable command below binds visualization to a native window and its owner.
The native ID is a `u64`; each renderer checks its platform's narrower ID range.

```json
{"op":"scope","scope":{"kind":"window","window_id":24947,"pid":87424}}
{"op":"scope","scope":{"kind":"desktop"}}
```

Desktop scope is the default for standalone helpers. macOS sessions select window
scope automatically for SkyLight pointer input. System panels can use the same
window scope; no application-name classification selects an always-on-top cursor.
Explicit desktop scope uses a high-level panel for desktop-wide visualization.

On macOS, window scope uses optional SkyLight ordering groups and orders the
helper directly above its target at the target's level. Cross-process raises can
break that relationship on the tested OS. The renderer therefore checks current
WindowServer order and repairs it only when incorrect. Private notifications did
not reliably replace that check in the live probe. This is not an atomic native
child-window guarantee; a compositor frame can race an order change.

The cursor is clipped to the target rectangle and follows its origin in logical
points. Occluding windows remain above it. Missing targets, owner mismatches,
hidden windows and off-screen targets suppress rendering. Window scope does not
join all Spaces. It uses MoveToActiveSpace when showing a visible target again.
The panel is transient and excluded from window cycling. Dock AX notifications
hide it during Mission Control, app Exposé and Show Desktop. If overview monitoring
cannot be established, visuals remain hidden and the helper retries. Dock restart
recreates the observer. Starting inside an already-active overview still needs
validation because AX notifications do not provide an initial state snapshot.

Live validation on this host covered ordering relative to VS Code and Ghostty,
raising both apps, hiding/unhiding VS Code, moving its origin by `(50, 50)`, and
Mission Control entry/exit. The cursor panel moved by the same delta and was absent
from the on-screen window list during Mission Control. Multiple Spaces, mixed-DPI
displays, full-screen transitions and Dock restart still need live validation.
The protocol acknowledges enqueueing only; it does not report compositor state.

## Linux renderer

The Linux renderer creates one overlay-layer `zwlr_layer_shell_v1` surface per output, sized to a 96-point box that moves with layer-surface margins. Its input region is empty and keyboard interactivity is off, so it never receives pointer events or focus. Coordinates are global logical layout coordinates from `xdg_output`. The glyph is rasterized with tiny-skia from the shared outline at the output's integer buffer scale; the shadow is a layered offset fill rather than a blur. Window scope reads the target rectangle from Hyprland IPC whenever the Hyprland event socket reports a change (every 100 ms without it, every 2 s as a fallback with it), clips the glyph to it, follows moves and hides while the window is unmapped, not on its monitor's active workspace or under a fullscreen window. Overlay-layer surfaces are above every window, so the renderer emulates the window-stack attachment by cutting the glyph away under the windows Hyprland stacks above the target (`compositor::hyprland::stacking_above`). Frames are rasterized only when the quantized position, idle offset, ring progress, appearance or clip changed, and the loop polls at 100 ms while nothing animates. The standalone executable and the in-process `LayerCursor` share this renderer; both acknowledge `queued` only.
