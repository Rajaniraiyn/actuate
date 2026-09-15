# Automation and cursor source audit

Reviewed 2026-09-16 against pinned upstream source. This review did not run either upstream application. Recommendations below are proposed work unless another document records implementation and validation.

## Source revisions and reuse

| Project | Revision | License at revision | Use in Unimation |
| --- | --- | --- | --- |
| AXTerminator | `e6c638d7763f61a6a5370c5acc36b27607837a42` | [PolyForm Noncommercial 1.0.0](https://github.com/MikkoParkkola/axterminator/blob/e6c638d7763f61a6a5370c5acc36b27607837a42/LICENSE.md) | Study behavior and implement independently. Do not treat this as an interchangeable permissive dependency or copy source into Unimation. |
| Cua | `fc188250b4ca8549b8e61f937fdb1fb560770e86` | [MIT](https://github.com/trycua/cua/blob/fc188250b4ca8549b8e61f937fdb1fb560770e86/LICENSE.md) | Reference shared motion and renderer boundaries. Preserve the required copyright/license notice if copying substantial source. |

AXTerminator's `axterminator-core` manifest also declares PolyForm Noncommercial. Extracting that crate does not avoid the license distinction. No upstream code was copied for this audit.

## AXTerminator findings

### Element recovery must preserve identity

The source has ordered locator strategies, a time budget, cached successful queries, and a separate element cache. These offer useful ideas for query recovery. Its element cache uses PID and query as keys, with a default 500 entries and five-second lifetime. Before returning a cached result, application lookup checks whether the element still has a role. Its healing cache is global and keyed by original query. These caches are not proof that a reference still names the intended control after app relaunch or list virtualization.

Unimation already retains native AX objects and never structurally rebinds references in `crates/macos/src/accessibility.rs`. Preserve that rule. Add optional locator recovery as a distinct operation that returns candidate references and matching evidence. Scope cache entries by provider session, application incarnation, window and query. Ambiguous labels must produce candidates, not an automatic click. A recovered element gets its own identity; it must not silently inherit an old `e12`.

Sources: [element cache](https://github.com/MikkoParkkola/axterminator/blob/e6c638d7763f61a6a5370c5acc36b27607837a42/src/cache.rs), [application lookup](https://github.com/MikkoParkkola/axterminator/blob/e6c638d7763f61a6a5370c5acc36b27607837a42/src/app.rs), [healing](https://github.com/MikkoParkkola/axterminator/blob/e6c638d7763f61a6a5370c5acc36b27607837a42/src/healing.rs).

### Stability is observable evidence, not application completion

The heuristic synchronization code hashes a bounded tree and polls every 50 milliseconds until three consecutive comparisons match. Another route uses an application SDK through XPC. A matching partial tree cannot prove that network work, rendering or a hidden modal has finished.

A composable `WaitCondition` should support a specific element predicate, tree quiet interval, window change or backend completion signal. Return which condition succeeded, its observation scope, elapsed time and whether traversal was complete. Preserve native operation deadlines inside the overall wait budget. Reuse Unimation snapshots and diffs rather than introduce a second normalization or identity scheme.

Source: [synchronization implementations](https://github.com/MikkoParkkola/axterminator/blob/e6c638d7763f61a6a5370c5acc36b27607837a42/src/sync.rs).

### Background action claims need route-level inspection

`perform_click_native` tries `AXPress`, but its fallback calls a coordinate click that posts to the global HID event tap. This fallback can occur even when the requested mode was background. `double_click_native` performs two semantic clicks with a delay; this does not establish native double-click event semantics. `children()` returns an empty vector on a native read error, which can hide an incomplete tree.

Keep Unimation's explicit input routes and `Effect::Unknown` for uncertain mutations. An unsupported semantic action may offer an allowed coordinate route, but cannot silently escalate to global input. Coordinate fallback needs current geometry, clipping, occlusion and modal checks. Native double-clicks need click-count semantics and release cleanup. Failed children reads remain reported errors or incomplete observations.

Source: [element action implementations](https://github.com/MikkoParkkola/axterminator/blob/e6c638d7763f61a6a5370c5acc36b27607837a42/src/element.rs).

### Capability declarations must distinguish stubs

The inspected MCP AX observer explicitly has no run-loop implementation. Subscription returns success but delivers no events. The visual healing strategy returns `None`. These are not working notification or visual recovery providers at this revision.

Unimation should report an unavailable capability until its provider can deliver it. A real notification provider needs its own run loop, bounded event queue, overflow reporting and full resnapshot after lost events. Notifications can retire destroyed references and dirty snapshot regions; polling remains a separate fallback. The current macOS accessibility implementation explicitly records event-based retirement as pending.

Sources: [observer stub](https://github.com/MikkoParkkola/axterminator/blob/e6c638d7763f61a6a5370c5acc36b27607837a42/src/mcp/observer.rs), [visual recovery stub](https://github.com/MikkoParkkola/axterminator/blob/e6c638d7763f61a6a5370c5acc36b27607837a42/src/healing.rs).

### Electron stays an optional integration

The CDP module discovers debugging ports and provides DOM, JavaScript and input operations. It is a useful example of a separate application integration, but it assumes an exposed debugging endpoint. It does not establish that every Electron app can be attached without configuration.

Keep native AX as the initial route. A future explicit CDP provider can expose browser-specific capabilities without making JavaScript execution part of the common interaction contract. Do not launch an app with a debugging port, scan unrelated endpoints or change the user's profile as a side effect of ordinary discovery.

Source: [Electron connection](https://github.com/MikkoParkkola/axterminator/blob/e6c638d7763f61a6a5370c5acc36b27607837a42/src/electron_cdp.rs).

## Cua cursor findings

The current shared `cursor-overlay` crate contains path planning, motion settings, render state, themes, badges and z-order helpers. The macOS renderer owns native windows and rendering delivery. This is directly useful to Unimation's cross-platform overlay design. Importing the crate wholesale would also bring its workspace-specific contract, theme artifacts and rendering dependencies; adopting the boundaries independently is a smaller first step.

Sources: [shared exports and configuration](https://github.com/trycua/cua/blob/fc188250b4ca8549b8e61f937fdb1fb560770e86/libs/cua-driver/rust/crates/cursor-overlay/src/lib.rs), [dependencies](https://github.com/trycua/cua/blob/fc188250b4ca8549b8e61f937fdb1fb560770e86/libs/cua-driver/rust/crates/cursor-overlay/Cargo.toml).

The shared render state separates motion progress, arrival, spring settling, click feedback and idle fading. Path planning has its own types. Session removal records tombstones so queued updates cannot recreate a removed cursor. Configuration includes a reduced-motion policy. The native overlay disables the window shadow, so a request for a soft cursor shadow should be implemented in cursor drawing rather than attributed to an upstream AppKit window shadow.

Sources: [render state](https://github.com/trycua/cua/blob/fc188250b4ca8549b8e61f937fdb1fb560770e86/libs/cua-driver/rust/crates/cursor-overlay/src/render_state.rs), [path planning](https://github.com/trycua/cua/blob/fc188250b4ca8549b8e61f937fdb1fb560770e86/libs/cua-driver/rust/crates/cursor-overlay/src/path_planner.rs), [macOS renderer and tombstones](https://github.com/trycua/cua/blob/fc188250b4ca8549b8e61f937fdb1fb560770e86/libs/cua-driver/rust/crates/platform-macos/src/cursor/overlay.rs).

### Changes appropriate for Unimation

At audit start, our shared interpolation was a straight path with cubic easing, and the AppKit renderer drew a flat purple polygon with a white outline. A degenerate cubic produces smooth timing but does not produce a curved route.

1. Put validated style, motion planning and time-dependent frame state in `overlay`. Native renderer modules consume the resulting frame. This keeps shape, scale, accent, shadow, ripple and reduced-motion choices portable.
2. Use a bounded curved cubic path with endpoint easing and exact final coordinates. Retarget from the currently displayed position. Elapsed monotonic time must determine progress, even when frame delivery pauses.
3. Draw an antialiased arrow with a clear outline and local soft shadow. Anchor its tip at the target coordinate and leave padding for the shadow. Use a fading click ring rather than scale changes that displace the tip.
4. Keep decorative animation separate from actual input. A spring or curved visual route must not alter the click endpoint, drag event trajectory or action result. Cursor arrival is separate from application acknowledgement.
5. Support immediate movement or reduced motion. Zero-distance moves, zero duration, hidden cursors, interruption and very large coordinates need deterministic results.
6. Preserve display-coordinate mapping and per-display scale. Include mixed-scale screens, negative desktop origins and a target crossing screens in validation.
7. As multiple cursors become supported, use session identity plus animation generation. Removed sessions cannot reappear because of late messages. Report queued, arrived, cancelled and superseded separately.

See [earlier cursor notes](reference-cursor-notes.md) for the prior pinned Cua and usecomputer comparison. These findings refine those notes; they do not imply upstream rendering or input was tested on this machine.

## Integration order

- First improve shared overlay motion/style and its native drawing, with pure geometry tests and a visual review.
- Next add a typed wait provider that consumes existing observations and reports evidence.
- Add native notification delivery with explicit event loss and reference retirement.
- Add optional locator recovery with candidate scoring and ambiguity reporting.
- Keep device discovery, capture, observation, input and overlays as separate capability providers. A discovery result must not imply that all interaction capabilities exist.
