# Linux baseline and validation

This backend has been cross-checked for `x86_64-unknown-linux-gnu` on a macOS host. No Linux desktop, AT-SPI service, X server, compositor, or application was available for live validation. Successful compilation does not establish runtime support.

## Checks completed on the development host

- `cargo clippy -p cli --no-default-features --target x86_64-unknown-linux-gnu --all-targets -- -D warnings` passed.
- `cargo check -p linux --tests --target x86_64-unknown-linux-gnu` passed.
- Eight input-validation, pixel-decoding, state-projection, environment-routing, and provider-injection tests passed in a temporary host-compatible copy of this crate. That copy removed only the crate-level Linux cfg and moved the same dependencies to host scope. Those tests opened no display or D-Bus connections and did not validate native Linux behavior.

## Provider boundaries

`LinuxSession::new()` is lazy. It can report capabilities without opening a display or D-Bus connection. `Accessibility` owns an AT-SPI D-Bus connection independently of the desktop. `X11` owns a pure-Rust `x11rb::RustConnection`, with no xdotool, xclip, libX11, or XCB binary dependency.

`LinuxSession::with_providers` accepts `Box<dyn AccessibilityProvider>` and `Box<dyn DesktopProvider>`. These contracts combine the shared observation, semantic, pointer, and keyboard traits. A portal or compositor provider can replace desktop discovery, capture, and input without changing the session commands. Its descriptor is reported directly; an injected provider is not labeled X11. The concrete providers also implement the shared capability traits so libraries can compose them without using the session protocol.

Default routing distinguishes X11, Wayland, and headless/unknown environments. The environment is a hint, not proof of compositor permissions. A Wayland session with `DISPLAY` still does not select X11 automatically. An embedder can explicitly use `X11::connect(Some(":1"))` and inject it to control that server. This scope may be Xorg, Xvfb, Xnest, Xephyr, or XWayland. It never promises access to native Wayland windows outside that server.

## Implemented operations

| Route | Operations | Limits |
| --- | --- | --- |
| AT-SPI D-Bus | Application discovery, PID snapshots, advertised actions, replacing editable text | Selected attributes only; apps must expose accessibility |
| X11/EWMH | Windows, client stacking order, native window IDs, client-reported PIDs, mapped state, desktop numbers, reported active PID | WM properties may be absent; root-child fallback is identified |
| X11/RandR | Monitor bounds and physical sizes | Requires RandR monitor support; no guessed DPI transform |
| X11/GetImage | Root desktop PNG and `FrameMapping` | TrueColor 16/24/32-bit layouts; cursor excluded; 256 MiB combined pixel budget |
| X11/XTEST | Global move, click, drag, native key chord, explicit wheel detents | Shared cursor/focus; process delivery and Unicode text are not implemented |
| Shared session | Snapshot retention, diff, cached inspect | Latest 16 snapshots; inspect identifies cached evidence |

X11 window records remain separate from local AT-SPI applications. `_NET_WM_PID` is client-reported and may belong to a remote machine; matching numbers do not establish identity. Discovery therefore leaves `active_pid` unknown and requires an explicit PID for snapshots. `window_context.reported_active_pid` is diagnostic evidence only.

The protocol uses `snapshot`/`observe`, `semantic`, `pointer`, `key`, `text`, `capture`, `windows`, `displays`, `discover`, `capabilities`, `diff`, `inspect`, and `snapshots`. Unsupported routes return an error. Portable pixel scrolling is not silently converted into wheel ticks. The `x11_wheel` extension explicitly takes detents; positive vertical scrolls down, positive horizontal scrolls right.

Example requests for a persistent `unimation session --json` connection:

```json
{"op":"capabilities"}
{"op":"discover"}
{"op":"snapshot","request":{"pid":1234,"max_nodes":500,"max_depth":20}}
{"op":"capture","path":"/tmp/unimation-linux.png"}
{"op":"pointer","delivery":{"kind":"global"},"action":{"kind":"move","point":{"x":300,"y":200}}}
{"op":"x11_wheel","vertical":3,"horizontal":0}
```

Replace the sample PID and inspect a current capture before sending input. Semantic requests use the full `ElementRef` returned by the same session. `SetString` with `attribute: "text"` replaces the editable target's contents; it is not global typing or an append operation.

## Evidence and failure handling

