# Cursor implementation research

Reviewed 2026-09-16. These are source observations and implementation proposals, not runtime validation of upstream projects.

## Project identities

- `remorses/usecomputer` is the Zig desktop automation CLI. Its native library supports macOS, X11 and Windows. [README](https://github.com/remorses/usecomputer/blob/711a429fa4f005c6a53a05c86a5a23a2cf258579/README.md)
- `trycua/cua` is the Cua repository containing Cua Driver and its cursor overlay crates. `CUA` alone is also a generic abbreviation and does not uniquely identify a repository.
- `xlang-ai/OpenCUA` is a distinct project for computer-use models, data and annotation infrastructure. Its README identifies AgentNet, AgentNetTool and AgentNetBench. This review found no cursor-overlay implementation by filename in its recursive source tree; that is not proof none exists in related repositories. [README](https://github.com/xlang-ai/OpenCUA/blob/dfc91ba89f700d10f26ec50362d308571482ab8b/README.md)

## Cua's macOS overlay

The inspected implementation creates a transparent borderless `NSWindow`, not an `NSPanel`. It sets `ignoresMouseEvents`, disables shadows and uses accessory application activation policy. AppKit initialization requires the main thread. Window ordering is dispatched to the main queue.

The window remains at normal level and uses `orderWindow:relativeTo:` to sit above its target. A guarded `orderFrontRegardless` workaround applies only when that exact target is already the frontmost visible normal window. Collection behavior includes join-all-Spaces, full-screen auxiliary and stationary.

Rendering uses the main screen's frame and backing scale. This is not a demonstrated general solution for mixed-scale displays or display changes. Read-only sharing is explicitly enabled for capture. Cursor identities have separate animation arrival waiters; session tombstones prevent delayed commands recreating removed cursors.

[Inspected overlay source](https://github.com/trycua/cua/blob/14dd8cfdec2be0f64ed67597a69082ac22c0e708/libs/cua-driver/rust/crates/platform-macos/src/cursor/overlay.rs)

## Cua's motion components

The shared overlay crate contains cubic Bezier evaluation and tangent/heading methods, with length estimated by 32 line segments. A separate Dubins planner supports minimum-turning-radius paths with linear fallback. Motion settings include speed, turn radius, click dwell, idle hiding and post-arrival spring damping. The presence of both planners should not be summarized as every move using Bezier interpolation.

[Bezier math](https://github.com/trycua/cua/blob/14dd8cfdec2be0f64ed67597a69082ac22c0e708/libs/cua-driver/rust/crates/cursor-overlay/src/bezier.rs), [path planner](https://github.com/trycua/cua/blob/14dd8cfdec2be0f64ed67597a69082ac22c0e708/libs/cua-driver/rust/crates/cursor-overlay/src/path_planner.rs), [motion settings](https://github.com/trycua/cua/blob/14dd8cfdec2be0f64ed67597a69082ac22c0e708/libs/cua-driver/rust/crates/cursor-overlay/src/motion.rs)

## usecomputer's input paths

The Zig library's drag operation accepts an optional quadratic Bezier control point. Without it, interpolation is linear. It uses 32 samples and derives default duration from a control-polygon length estimate, with a minimum duration of 200 ms. Control-polygon length is an estimate, and equal parameter steps do not provide constant arc-length speed.

On macOS, its movement helper warps the shared cursor and posts a `kCGEventMouseMoved` event. The inspected drag loop calls that helper while holding the mouse button. It therefore is not evidence of a separate synthetic cursor or a background input route. Its error branches do not establish a general cleanup guarantee after every failed step. These choices should not be copied as Actuate's drag semantics.

[Native implementation, drag and movement helpers](https://github.com/remorses/usecomputer/blob/711a429fa4f005c6a53a05c86a5a23a2cf258579/zig/src/lib.zig#L1566)

## Decisions for Actuate

These are proposed design rules derived from the comparison:

- Keep visual cursor motion separate from input delivery. A decorative spring overshoot must never move the actual click or drag endpoint.
- Keep overlay drawing on the AppKit main thread and make key/main-window refusal explicit. Do not set a host application's activation policy from a reusable library provider.
- Give each cursor a session identity and animation generation. Cancelled, superseded and arrived are separate outcomes. Late updates cannot recreate removed cursors.
- Use elapsed monotonic time for animation, finite validated geometry, bounded duration, and an exact final endpoint. Handle zero-distance movement without division by zero.
- Define coordinates in desktop logical points. Convert against the primary display's desktop origin, then the destination screen's AppKit frame. Use per-screen backing scale and refresh after display changes.
- Treat capture inclusion as a requested policy. Exclude known owned overlay window IDs from foreground-target verification. A visible cursor must not be mistaken for an application window.
- Keep native drag event types, held-button cleanup and route validation independent of the visual path planner. A visually smooth animation does not verify application input consumption.

## Follow-up review

See [the newer automation audit](automation-reference-audit.md) for current Cua sources and [the overlay README](../crates/overlay/README.md) for implemented appearance/motion changes and visual validation.
