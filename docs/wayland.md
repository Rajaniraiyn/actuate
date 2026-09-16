# Wayland portal provider

`linux::wayland::Portal` implements `DesktopProvider` independently from AT-SPI. It uses ashpd 0.13 over zbus and the XDG RemoteDesktop D-Bus Notify protocol. No helper executable, privileged input device, shell command, or automatic XWayland fallback is used.

This is a cross-compiled baseline. It has not been exercised on a Linux desktop. Portal support depends on the installed portal backend and compositor. GNOME, KDE, wlroots compositors, Weston, nested compositors, and headless sessions must be tested independently. An installed Wayland socket does not prove RemoteDesktop support.

## Start and retain a session

Run the Linux CLI's long-lived `session` command and send these requests on the same connection:

```json
{"op":"wayland_start","keyboard":true,"pointer":true,"screencast":true}
{"op":"wayland","command":{"kind":"status"}}
```

Start requests devices, optionally selects monitor streams, then opens the portal consent dialog. No consent dialog opens during discovery, capabilities, or AT-SPI observation. The provider remains connected until explicit stop, session process exit, revocation, or compositor failure. Starting a second provider in the same session is rejected instead of replacing a live connection.

`keyboard`, `pointer`, and `screencast` are explicit booleans. Returned grants can differ from the request. Each input operation checks the relevant grant. Set `screencast:false` for relative input and keyboard access without selecting a monitor. Session restore tokens and permission persistence across processes are not implemented.

D-Bus method replies use a five-second timeout. Portal consent responses can wait for the user until the dialog is answered; dismiss the dialog to cancel startup. Startup errors close a session that was already created. A `Closed` subscription is installed before Start. The provider polls it before input and status. Revoked sessions do not silently reconnect or reopen consent.

## Coordinates and input

Read the returned stream IDs and explicitly choose one:

```json
{"op":"wayland","command":{"kind":"select_stream","stream":42}}
{"op":"pointer","delivery":{"kind":"global"},"action":{"kind":"move","point":{"x":100,"y":100}}}
{"op":"wayland","command":{"kind":"relative_motion","dx":10,"dy":0}}
{"op":"wayland","command":{"kind":"axis","dx":0,"dy":20}}
{"op":"key","delivery":{"kind":"global"},"chord":{"key_code":28}}
```

Replace `42` with a granted stream ID. Absolute coordinates are logical units relative to that stream, not desktop coordinates, screenshot pixels, AT-SPI screen coordinates, or X11 root pixels. No conversion between those coordinate spaces is assumed. A stream must supply logical bounds; missing bounds or an out-of-bounds point rejects the action before dispatch. Key codes are Linux evdev codes, such as `28` for Enter. They are not X11 keycodes, Unicode, Windows virtual keys, or macOS virtual keys.

The current RemoteDesktop specification calls its coordinate extent `logical_size`, while the linked ScreenCast specification describes the `size` property as compositor-coordinate dimensions. This provider uses ashpd's `Stream::size`, which implements that documented compositor-coordinate property. It never uses PipeWire buffer dimensions. The typed stream metadata exposed by ashpd is returned in status; unknown future properties are not retained by that wrapper. The `size` metadata is an initial observation, not a live display-configuration monitor. Stop and restart after monitor hotplug, rotation, scale changes, suspend/resume, or stream replacement before reusing coordinates. Dynamic geometry invalidation is pending native implementation and testing.

Move, click, and drag use the same selected stream. Portable pixel `scroll` is rejected because the portal does not define its continuous axis values as pixels. Use the explicit `axis` extension for portal-native continuous scroll units; it acts at the current shared pointer position. Drag uses the shared `MotionPlan` with deadline-based samples. Keyboard modifiers and mouse buttons have best-effort release cleanup, including failed press calls where delivery is uncertain. Input transport errors report `effect:unknown`. Successful calls report `effect:dispatched`, never verified application success. Portal disconnection may prevent cleanup; no input transport can guarantee a release after its connection is gone.

This route affects shared desktop input. It cannot deliver background input to a PID, isolate physical input, lock the user's cursor, or verify whether an overlapping window received the action. It never calls `ConnectToEIS`; Notify and EIS transports cannot be mixed in a session. A future libei provider should own a separate negotiated transport.

## Capture and overlays

Monitor selection returns stream metadata but does not produce a screenshot. `capture` explicitly returns unsupported. The Rust `open_pipewire_remote()` method returns an owned file descriptor for a separately implemented capture provider. It does not decode frames or supply a `FrameMapping`. A future capture implementation must preserve crop, transform, scale, and stream identity, and should adopt the ScreenCast v6 `pipewire-serial` identity for PipeWire connections rather than trusting reusable node IDs.

RemoteDesktop does not enumerate arbitrary foreign windows. Use AT-SPI for accessible applications and an independently supplied compositor integration for window metadata. Stream IDs are not overlay window IDs. Overlay support is independently negotiated; input consent does not grant layer-shell, foreign-window attachment, or an always-on-top surface.

## Stop

```json
{"op":"wayland","command":{"kind":"stop"}}
```

The provider closes the portal session and releases the desktop provider slot after a successful stop. Dropping the Rust provider attempts Close as well. No temporary permissions or restore tokens are written to disk.

## Native acceptance checks

On each compositor, use an expendable test window with a text field, slider, and scroll area:

1. Run discovery before start and verify that it opens no consent prompt.
2. Reject consent and verify that the session stays usable and no input occurs.
3. Start with only pointer access and verify that keyboard input is rejected.
4. Select each returned monitor in turn. Check corners, center, and mixed-scale display coordinates against visible pointer motion.
5. Test click, wheel, slider drag, and native drag-and-drop separately; inspect resulting state, not just receipts.
6. Test Enter and modifier chords using evdev codes. Check that no modifier remains held after an error.
7. Revoke the session through the desktop sharing indicator and confirm that status/input reports closure without opening another prompt.
8. Verify stop removes the sharing indicator. Test a second explicit start.
9. Confirm that capture and process-targeted pointer input return unsupported rather than using XWayland or an unrelated screen.
10. Restart after a display configuration change. Record compositor and portal backend versions, output scale/rotation, stream metadata, action receipt, and observed result.

## References

- [RemoteDesktop specification](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.RemoteDesktop.html)
- [ScreenCast specification](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.ScreenCast.html)
- [ashpd RemoteDesktop wrapper](https://docs.rs/ashpd/0.13.13/ashpd/desktop/remote_desktop/index.html)
- [ashpd stream metadata](https://docs.rs/ashpd/0.13.13/ashpd/desktop/screencast/struct.Stream.html)
