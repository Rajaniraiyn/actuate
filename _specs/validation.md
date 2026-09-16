# Validation on macOS

Checked on this development Mac on 2026-09-15. These are observations on the installed app and OS versions, not universal compatibility claims. The direct Rust CLI reported accessibility trust as granted. Swift is used only to compile a disposable test app.

## Working end-to-end paths

- Calculator: observe native buttons, perform `AXPress` for `2 + 3 =`, and reobserve the display containing 5. Display text retains native direction marks.
- Calculator references: a live button kept its reference across actions and observation. Another session rejected the reference.
- Fixture: set Unicode `AXValue`, set boolean `AXFocused`, and read each value back.
- Fixture: process-directed Quartz Unicode keyboard events entered `Hi🦀` in the focused field.
- Fixture: global Quartz click invoked a button, and global movement plus scrolling reached the scroll view.
- Protocol: unknown fields rejected before dispatch, request IDs echoed, invalid JSON recovered, foreign references rejected, default budgets applied, and node-limit truncation reported.

## Known input limits

Process-directed public Quartz clicking and scrolling returned dispatch receipts but did not produce the intended fixture effects. The backend does not retry or switch to global delivery. Tests explicitly observe this limitation before testing global delivery as a separate operation. The separate SkyLight provider now implements window-targeted packets; see [SkyLight validation](skylight.md#host-validation) for its app-specific results. This does not change the public Quartz result.

No assertion claims that a successful public event-post call proves event consumption or that a PID identifies a particular window. Global input may change foreground interaction and the shared pointer. No background-focus guarantee has been validated.

## Read-only app coverage

| Application | Observation | Result |
| --- | --- | --- |
| Calculator | Application tree | 229 nodes in the initial sample; arithmetic actions verified separately |
| System Settings | Application tree | About 386 nodes; outlines, rows, cells, text field, toolbar and scroll areas |
| Dock | Application tree | 18 nodes, including 16 Dock items |
| Finder | 300-node budget | Menus dominated the budget; content and table elements also appeared |
| Spotlight | Application tree | 132 nodes including menus; no active search panel in this sample |
| Control Center | Application tree | 9 nodes including 7 menu-bar items; traversal completed |
| VS Code | Application tree, then web-area subtree | Initial 500-node budget dominated by menus. Subtree returned 277 nodes with completed traversal |

The VS Code subtree included 33 buttons, 56 static-text nodes, 7 toolbars, checkboxes, groups and other controls. Its Electron content was available through AX without changing accessibility settings. This does not establish coverage for every Electron application.

System Settings produced native invalid-element errors during observation. Control Center produced some native attribute failures even with complete traversal. Responses preserve these errors. `traversal_complete` distinguishes traversal coverage from aggregate `complete`.

## Automated checks

`cargo test --workspace` covers strict mutation parsing, observation defaults, retained opaque native values and nonfinite float preservation. `tests/macos_e2e.py` checks the live paths described above. Formatting and Clippy are also checked.

Subscriptions, mobile/Windows/Linux implementations, C ABI and Node addons remain unimplemented. Tests did not change System Settings preferences or edit user documents.

## Expanded implementation, 2026-09-16

The runtime now includes snapshot diff/query, capture mappings, native ScreenCaptureKit capture, physical keys, richer pointer actions and SkyLight. Implementation availability is separate from host compatibility. The earlier app table remains a record of the original observation samples.

Core tests added during this iteration cover incomplete diff coverage, reparenting with stable references, foreign/duplicate references, parameter changes, read errors and nested opaque values, null versus missing attributes, alternative native names, and invalid geometry. The final workspace run passed 18 core tests, 10 macOS tests and two overlay tests. Two permission-dependent capture unit tests stay ignored in the default suite; live capture paths were checked separately.

[SkyLight host results](skylight.md#host-validation) document the fixture and native event-probe checks, including a background first-mouse limitation. The [overlay README](../crates/overlay/README.md#validation) records its independent visual test. Neither establishes support for every application, display layout or system panel.

The expanded live fixture suite passed checkbox semantic toggles and explicit glyph clicking, slider clicking, popup menu selection, modal sheet discovery/dismissal and rejection of a global reference click beneath the sheet, dynamic insertion/removal diffs, unchanged sibling references, window-local clicking, and 300-pixel resized native window capture followed by an effective image click. Reusing that frame after input returned `stale_frame` with no effect.

The integrated overlay followed SkyLight input while the foreground PID and shared cursor coordinates stayed unchanged. Both native and executable desktop captures visibly contained the purple pointer after the helper rendered. An earlier capture preceded visible rendering, so queue acceptance remains distinct from a rendered-frame acknowledgement. Only one physical display layout was tested live; negative origins and differing scales have unit coverage.

Formatting, workspace Clippy with warnings denied, and a macOS build without the native-capture feature passed. Native capture has a 15-second caller deadline, but macOS callbacks can finish work after that deadline. An output write failure may leave a partial destination file, which is never reported as a successful frame. These recovery cases remain follow-up work.

The session now queues optional cursor visualization after SkyLight dispatch and exposes `cursor_state`. This integration is pending the final live test report. AX bounds-center selection remains a heuristic; controls with blank space inside their accessibility bounds require observed acceptance or a more precise point.

## Compact presentation and viewport loop

The compact-output iteration passed 34 core tests, 12 macOS tests and two overlay tests. Two permission-dependent capture unit tests remain skipped by default. Formatting, Clippy with warnings denied, and the macOS build without native capture passed. `tests/presentation_cli.py` checks native JSON preservation, text escaping, short-reference scope, output limits, and projected versus native diffs.

A real NSScrollView fixture starts with Row 12 below its viewport. A reference-center SkyLight click at that point returns `outside_viewport` with no effect. Pixel scrolling moves the same reference inside, leaves Row 0 above, and produces those relation changes in `diff_view`. A subsequent SkyLight click updates the fixture's status to `clicked:12`. `tests/presentation_e2e.py` checks this complete loop and retains the raw observations beneath both text and compact JSON views.

VS Code's focused window produced 540 nodes in one sample. The saved native JSON occupied 3,208,271 bytes; the interactive text projection of that same observation occupied 5,609 bytes and showed 64 candidates, with 476 filtered nodes explicitly counted. These are byte counts for different representations, not a claim of lossless compression or measured model-token counts. Native traversal completed, while native attribute errors remained reported.

On that VS Code window, SkyLight selected Search and then restored Explorer with the original retained references. AXSelected confirmed both state changes. The initial global center hit test reported occlusion, which did not prevent the window-targeted route from working. The projected diff reported selection changes and newly displayed controls. No document content was edited or search submitted. The editor exposed an accessibility message asking for screen-reader mode, so this test does not establish full editor-text accessibility.

## Native menu bar controls, 2026-09-16

- Control Center: opened the actual menu-bar panel and its Display, Sound and Wi-Fi
  details. Attributed AX descriptions supplied labels missing from plain attributes.
  Panel contents changed during opening; observations must wait for the expected control.
- Display and Sound: `set_float` on `AXValue` returned unsupported with no effect.
  Advertised `AXDecrement` and `AXIncrement` actions changed each slider from 1.0 to
  approximately 0.9 and restored it to 0.9999998807907104, within 1e-4 of its original
  value and displayed as 100%. Dark Mode, Night Shift and audio output were unchanged.
- Wi-Fi: invoked the exact advertised custom details action. Read the enabled/connected
  state without toggling Wi-Fi or switching networks. `AXShowMenu` instead opened the
  editing context menu despite returning an unknown effect.
- Spotlight: one deliberate global Command+Space opened the actual search panel after
  checking the foreground PID. Process-directed text entered `2+3`; native result text
  contained `5`. Process-directed Escape cleared and dismissed the panel.
- Ghostty: native menu actions opened About Ghostty. A native window capture showed the
  About dialog. Its own `AXCloseButton` closed it; a subsequent observation showed only
  the original terminal window.
- Discovery: the apps projection selected 12 of 65 records in one sample and reported
  the 53 omitted records. Default JSON preserved all records. Counts depend on running apps.

An initial cleanup script incorrectly sent global Escape repeatedly. The user observed
this interrupt the terminal interaction. Control Center's AX window was focused while
Ghostty remained the foreground process. Subsequent tests used process-directed Escape
and verified dismissal. `AXCancel` and menu-item press receipts alone did not prove that
an open subpanel closed. The final observation found no Control Center or Spotlight windows.

These checks establish the native semantic and process-key routes described above.
They do not establish SkyLight pointer support for every system panel. No documents
were edited and no network configuration was changed.

The final workspace suite passed 54 tests with two permission-dependent capture tests
ignored. Workspace build, formatting, Clippy with warnings denied, the macOS build
without native capture, CLI presentation regressions, and live discovery projections
passed. These automated checks are separate from the native panel observations above.


## iPhone and iPad Simulator, 2026-09-16

The installed iOS 26.4 runtime reported version 26.4.1, build 23E254a. Tests created an
iPhone 17 Pro and iPad Pro 13-inch M5 in a separate temporary device set. Both booted
without downloading runtimes or installing guest applications.

Native semantic presses navigated Settings to General and About on each device. Repeated
observations retained the General element reference. Raw diff and compact text rendering
worked; foreign session references were rejected without effects. Native PNG screenshots
visually confirmed the iPhone About page and iPad split-view About page. No settings
values were changed. Two consecutive final iPad About snapshots contained 18 nodes each
and had zero new, removed, unobserved or modified nodes. Its text projection was 1,516 bytes.

The final workspace suite passed 61 tests, with two existing capture tests ignored.
Workspace build, strict Clippy, CLI presentation regressions and the macOS build without
native capture passed. Live protocol checks covered invalid limits, unknown fields,
request-ID echoing and recovery. These results cover native AX semantic input, not HID.

The initial `xcrun simctl` listing invoked Xcode's automatic first-launch setup wrapper.
That wrapper was stopped. All subsequent operations used the existing CoreSimulator
executable directly. No simulator runtime download was requested.

Both test devices were shut down and deleted, then their temporary device set and
screenshots were removed. The default device-set UUID list matched its pre-test list.
The reproducible test is `tests/ios_simulator_e2e.py`; API and limitations are in
[the iOS guide](ios.md).


## Composed iOS input and shared overlay, 2026-09-16

A second disposable portrait iPad validated the composed library session after moving it
out of the CLI. Native hardware Home returned to the home screen. A normalized touch tap
opened Settings. A native press focused search and USB HID usage 4 entered `A`; iPadOS
capitalized the key. Tests therefore distinguish keyboard events from exact Unicode text.

A top-edge touch swipe opened Control Center. This fresh simulator exposed its header,
Add Controls and Power, but did not display the populated controls seen in the earlier
macOS Control Center test. No guest settings values were changed. A later integrated test
used an explicit AX point scope to read home icons, pressed Settings, typed USB HID usage
5 into search, opened Control Center by edge swipe and captured its PNG dimensions.
`tests/ios_system_e2e.py` reproduces that sequence on a caller-owned disposable portrait iPad.

Frontmost Home observation returned DockFolderViewService with one node. An explicit
AX hit test at 100,100 returned a nine-node container with widgets and app icons;
400,500 returned a calendar heading. This demonstrates why frontmost application scope
must not be treated as complete OS observation. Explicit SpringBoard PID translation
returned no root on this host and remains an availability-dependent operation.

The experimental native mouse service accepted enable, relative movement and removal.
The host cursor remained exactly at 696.65625,562.8125 throughout the agent's check.
Captures did not establish visible guest cursor movement or click consumption. The mouse
provider stays explicitly activated, separate from touch, and outside the default JSONL
session. A native pointer constructor test passed without dispatch; its default test is
ignored because it requires the installed private framework.

The overlay's shared process controller and command model moved into the overlay library.
AppKit rendering remains the implemented renderer; other platform renderers return explicit
unavailable results. Typed capability composition includes a compile-fail test proving that
an absent input provider cannot be used for touch. The typed session example verifies that
returned observations share the cache allocation through `Arc`.

The composed workspace finished with 69 unit tests and one compile-fail documentation
test passing, plus three native/permission-dependent tests ignored. All-feature build
and strict Clippy passed. The standalone iOS binary also passed live streamed-snapshot,
explicit-null request ID and malformed-request recovery checks. The disposable iPad
was shut down and deleted; its data directory and test captures were removed.

## Linux, 2026-09-16

Checked on this Arch host running Hyprland 0.56.2 (Wayland, one 1920x1200 output at scale
1.25), Xwayland 24.1 without the EI portal, AT-SPI2 2.60 and GTK 4.22. These are
observations on those versions, not universal compatibility claims. The user kept working
on other workspaces; every live check ran against disposable fixtures on a separate
workspace, and compositor-seat input was dispatched only while that workspace was active.

### Working end-to-end paths

- Discovery: the accessibility bus listed five registered applications; Hyprland clients
  were merged by pid, and mapped windows without an accessible application were reported
  separately. Chromium-based windows were absent because the bus `IsEnabled` flag was off.
- Observation: a 65-node Ghostty tree read in about 0.9 s with complete traversal. The
  GTK 4 fixture exposed frame, button, entry, check button, slider, list, rows and text
  view with canonical roles, toolkit role names, states, interfaces and actions.
- Geometry: GTK 4 reports window-relative extents for both coordinate types on Wayland.
  Adding the Hyprland client origin produced frame bounds equal to the compositor's window
  position. Xwayland extents are in X pixels, 1.25 times the logical size on this output,
  and are divided by the monitor scale before use.
- Semantic actions: `Click` on the button incremented the fixture counter twice, a
  misspelled action returned `unsupported` with no effect, `set_string` on the entry wrote
  `Hi🦀` and `attribute Text.Text` read it back, `set_float` moved the slider and
  `wait_attribute` on `Value.CurrentValue` matched, and `observe_subtree` reflected the
  toggled check button.
- Retained observations: `query` by role and name found the button, `diff` listed the
  changed check button and slider, `diff_view` reported the status label change, and the
  interactive text view listed the button.
- Window-targeted keys: GTK 4.22 check buttons advertise no action and reject
  `Component.GrabFocus` as unsupported. Shift+Tab sent through `hyprland_shortcut` walked
  the fixture's focus chain (the fixture logs focus changes), Space toggled the check
  button, and a letter reached the entry, all while `hyprctl activewindow` stayed on the
  terminal.
- Capture: unoccluded toplevel capture through ext-image-copy-capture, output capture
  through wlr-screencopy and a region crop all produced PNG frames with geometry mappings;
  an image click with semantic mode returned `unsupported`.
- Visual cursor: the in-process layer-shell renderer and the `actuate-overlay` helper
  both registered an overlay layer surface named `actuate-cursor` that `hyprctl layers`
  listed, and removed it on stop. A `grim` capture of the screen region around a helper
  `move`/`show`/`click` at 1151,551 showed the purple glyph with its white outline and the
  click ring at that point; `hyprctl layers` placed the surface at 1119,519 with the
  96-point box, so the hotspot matched the requested coordinate. During the suite the
  window-scoped glyph followed the semantic click to the button's center while the
  fixture workspace was shown and was parked off-screen while another workspace was
  active. No input events were injected by the overlay.
- Compositor seat pointer: with the fixture workspace active, a virtual-pointer move
  landed at the requested logical point exactly (`hyprctl cursorpos` matched to the pixel)
  and a subsequent click focused and incremented the button.
- Guard: with another workspace active, a reference-targeted global click returned
  `not_on_active_workspace` with no effect and the shared cursor did not move.
- Xwayland: with the X11 route selected, an XTest click through a fixture running under
  `GDK_BACKEND=x11` incremented its counter while `hyprctl cursorpos` stayed exactly where
  it was, and `GetImage` captured the X window drawable. A synthetic `send_event` click
  returned a delivery receipt without a fixture reaction, as documented.

The final suite after the cleanup pass: 63 core, 11 Linux, 9 overlay, 3 compositor,
9 iOS and 28 Android unit tests passed; Clippy with warnings denied and the Apple crates'
`aarch64-apple-darwin` type-check passed from Linux. `tests/linux_e2e.py` passed every
section on a run with the fixture workspace active, including the compositor-seat pointer
click that moved the shared cursor onto the button and incremented the counter.

### Real applications, 2026-09-16

`tests/linux_apps_demo.py` drove two installed applications through one long-lived
`ACTUATE_SOFT_CURSOR=1 actuate session --json` on the fixture workspace while the
user worked on another workspace with the keyboard and mouse; nothing touched the seat.

- Calculator: `omacalc` (Qt Quick under Xwayland) registers an accessible application
  but exposes no children even with `QT_LINUX_ACCESSIBILITY_ALWAYS_ON=1`, so it was
  driven by `click_image` on `x11_window` captures over the X11 route. Eight XTest
  clicks (`AC 2 + 3 =`, then `* 8 =`) produced `5` and `40` in window captures, each
  click recaptured first because a dispatched click advances the epoch, while
  `hyprctl cursorpos` never moved. An X window capture now resolves its Hyprland client
  so the soft cursor is window-scoped and the workspace guard still applies.
- Terminal: a Ghostty window titled `ActuateTerminal` exposes a frame and panels but
  no text node. `echo actuate-ok` plus Return sent as `hyprland_shortcut` keys to the
  frame printed the command and its output in a toplevel capture, and
  `hyprctl activewindow` stayed on the user's browser on another workspace throughout.
- Soft cursor: an output capture showed the glyph on the calculator's `=` key after the
  last click; while the user's workspace was active `hyprctl layers` reported the surface
  parked at -96,-96, and the user moved the floating calculator during the run without
  the glyph detaching. `capture` refuses to overwrite an existing path.
- The idle session with the cursor shown used about 1.5% of one core.

### Known limits

- Hyprland exposes one seat and no `ext-transient-seat`; the virtual pointer moves the
  user's cursor. Non-stealing routes are AT-SPI actions, Hyprland shortcuts and XTest
  inside Xwayland.
- A button press queued in the same flush as the motion that moved the pointer onto a
  GTK 4 button was not delivered on one run; the Wayland route now waits 40 ms after
  motion before pressing.
- `hit_test` and window-targeted clicks depend on Hyprland window records; other
  compositors get window-relative bounds only.
- Layer surfaces cannot join the window stack, so window scope emulates the macOS
  attachment: `compositor::hyprland::stacking_above` derives the windows above the
  target from Hyprland's tiers (tiled, maximized, floating, pinned, fullscreen) and
  client order and the renderer cuts the glyph away under them, and the Hyprland event
  socket triggers geometry refreshes. Unit tests cover the tiering and the cut; the
  covered-glyph case was not observed live because it needs the fixture workspace shown.
  Layer-shell surfaces of other clients (bars, launchers) are not modelled.
- Only one output and one scale were tested live; mixed-scale layouts rely on the
  per-monitor conversion without live evidence.
