# Windows cursor overlay

The Windows renderer is an experimental Win32 helper. It draws a cursor and click pulse using the existing line-delimited `CursorCommand` protocol. It does not inject input, move the hardware cursor, activate target applications, or change another window's styles.

## Implementation

- A `WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW` popup receives a premultiplied BGRA bitmap through `UpdateLayeredWindow`. Its hit-test handler also returns `HTTRANSPARENT`. The helper does not add a taskbar button or an Alt-Tab item.
- The raster uses the shared cursor outline, cubic motion sampler and decorative idle policy. `tiny-skia` supplies antialiased path rendering. The helper converts its premultiplied RGBA output to BGRA without premultiplying twice.
- Coordinates are physical pixels in the Windows virtual desktop, including negative monitor origins. A per-monitor-v2-aware UI thread avoids coordinate virtualization. Glyph size uses the helper window's current monitor DPI. The cursor hotspot remains at the supplied physical coordinate.
- Desktop scope is topmost. Window scope validates HWND, process ID, top-level ownership, visible/non-minimized state, DWM cloaking, frame bounds, and current virtual desktop membership. Failed checks hide the overlay. An unavailable virtual desktop manager disables window-scoped presentation.
- Window scope places only the overlay immediately above its target and below the target's preceding window. It clips pixels to the target's DWM frame bounds and follows target translations. It does not promote the target or make unrelated windows click-through.
- Virtual desktop changes may move the helper's own window to the verified target desktop, or to the current foreground window's desktop for desktop scope. They never switch the user's desktop or move a foreign window. Desktop scope remains on the creation desktop if the virtual desktop manager is unavailable.
- Native windows, bitmaps and memory DCs have RAII cleanup. The UI thread owns COM and all presentation resources. EOF and `quit` close the helper; the input thread uses a bounded command queue.

## Limits to test on Windows

There has been no live Windows validation. Cross-target checks establish API/type consistency, not compositor behavior.

Attachment refreshes approximately every 16 ms. Win32 does not give this implementation an atomic foreign-window attachment. A window can move or reorder between inspection and presentation. This is a best-effort baseline, not a guarantee of zero-frame leakage during compositor transitions. Target frame clipping is rectangular; rounded or custom-shaped windows need additional region clipping. HWND reuse by the same process cannot be distinguished using HWND and PID alone.

Task View thumbnails are not native attachment targets. There is no tested Task View transition suppression yet. Test this before relying on the overlay during overview animations. Secure desktop/UAC prompts, lock screens, exclusive fullscreen applications and protected content are outside this helper's coverage. It does not request UIAccess or bypass the secure desktop. Mixed-DPI crossings use the helper window's dominant monitor; a glyph straddling monitors uses one scale.

The command transport acknowledges queuing, not compositor presentation. Diagnostics go to stderr. Check the real screen as well as screenshots because a capture provider may omit layered windows.

## Manual acceptance

Build on Windows:

```powershell
cargo build -p overlay
.\target\debug\unimation-overlay.exe
```

Paste individual commands while keeping stdin open:

```json
{"op":"move","x":400,"y":300,"duration_ms":500}
{"op":"click","x":400,"y":300}
{"op":"configure","appearance":{"motion":"reduced","idle":{"style":"off"}}}
{"op":"hide"}
{"op":"show"}
```

Use a real top-level HWND and owning PID obtained from discovery for window scope:

```json
{"op":"scope","scope":{"kind":"window","window_id":123456,"pid":1234}}
```

Replace both example numbers. Verify:

1. Click through the visible glyph to another app. Neither the overlay nor its helper should acquire focus or capture input.
2. Partially cover the target with a different app, including an always-on-top window. The glyph must disappear under the covering window and remain visible over the uncovered target.
3. Move, resize, minimize, hide and close the target. Check target-edge clipping and stale-handle behavior. Send a scope with an intentionally wrong PID; it must remain hidden.
4. Move the target between monitors with different scaling, including a monitor left or above the primary display. Compare the glyph hotspot against the physical screenshot coordinates.
5. Switch virtual desktops, move the target to another desktop, and use Task View. Record any stray frame or independently visible preview. The helper must not switch desktops automatically.
6. Alternate desktop and window scopes. The window-scoped glyph must lose desktop-topmost behavior.
7. Quit while animating and confirm that no overlay window or held input remains.

```json
{"op":"quit"}
```

## API references

[Microsoft layered-window hit testing](https://learn.microsoft.com/en-us/windows/win32/winmsg/window-features) documents transparent layered windows and per-pixel alpha. [SetWindowPos](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwindowpos) defines relative insertion and nonactivation flags. [DWM attributes](https://learn.microsoft.com/en-us/windows/win32/api/dwmapi/ne-dwmapi-dwmwindowattribute) supply cloaking and physical frame bounds. [IVirtualDesktopManager](https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nn-shobjidl_core-ivirtualdesktopmanager) supplies desktop membership and own-window relocation. [GetDpiForWindow](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getdpiforwindow) documents DPI behavior for per-monitor-aware windows.
