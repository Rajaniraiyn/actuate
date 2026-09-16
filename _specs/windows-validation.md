# Windows baseline and native validation

The Windows provider compiles against Microsoft's `windows` 0.62.2 bindings, imported as `windows_api` to avoid the local crate name. Windows-only code has one crate-root `cfg` boundary. Initial development used cross-compilation. See the [2026-09-17 Windows host results](windows-host-results.md) for native fixture tests, focus-stealing failures, fixes and remaining coverage gaps.

## Available providers

- `WindowDiscovery` implements `Discover` using `EnumWindows`, preserving window handles, owning PIDs, titles, visibility, minimized state, physical rectangles and read errors. Discovery includes the foreground PID. Its application names are window titles, not executable product names. It does not initialize COM.
- `Accessibility` implements `Observe` and `SemanticActions` using UI Automation's raw view. It wraps every enumerated top-level window for a process under a synthetic application root. Available patterns determine actions. WPF, WinForms, Win32, WinUI and Electron receive the same pattern-based treatment; no framework name implies complete support. UIA providers may activate their windows during semantic actions; `may_activate_target` reports this limitation.
- `GlobalInput` implements `PointerInput`, `TextInput` and `KeyboardInput<Key = KeyChord>` using `SendInput`. Routes are explicit. Process-targeted delivery returns an error before input. There is no `PostMessage`, `SendMessage`, focus-stealing or coordinate-click fallback from semantic actions.
- `DesktopCapture` implements `Capture<Request = String, Frame = serde_json::Value>`. The request is a PNG path. GDI captures the composed desktop at its native physical size. The result includes pixel-to-desktop offsets, dimensions and cursor exclusion. It does not restore minimized windows or activate targets.
- `displays()` enumerates physical monitor rectangles and work areas. Display handles are observations, not persistent hardware identifiers.

`WindowsSession` owns references and the last 16 snapshots. It accepts `capabilities`, `discover`, `windows`, `uia_roots`, `displays`, `snapshot` with alias `observe`, `inspect`, `snapshots`, `semantic`, `pointer`, `text`, `key`, `windows_wheel`, `capture` and `diff`. Actions and references use the shared core types. Unknown fields and commands fail parsing. Snapshot reference objects must come from the same persistent session.

Snapshots can include a top-level `window_id` to restrict observation to one HWND owned by the requested PID. `uia_roots` performs bounded root discovery for shell windows omitted by `EnumWindows`; it does not imply complete desktop-tree coverage. `inspect` reports an optional UIA `clickable_point`, which is not a guarantee that a point is suitable for dragging.

`cursor_overlay`, `cursor`, and `cursor_state` manage an independent native visual helper. State reports its configured tracking and physical-cursor policy, never a rendered-frame acknowledgement. `taskbar_state` and `taskbar_auto_hide` explicitly query or change shared-shell state. See [interactive results](windows-interactive-results.md) for policy examples and tested effects.

The default UIA provider initializes lazily, so `capabilities`, discovery, display enumeration and capture do not require COM. `WindowsSession::with_providers` accepts independent `WindowsAccessibility` and `WindowsInput` implementations. Their supertraits enforce observation, semantic, pointer, text and keyboard contracts. Their typed capability descriptions are included in session output. Applications can instead compose `Accessibility`, `GlobalInput`, `WindowDiscovery` and `DesktopCapture` directly with the shared traits.

## Identity, observations and input

Session namespaces include PID, a local counter and creation time. Native references use UIA runtime IDs bucketed by process PID and process creation time. `CompareElements` verifies a retained candidate, avoiding a quadratic scan. An unavailable retained object is never revived. Process replacement, unknown references and references from another session fail resolution. Automation IDs and names are attributes, never stable identity keys. Runtime ID reuse within an app still depends on the provider reporting element destruction correctly.

UIA objects stay on their creating MTA thread and make the provider non-Send/non-Sync. Embedders should dedicate a non-UI thread to the session. COM objects are released before `CoUninitialize`. UIA connection and transaction timeouts are three seconds. Traversal checks a ten-second deadline between nodes and child queries; an individual native call can extend that deadline. This is not a hard cancellation guarantee. The session retains at most 20,000 native element references and rejects further allocation instead of silently reassigning IDs.

Property failures remain node issues. They make `complete` false independently of `traversal_complete`. Node/depth limits and traversal errors are explicit. `offscreen` is UIA evidence; false does not establish lack of occlusion or permission to click. This baseline reads a useful set of common properties, not every UIA property or provider extension.

Window rectangles, UIA bounds, screenshots and pointer coordinates use **physical desktop pixels**. Thread-scoped per-monitor-v2 DPI awareness is restored after native work. Absolute mouse normalization includes negative virtual-desktop origins and aims at physical pixel centers. Points in monitor gaps are rejected. Capture pixel `(x, y)` maps to desktop `(x + bounds.x, y + bounds.y)` without scaling. Capture does not burn a cursor into the image.

Global input rejects held modifiers or mouse buttons before dispatch. It does not release the user's existing keys. A user can still change input during the gesture; there is no exclusive OS input lock. Drag deadlines use a monotonic clock. Partial dispatch returns `Effect::Unknown`, and cleanup attempts to release injected buttons/modifiers. Normal release failures propagate. Unicode text uses UTF-16 `KEYEVENTF_UNICODE`, which differs from layout-specific physical typing. Empty text has `Effect::None`. Successful input or UIA calls report `Dispatched`, never verified application success.

