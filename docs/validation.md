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

[SkyLight host results](skylight.md#host-validation) document the fixture and native event-probe checks, including a background first-mouse limitation. The [overlay README](../crates/unimation-overlay/README.md#validation) records its independent visual test. Neither establishes support for every application, display layout or system panel.

The expanded live fixture suite passed checkbox semantic toggles and explicit glyph clicking, slider clicking, popup menu selection, modal sheet discovery/dismissal and rejection of a global reference click beneath the sheet, dynamic insertion/removal diffs, unchanged sibling references, window-local clicking, and 300-pixel resized native window capture followed by an effective image click. Reusing that frame after input returned `stale_frame` with no effect.

The integrated overlay followed SkyLight input while the foreground PID and shared cursor coordinates stayed unchanged. Both native and executable desktop captures visibly contained the purple pointer after the helper rendered. An earlier capture preceded visible rendering, so queue acceptance remains distinct from a rendered-frame acknowledgement. Only one physical display layout was tested live; negative origins and differing scales have unit coverage.

Formatting, workspace Clippy with warnings denied, and a macOS build without the native-capture feature passed. Native capture has a 15-second caller deadline, but macOS callbacks can finish work after that deadline. An output write failure may leave a partial destination file, which is never reported as a successful frame. These recovery cases remain follow-up work.

The session now queues optional cursor visualization after SkyLight dispatch and exposes `cursor_state`. This integration is pending the final live test report. AX bounds-center selection remains a heuristic; controls with blank space inside their accessibility bounds require observed acceptance or a more precise point.

## Compact presentation and viewport loop

The compact-output iteration passed 34 core tests, 12 macOS tests and two overlay tests. Two permission-dependent capture unit tests remain skipped by default. Formatting, Clippy with warnings denied, and the macOS build without native capture passed. `tests/presentation_cli.py` checks native JSON preservation, text escaping, short-reference scope, output limits, and projected versus native diffs.

A real NSScrollView fixture starts with Row 12 below its viewport. A reference-center SkyLight click at that point returns `outside_viewport` with no effect. Pixel scrolling moves the same reference inside, leaves Row 0 above, and produces those relation changes in `diff_view`. A subsequent SkyLight click updates the fixture's status to `clicked:12`. `tests/presentation_e2e.py` checks this complete loop and retains the raw observations beneath both text and compact JSON views.

VS Code's focused window produced 540 nodes in one sample. The saved native JSON occupied 3,208,271 bytes; the interactive text projection of that same observation occupied 5,609 bytes and showed 64 candidates, with 476 filtered nodes explicitly counted. These are byte counts for different representations, not a claim of lossless compression or measured model-token counts. Native traversal completed, while native attribute errors remained reported.

On that VS Code window, SkyLight selected Search and then restored Explorer with the original retained references. AXSelected confirmed both state changes. The initial global center hit test reported occlusion, which did not prevent the window-targeted route from working. The projected diff reported selection changes and newly displayed controls. No document content was edited or search submitted. The editor exposed an accessibility message asking for screen-reader mode, so this test does not establish full editor-text accessibility.