- AT-SPI references retain a unique bus owner and object path. Session IDs include time, process ID, and a local counter. References never rematch by label or tree position. The session stops allocating at 100,000 references instead of growing indefinitely.
- Actions re-read state and reject defunct/stale objects. Toolkit reuse of a path within the same bus connection remains a limitation until lifecycle event subscriptions can retire those references. A unique owner prevents retargeting a replacement application, but cannot alone prove object lifetime within that app.
- Observations preserve raw state words, native attributes, relations, action metadata, interface names, and individual read failures. Normalized role, name, bounds, and state booleans support compact rendering. `complete` remains false because the baseline reads selected properties. `traversal_complete` separately reports tree coverage.
- Traversal bounds node count, depth, queue size, and a 30-second budget checked between nodes. Individual D-Bus method calls time out after three seconds, so an in-flight node can exceed that overall budget. Cycles/repeated child references are reported as incomplete traversal.
- AT-SPI `visible`/`showing` evidence does not establish that another window is not covering a control. Its screen coordinates are labeled `atspi_screen`; X11 captures and input use `x11_root_pixels`. There is no automatic reference-center pointer click that guesses their equivalence on fractional-scale or mixed-scale desktops.
- Coordinates are checked for finite values, root bounds after rounding, and XTEST's signed 16-bit limit. Captures include the actual pixel-to-root mapping. Resizing, display hotplug, and moving a window still require a fresh capture before an agent reuses coordinates.
- Input rejects an already-held mouse button or keyboard key at preflight. It does not grab the desktop or suppress a user's input. Concurrent human activity after preflight remains possible.
- Button/key pairs attempt release even when a press acknowledgement is uncertain. Failed releases get one cleanup retry. Drag cleanup releases at the last dispatched point. Modifier cleanup releases only keys this operation attempted to press. Disconnected-server cleanup cannot be guaranteed and is reported as unknown effect.
- Drag timing uses the shared motion planner and absolute deadlines. An X server acknowledgement is `dispatched`, not evidence that a widget accepted the action or a drop succeeded.

## Linux machine acceptance

Build on the actual host with `cargo build -p cli --no-default-features`, then follow [the shared acceptance procedure](desktop-acceptance.md).

1. Run `capabilities`, `discover`, `windows`, `displays`, and desktop capture in an X11 session. Compare monitor arrangement and PNG dimensions to the actual desktop, including a second monitor and a negative-position RandR output if available.
2. Snapshot a GTK app, Qt app, and browser/Electron app by PID. Confirm names, roles, read failures, offscreen state evidence, and limits. Compare a second snapshot after scrolling and expanding controls.
3. Invoke an advertised semantic action, then verify the resulting UI. Replace text only in a disposable field. Close a target and confirm the reference fails instead of acting on a new control.
4. In a disposable app, test coordinate click, right click, wheel detents, native key chord, slider drag, text-selection drag, and native drag-and-drop separately. Capture the before/after state for each.
5. Hold a mouse button or keyboard key before an input request. Confirm rejection and no release of the user's held input. Test ordinary Num Lock/Caps Lock toggles separately from physically held keys.
6. Switch virtual desktops and resize/rearrange displays. Record EWMH/mapped-state behavior and require fresh coordinates. Mapped state alone is not a claim that a window is on the current desktop.
7. In a Wayland session with XWayland enabled, verify AT-SPI discovery/snapshots independently. Default desktop capture/input must report the missing provider rather than pretending XWayland covers the desktop.
8. Record desktop environment, compositor/X server version, scale settings, keyboard layout, target app, request, returned effect, and observed result. Do not record `dispatched` as a passed action test.

## Remaining implementations

The initial [Wayland provider](wayland.md) uses an explicit RemoteDesktop session with D-Bus Notify input. PipeWire capture and EIS are pending. The standard RemoteDesktop portal manages consent/session lifetime and granted devices, with ScreenCast/PipeWire for images. Modern portal implementations offer EIS/libei input, while older ones may expose D-Bus Notify methods. A provider must negotiate these capabilities and preserve the stream-to-input coordinate mapping. It must not mix the two input methods after opening EIS.

Compositor-specific routes can be separate providers where their protocols exist: wlroots virtual-input/screencopy protocols, KDE-specific support, and GNOME-supported portal/remote-desktop paths. Availability and permissions need testing per compositor; none is implemented here. Kernel uinput is another explicitly privileged device route, not an automatic fallback. An initial [X11 overlay](linux-overlay.md) is implemented. Wayland overlays, Unicode keyboard synthesis, window capture, lifecycle subscriptions, and provider-specific background input remain future work.

## References reviewed

- [x11rb documentation](https://docs.rs/x11rb/latest/x11rb/) describes the pure-Rust connection and opt-in XTEST/RandR extensions used here.
- [GNOME Accessible XML](https://github.com/GNOME/at-spi2-core/blob/main/xml/Accessible.xml), [Action XML](https://github.com/GNOME/at-spi2-core/blob/main/xml/Action.xml), [Component XML](https://github.com/GNOME/at-spi2-core/blob/main/xml/Component.xml), and [EditableText XML](https://github.com/GNOME/at-spi2-core/blob/main/xml/EditableText.xml) define the D-Bus signatures. [AT-SPI constants](https://github.com/GNOME/at-spi2-core/blob/main/atspi/atspi-constants.h) define state bits and their visibility semantics.
- [Odilia's Rust AT-SPI crates](https://github.com/odilia-app/atspi) are a candidate for replacing the narrow direct `zbus` adapter as subscriptions and more interfaces are added. The baseline uses the workspace-compatible blocking `zbus` connection for its small set of XML-verified calls; it avoids adding a second async runtime or native libatspi. The handwritten interface strings/state aliases are limited to this adapter. Moving to generated `atspi-proxies` interfaces is preferred as the interface set grows; direct D-Bus calls are not claimed to be superior to those crates.
- [Enigo](https://github.com/enigo-rs/enigo) separates X11, Wayland, and libei routes and labels its Wayland/libei support experimental. It is a useful input reference, not evidence of compositor-independent behavior.
- [RemoteDesktop portal](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.RemoteDesktop.html) documents consent, session lifetime, ScreenCast integration, and EIS versus Notify delivery.
