# Linux

The `linux` crate provides the host backend on Wayland and X11 sessions. Observation
and semantic actions use AT-SPI2 over D-Bus. Input, capture, window discovery and the
visual cursor are separate providers selected explicitly; nothing falls back from one
route to another. Hyprland is the first compositor adapter. Other Wayland compositors
get the protocol-level routes without window geometry, and X11 sessions get XTest.

```sh
cargo build -p cli -p overlay
target/debug/unimation discover --scope apps
target/debug/unimation observe PID --interactive --limit 80
target/debug/unimation capture /tmp/new-frame.png
target/debug/unimation session --json
```

## Providers and routes

| Capability | Provider | Route name | Delivery |
| --- | --- | --- | --- |
| Observation, inspect, attributes | `AtSpi` | `linux.atspi` | Read-only D-Bus calls with a 2 s deadline per call |
| Semantic actions | `AtSpi` | `linux.atspi.action`, `editable_text.set_text_contents`, `value.current_value`, `component.grab_focus`, `text.set_caret_offset`, `text.set_selection` | Native AT-SPI methods; no focus or pointer change unless the toolkit does it |
| Global pointer, text, keys | `WaylandInput` | `linux.wayland.virtual_pointer`, `linux.wayland.virtual_keyboard` | Compositor virtual devices. Moves the shared cursor; keys go to the focused surface |
| X11 pointer, text, keys | `X11` | `linux.x11.xtest.*` (`.xwayland_local` suffix under Xwayland) | XTest. Under Xwayland without the EI portal this stays inside the X server and never moves the compositor cursor |
| Process-directed click | `X11` | `linux.x11.send_event` | Synthetic `send_event` to one X window; many toolkits ignore it |
| Window-targeted keys | Hyprland IPC | `linux.hyprland.send_shortcut` | `hl.dsp.send_shortcut`/`send_key_state` to a window that need not be focused |
| Capture | `WaylandCapture`, `X11` | `linux.wayland.screencopy`, `screencopy_region`, `image_copy_capture`, `linux.x11.get_image` | Output, region and unoccluded toplevel frames as PNG |
| Cursor visualization | `overlay::linux::LayerCursor` | in-process or `unimation-overlay` | Layer-shell overlay surface with an empty input region |

Receipts report delivery, never consumption. `capabilities` reports which providers are
connected; providers connect lazily on first use so a session without input never
creates virtual devices.

## Observation model

Every node keeps native AT-SPI values under native names. `role` is the canonical
`AtspiRole` name from the numeric `GetRole` (`push button`, `check box`, `frame`);
`role_name` is the toolkit's own `GetRoleName` string, which GTK 4 reports as `button`
or `text box`. `states` lists every set state and `state.<name>` booleans carry
evidence: set states are always present, and a false value appears only where it is
meaningful (visual states on components with geometry, `focused` on focusable nodes,
`checked` on checkable nodes, `expanded` on expandable nodes). `interfaces` identifies
the node's D-Bus interfaces and marks the schema for presentation and queries.

Geometry: `bounds_window` is the raw window-relative `GetExtents` result. With a
compositor adapter (Hyprland), the toplevel frame from `hyprctl clients`, matched by pid
and frame title, is applied to produce `bounds` in global logical layout coordinates;
Xwayland extents are divided by the monitor scale first, and `bounds_source` records
how. Without an adapter, `bounds_screen` is read as well and copied to `bounds` with
`screen_extents_unverified`, because on Wayland it is window-relative too. Every read
path (`snapshot`, `inspect`, `click`, `actionability`) derives `bounds` the same way,
and geometry-based routes fail rather than fall back to window-local coordinates.

The X11 provider takes logical layout points and converts them with the same per-monitor
scale, so `windows` reports both logical `bounds` and raw `x_bounds`.

Text, value and action details are read when the interfaces exist: `text` (up to 4096
characters, otherwise only `text_length`), `caret_offset`, `value`, `value_min`,
`value_max`, `value_step`, `actions_detail` with localized names and key bindings.
`attribute` reads any `Interface.Property` by name, for example `Text.CaretOffset` or
`Value.CurrentValue`; `Text.Text` reads the complete text.

Toolkits that gate accessibility on the bus flag (Chromium, Electron, Qt) do not appear
until `org.a11y.Status.IsEnabled` is true. The session exposes that flag through the
`accessibility` operation. It is desktop-wide, other applications observe it, and the
library never sets it implicitly.

## Session operations

All core operations from [the session protocol](session.md) apply. Linux additions:

| Operation | Fields | Result |
| --- | --- | --- |
| `snapshot` | `request`, optional `scope: application/focused_window`, `options`, `format` | Observe and render in one request |
| `accessibility` | `action: {kind: status}` or `{kind: set_enabled, enabled}` | Bus flags, or a receipt after changing them |
| `input_route` | `route: wayland/x11` | Selects the server for global delivery |
| `hyprland_shortcut` | `target` or `address`, `mods`, `key`, optional `state: down/up` | Window-targeted key through Hyprland |
| `capture` | `source: {kind: focused_output}`, `{kind: output, name}`, `{kind: window, address}`, `{kind: region, x, y, width, height}`, `{kind: x11_window, window_id}`; `path` | Frame ID, file and mapping |
| `click_image`, `click_window`, `scroll_target` | As on macOS; `mode: global` or `process` | Receipt |
| `cursor_overlay` | `{kind: start}` in-process, `{kind: start, executable}` helper, `{kind: stop}` | Overlay lifecycle |
| `cursor_state` | None | Last dispatched pointer, compositor cursor position, overlay status |
| `displays`, `windows`, `capabilities`, `actionability` | As on macOS | Native discovery and evidence |

`click` with `mode: semantic` performs the schema's activation action: the first
advertised action named `click`, `press`, `activate`, `toggle`, `jump` or
`default.activate`, matched case-insensitively and performed with the advertised
spelling (`NativeSchema::activate_actions`). GTK 4.22 check buttons advertise no action
and reject `Component.GrabFocus` as unsupported; walk the window's focus chain with
Shift+Tab and press `space`, both through `hyprland_shortcut`, which does not change
the active window.

`Delivery.process` has no Wayland pointer or text route. Pointer clicks with a process
delivery use X11 synthetic events when the pid owns an X window; keys use
`hyprland_shortcut`; text has no process route.

## Workspace guard

Global pointer input reaches whatever the compositor shows. Reference-targeted global
input and image clicks therefore check that the target window is mapped and on its
monitor's active workspace, returning `not_on_active_workspace` with no effect
otherwise. The X11 route on Xwayland windows skips the workspace check because XTest
delivers by X window geometry inside the X server. Raw coordinate `pointer` requests
have no target and are dispatched as requested.

Hyprland has a single seat and no `ext-transient-seat` support, so there is no second
independent pointer: the virtual pointer moves the user's cursor. Routes that do not
touch the seat are AT-SPI actions, `hyprland_shortcut` and XTest under Xwayland.

## Visual cursor

`overlay::linux::LayerCursor` renders the shared cursor glyph on an overlay-layer
surface per output with an empty input region and no keyboard interactivity. It never
receives input or takes focus. Coordinates are global logical layout coordinates.
Start it with `cursor_overlay` or by setting `UNIMATION_SOFT_CURSOR=1` before
`unimation session`, which starts the in-process renderer for the whole session.

The glyph follows every targeted action, not only the compositor-seat pointer: a
semantic click, a `semantic` action, a `hyprland_shortcut` with a target and an X11
click move it to the element's center first and pulse it once the route reports
dispatch. Those routes never move the real pointer, so the soft cursor is the only
visible trace of them.
Window scope emulates the macOS window attachment on top of a layer surface, which
Wayland always stacks above every window: the glyph is clipped to the Hyprland window
rectangle, follows the window when it moves, hides when the window is unmapped, on
another workspace or under a fullscreen window, and is cut away wherever a window
stacked above the target covers it (`compositor::hyprland::stacking_above`, derived
from Hyprland's tiers and client order). Geometry refreshes on Hyprland's event socket,
so a workspace switch reveals or parks the glyph immediately, with a two-second poll as
the fallback. The same renderer backs
the standalone `unimation-overlay` executable through the JSONL protocol in the
[overlay README](../crates/overlay/README.md). Acknowledgements remain `queued`.

## Validation

`tests/linux_e2e.py` runs against `native/linux/fixture.py`, a disposable GTK 4 window
with a button, entry, check button, slider, list and text view. Launch it on the
workspace where the tests may act, without focusing it:

```sh
hyprctl dispatch 'hl.dsp.exec_cmd("UNIMATION_FIXTURE_LOG=/tmp/fixture.log python3 native/linux/fixture.py", { workspace = "3 silent" })'
hyprctl dispatch 'hl.dsp.exec_cmd("GDK_BACKEND=x11 UNIMATION_FIXTURE_ID=dev.unimation.fixture.x11 UNIMATION_FIXTURE_LOG=/tmp/fixture-x11.log python3 native/linux/fixture.py", { workspace = "3 silent" })'
UNIMATION_FIXTURE_LOG=/tmp/fixture.log UNIMATION_FIXTURE_X11_LOG=/tmp/fixture-x11.log python3 tests/linux_e2e.py
```

The script only dispatches compositor pointer input when the fixture's workspace is
active; otherwise it verifies that the guard refuses with no effect. See
[validation](validation.md#linux-2026-09-16) for the recorded results.