Portable pixel scrolling is explicitly unsupported. Win32 wheel ticks are not silently substituted for pixels. UIA `scroll_into_view` is available when the target exposes ScrollItemPattern. The separate `windows_wheel` extension accepts integer detents, bounded to -120..120 per axis. Positive vertical means down and positive horizontal means right. The provider converts vertical detents to negative Win32 wheel deltas. It requires explicit global delivery and optionally moves the shared cursor to a physical desktop point first. It never foregrounds a window. Its capability metadata declares signs and limits; injected providers default to an unsupported wheel method.

## Native acceptance run

The [interactive report](windows-interactive-results.md) also documents the
compiled Rust common-dialog fixture and six passing isolated dialog scenarios.
Use `python tests/windows_dialogs.py --isolated` for UIA-only Save/Open coverage
without operating the shared desktop. Build its example binary first as shown
in that report.

On Windows 10 or newer, use an unlocked interactive desktop. Run from the repository:

```powershell
cargo build -p cli --no-default-features
cargo test -p windows@0.1.0
$bin = ".\target\debug\actuate.exe"
& $bin capabilities --json
& $bin displays --json
& $bin windows --json
& $bin discover --json
& $bin capture desktop.png --json
& $bin session --json
```

The session consumes one JSON object per line. Start with observation and read the returned session/reference values. Replace the PID below with the application PID from discovery:

```json
{"op":"snapshot","request":{"pid":1234,"max_nodes":500,"max_depth":20}}
{"op":"snapshots"}
{"op":"capture","path":"desktop.png"}
```

An explicit wheel request in the same session is:

```json
{"op":"windows_wheel","delivery":{"kind":"global"},"vertical":3,"horizontal":0,"point":{"x":500,"y":400}}
```

The point must be chosen from the current desktop capture. This sends three downward detents, not three pixels. A zero request without a point sends no input.

Use a disposable document for input tests. Test the following matrix and record both the receipt and resulting application state:

| Case | Evidence to collect |
| --- | --- |
| Win32 or WinForms controls | Read text, invoke a button, set a ValuePattern value; identify unsupported patterns |
| WPF and WinUI | Expand/collapse, selection, scrolling into view, provider-specific missing properties |
| VS Code/Electron | Embedded document tree and actual click/drag behavior; do not assume browser DOM access |
| Persistent references | Observe twice, change unrelated UI, confirm surviving native controls keep references |
| Stale targets | Close a dialog and act on its old reference; confirm no name/position rematching |
| Multiple windows | Confirm each process window appears under the synthetic root |
| Mixed DPI monitors | Capture, map points, move window across monitors and verify actual pointer position |
| Negative origins/gaps | Test a monitor left/above primary and rejection of coordinates in empty layout gaps |
| Input interference | Hold Shift or a mouse button before a command; expect rejection without release |
| Drag | Separately test slider tracking, text selection and native drag-and-drop |
| Elevation | Try an elevated disposable app from an ordinary session; preserve rejected/unknown input without fallback |
| Unicode | Test non-ASCII text and supplementary characters in a disposable editor |
| Capture | Inspect layered windows, HDR differences and hardware/protected-content omissions |

The shared read-only smoke script in `tests/desktop_smoke.py` exercises discovery, capture, repeated snapshots and diffs. Native input tests require observing the resulting application state; a successful API return is insufficient.

## Deliberate gaps

No background mouse/keyboard route, desktop switching, WGC/DXGI capture, window-only capture, UIA subscription, virtualized-item realization, arbitrary property enumeration, MSAA/IAccessible2 or Java Access Bridge provider is claimed. The independent [overlay helper](windows-overlay.md) exists, but visible rendering and workspace transitions remain unvalidated. Locked/secure desktops, elevated targets and protected content can reject or omit operations. GDI capture may omit hardware overlays and does not promise HDR fidelity. Shared global input still changes the actual desktop cursor.

MSAA/IAccessible2, Java Access Bridge, application-specific messaging, browser accessibility and future targeted input belong in independent providers. An adapter can be selected for a framework or app when its measured capabilities justify it. It must preserve reference ownership, report its route and keep unsupported operations explicit.

## Reference decisions

- [Microsoft windows-rs](https://github.com/microsoft/windows-rs) supplies current generated COM and Win32 bindings.
- [UIA threading guidance](https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-threading) informs apartment ownership and the non-UI thread requirement.
- [CompareElements](https://learn.microsoft.com/en-us/windows/win32/api/uiautomationclient/nf-uiautomationclient-iuiautomation-compareelements) compares native identity; names and AutomationId are not substitutes.
- [SendInput](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-sendinput) documents packet counts, existing key-state interference and UIPI restrictions. A failed call does not specifically identify UIPI as its cause.
- [BitBlt](https://learn.microsoft.com/en-us/windows/win32/api/wingdi/nf-wingdi-bitblt) provides the initial composed-desktop capture route.
- [Microsoft winapp CLI](https://github.com/microsoft/winappCli/blob/main/docs/ui-automation.md) separates pattern actions from injected input and documents interactive-desktop requirements. That distinction carries into this provider; its automatic screenshot restoration/foreground fallback does not.
- [Cua Windows backend](https://github.com/trycua/cua/blob/main/libs/cua-driver/rust/crates/platform-windows/src/lib.rs) separates UIA/MSAA, message input, GDI and WGC modules. Its targeted message route is a useful future comparison, not evidence that all applications consume background messages. No such universal behavior is claimed here.

## Checks performed on macOS

Passed:

```text
cargo check -p cli --no-default-features --target x86_64-pc-windows-gnu
cargo clippy -p windows@0.1.0 --target x86_64-pc-windows-gnu --all-targets -- -D warnings
cargo fmt -p windows -- --check
```

Those earlier cross-compilation checks did not execute Windows tests. Subsequent native unit tests and live acceptance results are documented in [host results](windows-host-results.md) and [interactive results](windows-interactive-results.md).
