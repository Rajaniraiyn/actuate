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

Process-directed public Quartz clicking and scrolling returned dispatch receipts but did not produce the intended fixture effects. The backend does not retry or switch to global delivery. Tests explicitly observe this limitation before testing global delivery as a separate operation. Window-targeted routing, event metadata, and SkyLight remain follow-up work.

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

Screenshots, display transforms, subscriptions, physical key chords, SkyLight, mobile/Windows/Linux implementations, C ABI and Node addons are not implemented or validated in this slice. Tests did not change System Settings preferences or edit user documents.
