# Linux cursor overlay

The initial renderer uses X11 through `x11rb`. It consumes the same newline-delimited cursor commands as macOS. Rendering never sends input or moves the shared hardware pointer.

## X11 and Xorg

The renderer creates an override-redirect window with an empty Shape input region. The window cannot receive pointer events. Its visible region follows the shared rounded cursor outline and click pulse. Motion and decorative idle offsets use the shared implementations. EOF destroys the renderer window.

Desktop scope raises only the renderer's own window. Window scope validates `_NET_WM_PID`, map state, and EWMH desktop membership. It follows the target's ancestor chain to find its root-level frame, places the overlay immediately above that frame, and clips drawing to the target client rectangle. Windows above the target therefore remain above the overlay. It never raises the target window or assigns it an always-on-top property. A destroyed target, owner mismatch, or failed query hides the overlay.

The renderer polls at approximately 60 Hz. Its stdin queue holds at most 128 commands, and each frame processes at most 128 commands and X events. Target motion rebases existing visual state before new absolute coordinates are applied. Window-manager changes and compositor presentation are asynchronous; this is not an atomic attachment guarantee. Workspace changes that unmap a target hide it, and `_NET_WM_DESKTOP` rejects inactive workspaces even when a compositor keeps them mapped. Missing EWMH workspace metadata hides a window-scoped overlay because map state alone cannot prove workspace membership. Compositor overview effects are not reliably observable through core X11; this remains a native validation item.

Only the selected X screen is rendered; separate X screens need separate renderer instances. Coordinates use X11 root pixels, matching XTEST coordinates, with downward Y. They are not automatically converted from screenshot pixels. The caller must apply its capture mapping first. `appearance.scale` controls glyph sizing. Per-monitor DPI adaptation, anti-aliasing, shadows, shaped target clipping, and compositor-specific overview hooks remain unfinished. The current glyph uses a binary Shape mask, which also works without a compositing manager. A reparenting window manager and `_NET_WM_PID` are supported; targets without owner metadata are rejected rather than guessed.

## Wayland

This renderer refuses sessions identified by `WAYLAND_DISPLAY` or `XDG_SESSION_TYPE=wayland`, including sessions with an XWayland `DISPLAY`. XWayland cannot provide the native compositor's complete window ordering or a general foreign-window attachment mechanism.

A native provider needs its own capability contract. Layer-shell can place a surface on an output layer, but does not attach it to arbitrary application windows or reproduce their occlusion. GNOME does not generally expose layer-shell. A compositor extension, GNOME Shell extension, KWin script/effect, or an application-owned subsurface could supply narrower guarantees. No layer-shell support is claimed in this implementation. The automation portal backend and overlay presentation are separate capabilities.

## Run on a Linux machine

Build `unimation-overlay` on an X11 desktop and pipe the standard commands to stdin. Keep stdin open while observing the cursor.

```json
{"op":"scope","scope":{"kind":"desktop"}}
{"op":"move","x":200,"y":200,"duration_ms":500}
{"op":"click","x":200,"y":200}
{"op":"scope","scope":{"kind":"window","window_id":12345,"pid":1234}}
{"op":"move","x":250,"y":250,"duration_ms":500}
{"op":"quit"}
```

Replace the example window and process IDs with discovery results. Test click-through, overlapping windows, moving/resizing the target, minimizing/restoring it, workspace changes, target termination, multi-monitor movement, and reduced motion. Confirm that no window activation or keyboard focus changes occur. No native Linux validation has been performed on the macOS development host.

## Protocol references

The [X Shape 1.1 specification](https://www.x.org/releases/X11R7.7/doc/xextproto/shape.html) defines separate input and bounding regions. [EWMH application properties](https://specifications.freedesktop.org/wm-spec/latest/ar01s05.html) define owner and desktop metadata. These are runtime requirements and hints, not a promise that every window manager implements the same presentation behavior.
