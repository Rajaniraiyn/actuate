# Desktop provider routes

Status: 2026-09-16. This separates the implemented Windows/Linux baseline from candidate providers. Cross-compilation and source review do not establish live desktop compatibility.

## Composition boundary

Choose observation, semantic actions, capture, coordinate input, keyboard input, and cursor visualization independently. A framework name is discovery evidence, not proof that every control supports every route. An Electron window can contain native dialogs, browser content, and a canvas with different capabilities.

The current library already supplies independent `Observe`, `SemanticActions`, `Capture`, `PointerInput`, `KeyboardInput`, `TextInput`, and `CursorVisualization` traits. Provider-owned associated request/frame types permit specialized capture without forcing private window handles into core types. These are ordinary Rust integrations today. A dynamic plugin ABI, runtime registry, and automatic route planner are not implemented.

This example uses existing traits and deliberately keeps observation and semantic references in the same provider instance:

```rust
use unimation::{
    Observe, SemanticActions, PointerInput, ObserveRequest,
    ElementRef, SemanticAction, Receipt, Result, Snapshot,
};

struct Desktop<A, P> {
    accessibility: A,
    pointer: P,
}

impl<A: Observe + SemanticActions, P: PointerInput> Desktop<A, P> {
    fn snapshot(&mut self, request: ObserveRequest) -> Result<Snapshot> {
        self.accessibility.observe(request)
    }

    fn perform(&mut self, target: &ElementRef, action: SemanticAction)
        -> Result<Receipt>
    {
        self.accessibility.semantic(target, action)
    }
}
```

On Windows, `A` can be `windows::Accessibility` and `P` can be `windows::GlobalInput`. On Linux they can be `linux::Accessibility` and `linux::X11`. A future portal pointer provider changes `P`, leaving AT-SPI references intact. Capture and visualization can be additional fields with their own bounds. The existing core `Providers<O, S, P, T>` is also usable, but independently constructing its observation and semantic providers would create unrelated reference registries. Sharing the owning session requires an explicit adapter; cloning an element reference does not transfer ownership.

## Linux routes

| Environment or protocol | Implemented now | Separate candidate integration and boundary |
| --- | --- | --- |
| AT-SPI accessibility bus | App discovery, bounded snapshots, named actions and editable text | Text/range/value interfaces, event subscriptions, toolkit-specific diagnostics. Availability depends on the app exporting interfaces, independently of its display server. |
| X11 protocol, including Xorg | EWMH window data, RandR monitor data, root image capture, XTEST global pointer/key input, native wheel detents | XComposite window capture, richer XInput2 support, cursor capture. Probe extension versions. Do not identify an X server implementation from the protocol alone. |
| XWayland inside Wayland | Automatic X11 input/capture selection is refused | Explicit X-server-scoped integration may operate on its X clients. It does not establish control over native Wayland clients or the whole compositor desktop. |
| Wayland portals | Explicit RemoteDesktop session with D-Bus Notify input and granted stream metadata | PipeWire frame capture and EIS remain pending. AT-SPI works independently. See [Wayland sessions](wayland.md). |
| Compositor-specific Wayland extensions | Not implemented | Independently versioned providers for advertised capture, virtual-input, toplevel, workspace, or compositor IPC capabilities. Never assume a GNOME, KDE, Sway, or Hyprland name establishes all protocols. |
| Kernel virtual devices | Not implemented | Optional uinput provider with explicit device ownership and seat configuration. This generates input, not screenshots, windows, accessibility, or background targeting. |
| Nested/headless X servers, remote displays | No separately validated support | Explicit connection/provider selection and native tests. Coordinate domains and desktop ownership belong to that server; local AT-SPI apps must not be associated by PID alone. |
| Other display stacks | Not implemented | An implementation can supply only the traits it can support. Report unsupported capture or input rather than inventing a generic Linux fallback. |

AT-SPI actions are target-advertised operations. Their existence does not establish coordinate input or renderer visibility. Preserve action names and rejection results. [AT-SPI Action interface](https://gnome.pages.gitlab.gnome.org/at-spi2-core/devel-docs/doc-org.a11y.atspi.Action.html)

### Portal provider lifecycle

A future provider should own a persistent RemoteDesktop session, granted devices, capture streams, and either its EIS connection or D-Bus input route. RemoteDesktop `Start` negotiates actual access. After `ConnectToEIS`, the protocol forbids mixing EIS input with the `Notify*` methods. Session closure must invalidate outstanding input/capture handles. Portal availability is a read-only discovery result; it is not equivalent to an active session. [RemoteDesktop specification](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.RemoteDesktop.html)

Record stream identity, source metadata, buffer dimensions, cursor mode, and mapping revisions. Stream dimensions need not equal AT-SPI coordinates. Current ScreenCast documentation distinguishes embedded cursor pixels from cursor metadata and hidden cursors; recent versions supply a stream serial to avoid recycled PipeWire node IDs. Negotiate the installed interface version before using those fields. [ScreenCast specification](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.ScreenCast.html)

Direct compositor capture is another provider, not a substitute for the entire portal contract. For example, `ext-image-copy-capture-v1` exposes buffer constraints, transforms, damage, presentation timestamps, and session stop events. Its cursor capture support does not grant input injection. [Wayland protocol XML](https://github.com/wayland-mirror/wayland-protocols/blob/main/staging/ext-image-copy-capture/ext-image-copy-capture-v1.xml)

Do not implement a supposed universal injector around libinput. Libinput consumes devices for compositors and X input drivers. For a virtual device, the kernel's uinput interface or an appropriate wrapper is the relevant boundary. Neither provides per-app background delivery automatically. [libinput architecture](https://wayland.freedesktop.org/libinput/doc/latest/what-is-libinput.html), [kernel uinput documentation](https://docs.kernel.org/input/uinput.html)

## Windows routes

| App or rendering family | Implemented baseline path | Candidate integration and evidence required |
| --- | --- | --- |
| Standard Win32, WinForms, WPF controls | UIA observation and available semantic patterns; explicit global SendInput | Additional UIA patterns, cached property batches, events. Per-control pattern availability determines support. |
| WinUI, UWP, XAML islands | Same UIA client, where providers expose elements | Desktop-root discovery for hosted fragments; Windows.Graphics.Capture for suitable window frames. Test hosting HWND/process boundaries. |
| Chromium, Electron, WebView2 | UIA when exported; desktop capture; explicit global input | Independent browser protocol provider only for a connected browser target. No automatic script injection or assumed HWND-message support. |
| Legacy MSAA/custom controls | UIA data where proxies expose it | Dedicated MSAA observer/action provider; preserve native child IDs, roles, and process generation. |
| Qt, GTK, Java, custom canvas, games | Whatever accessibility the target exports; desktop pixels and global input | Toolkit/bridge integrations based on observed interfaces. Canvas coordinates require verified capture mapping. Rendering technology alone supplies no semantic tree. |
| Elevated apps, secure desktop, other Windows sessions | No automatic elevation/session switch | Explicit interactive-session transport and permission diagnostics. A service process must not report an empty Session 0 desktop as an empty user desktop. |

Microsoft supplies UIA providers for standard Win32, WinForms, and WPF controls; custom controls need their own provider or a proxy. Hosted fragments may mix frameworks inside one window. This is why Unimation should preserve framework IDs and advertised patterns rather than add a `supports_wpf=true` claim. [UIA provider overview](https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-providersoverview)

WinUI exposes its accessibility through automation peers and control patterns. Add patterns by capability, including scroll, range value, text selection, and virtualization, rather than creating separate CLI command trees for WinUI and WPF. [WinUI pattern interfaces](https://learn.microsoft.com/en-us/windows/apps/design/accessibility/control-patterns-and-interfaces)

### Background input and capture candidates

A future window-message provider should be explicitly target-scoped and report delivery separately from observed effects. `PostMessage` only queues a message and is subject to integrity restrictions. It does not certify that a framework consumed the event. Client-coordinate packing, child HWND selection, modifier semantics, and native dragging require dedicated tests. [PostMessage documentation](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-postmessagew)

The implemented SendInput provider affects the shared input stream. Its returned count is accepted events, not task success. UIPI restrictions and held keyboard state matter. It must never become an implicit fallback from a background request. [SendInput documentation](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-sendinput)

Window capture should also be replaceable independently. `PrintWindow` asks the target to render and can block. Windows.Graphics.Capture is another candidate; negotiate capture support and lifecycle rather than treating a black frame as proof that the app is blank. Current GDI capture is the shared desktop, so covering windows are expected pixels. [PrintWindow documentation](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-printwindow), [Windows capture documentation](https://learn.microsoft.com/en-us/windows/apps/develop/media-authoring-processing/screen-capture)

## Reference implementation findings

Pinned source review, not native reproduction:

- Cua Windows separates UIA/window-message background delivery from explicitly selected foreground SendInput. It contains action-specific refusals for targets where synthetic messages are dropped. This is useful routing evidence, not a universal framework support guarantee. Its Linux delivery layer similarly distinguishes focused injection from actual target-scoped background input. [Windows delivery source](https://github.com/trycua/cua/blob/625118a9076e51da2f57b6a5d475972030197443/libs/cua-driver/rust/crates/platform-windows/src/input/delivery.rs), [Linux delivery source](https://github.com/trycua/cua/blob/625118a9076e51da2f57b6a5d475972030197443/libs/cua-driver/rust/crates/platform-linux/src/input/delivery.rs)
- Pi Windows rejects raw coordinate input unless its request policy is foreground. Its Linux documentation separates AT-SPI from X11 capture/input and explicitly says portal diagnostics do not implement a working Wayland session. These distinctions are useful acceptance criteria for our capability reports. [Windows input source](https://github.com/injaneity/pi-computer-use/blob/4b8dbd7eaa13328ab1a8a4b55d0be0b077de7d62/native/windows/bridge-rs/src/input.rs), [Linux provider documentation](https://github.com/injaneity/pi-computer-use/blob/4b8dbd7eaa13328ab1a8a4b55d0be0b077de7d62/docs/linux.md)

## Integration sequence

1. Validate the current native baseline on real X11, Wayland, and Windows sessions. Record app/framework versions, display scaling, input route, and postconditions. Wayland input requires an explicit portal session and user grant; capture still reports unavailable.
2. Add portal discovery and an explicitly opened persistent session. Implement one capture stream and one input route before compositor-specific extensions.
3. Add Windows UIA pattern coverage and a window capture provider. Keep isolated timeout handling around providers that can hang.
4. Prototype Windows message delivery and Linux targeted alternatives as opt-in providers. Test ordinary click, hover, text, selection, slider tracking, and native drag-and-drop separately.
5. Add target-scoped overlays separately from injection. A drawn cursor never proves that the OS cursor, focus, or input recipient matches it.
6. Register compiled providers at the host boundary. Runtime route decisions must carry target ownership, coordinate domain, availability evidence, focus/cursor effects, and failure results. Type bounds establish implemented operations; they cannot establish runtime permission, target compatibility, or successful UI effects.
