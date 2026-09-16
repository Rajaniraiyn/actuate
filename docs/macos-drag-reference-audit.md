# Background drag source audit

Reviewed 2026-09-16. This is a source review, not a claim that the upstream tools
were executed. Our live comparison is in [macOS drag validation](macos-drag-validation.md).

## Findings

### Cua / trycua

At `27062ed04a36a5aa1969fa44f58349d16f0bdc13`, the public macOS drag tool
rejects non-foreground delivery with `background_unavailable`. Its foreground
branch activates the target and uses global HID events. A lower-level targeted
helper remains in the source, but its existence does not establish support in
the public tool.

That helper uses an HID-state event source, posts through both SkyLight and
`CGEventPostToPid`, adds a 50 ms dwell before release, and optionally reposts the
release globally. Creating an HID-state event source is distinct from posting to
the global HID tap. A worker thread or different event source alone is not proof
of held-button state at the target.

The shared stamping helper still writes private field 58 as a click-group ID.
Our native probe found that write changes timestamps on this host. Do not copy
that assumption. Posting the same packet through two transports may also deliver
duplicate input; it needs target-side evidence before adoption.

Sources: [public drag tool](https://github.com/trycua/cua/blob/27062ed04a36a5aa1969fa44f58349d16f0bdc13/libs/cua-driver/rust/crates/platform-macos/src/tools/drag.rs#L186),
[mouse delivery](https://github.com/trycua/cua/blob/27062ed04a36a5aa1969fa44f58349d16f0bdc13/libs/cua-driver/rust/crates/platform-macos/src/input/mouse.rs#L634).

The identifiable project is `trycua/cua`. The search did not establish a separate
repository named Tri-CUA, so it is not counted as independent corroboration.

### Dioxus accessibility-cli

At `519e0d3b4d339a3a5c1663a092d2169424b07afb`, the macOS low-level mouse API
has Move, Down and Up variants. Move maps to `MouseMoved`; there is no dragged
variant in this event constructor. Its SkyLight posting helper deliberately
refuses a focus-stealing fallback. This is useful for route boundaries, but is
not a native drag-and-drop solution to copy.

Sources: [event kinds](https://github.com/DioxusLabs/accessibility-cli/blob/519e0d3b4d339a3a5c1663a092d2169424b07afb/packages/accessibility-macos-sys/src/macos/types.rs#L40),
[event construction and delivery](https://github.com/DioxusLabs/accessibility-cli/blob/519e0d3b4d339a3a5c1663a092d2169424b07afb/packages/accessibility-macos-sys/src/macos/events.rs#L85).

### Pi computer use

At `4b8dbd7eaa13328ab1a8a4b55d0be0b077de7d62`, the macOS bridge defaults
to HID delivery, activates the target if needed and posts to the global HID tap.
It serializes physical input. Its optional PID route calls public
`CGEventPostToPid`; it is not a separate SkyLight drag workaround. The drag
function interpolates an incoming path with short sleeps.

Source: [delivery and drag implementation](https://github.com/injaneity/pi-computer-use/blob/4b8dbd7eaa13328ab1a8a4b55d0be0b077de7d62/native/macos/bridge.swift#L3308).

### Open Computer Use / Open Codex Computer Use

The previously reviewed `opensymph/open-computer-use` source at
`5b433b98019c18201a15d11e8c3cb0010879a3d8` has separate targeted and global
drag paths. The `iFurySt/open-codex-computer-use` checkout at
`386a260d1ab8b690adbbb27f7471595cf0c2b752` retains that split and explicitly
reports the targeted path's inability to drive window-server drag sessions.
Its global path is enabled through
`OPEN_COMPUTER_USE_ALLOW_GLOBAL_POINTER_FALLBACKS`.

The newer global implementation uses a default event source, movement deltas,
matching event numbers on newer OS versions, a 50 ms press dwell and a step count
scaled to distance. These are useful comparison points for our global provider.
The source comments' broader claims about OS requirements need independent tests;
the implementation is not evidence that the same recipe repairs SkyLight delivery.

Sources: [earlier input implementation](https://github.com/opensymph/open-computer-use/blob/5b433b98019c18201a15d11e8c3cb0010879a3d8/packages/OpenComputerUseKit/Sources/OpenComputerUseKit/InputSimulation.swift#L122),
[current drag construction](https://github.com/iFurySt/open-codex-computer-use/blob/386a260d1ab8b690adbbb27f7471595cf0c2b752/packages/OpenComputerUseKit/Sources/OpenComputerUseKit/InputSimulation.swift#L142),
[route contract](https://github.com/iFurySt/open-codex-computer-use/blob/386a260d1ab8b690adbbb27f7471595cf0c2b752/packages/OpenComputerUseKit/Sources/OpenComputerUseKit/ComputerUseService.swift#L190).

### Klyk and key-window preparation

Klyk uses `SLPSPostEventRecordTo` to make a background window key without raising
it, then attempts targeted native input. This is a candidate for controls that
require key-window state. It changes input-routing state even if the frontmost
application remains unchanged. Its current architecture describes strict
background refusal and foreground input for Chromium. Earlier indexed slider
success claims must not be generalized to file drops or Electron tab drags.

Its field table also labels target PID as 39 and pressure as 34. The local Apple
SDK defines these public fields as 40 and 2. Reuse the idea of key-window
preparation only after testing; do not copy its numeric field table.

Source: [architecture and routing](https://github.com/legetdev/klyk/blob/355b32fea8a847680e4e65540cd4bb598a9f1411/ARCHITECTURE.md).

### Codex native Computer Use and OpenCUA

A public Codex issue reports target-side missing held-button state on macOS 26.7
and 27, including a foreground comparison and successful independent HID control.
It is a user report, not a confirmed vendor diagnosis. It corroborates the
symptom, not an explanation or repair of closed native internals.

[Codex issue 43047](https://github.com/openai/codex/issues/43047).

OpenCUA's published model and evaluation material uses PyAutoGUI actions. That is
not evidence of a hidden per-window SkyLight drag implementation.
[OpenCUA model documentation](https://github.com/xlang-ai/OpenCUA/blob/main/model/README.md).

## Decisions for Unimation

1. Retain distinct targeted pointer tracking and global native drag delivery.
   Never infer native drag-and-drop support from a symbol loading, a successful
   dispatch or a slider changing value.
2. Test key-window preparation independently on an owned native fixture. Record
   frontmost PID, key-window changes, cursor position and stacking before/after.
   Do not make it an unconditional preparation step for every app.
3. Compare source-state choices, press/release dwell and movement deltas one at a
   time. Record target events and native session callbacks. A worker may own the
   gesture and cleanup, but moving code to a worker does not establish OS input
   state by itself.
4. Validate separate cases: custom mouse tracking, NSSlider, text selection,
   native same-app drop, cross-app file drop and Electron tab reorder. A result
   for one is not a capability claim for the others.
5. Verify the requested outcome before retrying. If dispatch occurred but the
   outcome is absent, report that uncertainty. Offer explicitly selected global
   input or a semantic application operation where available.

No universal background native-drag workaround was verified in this review.
No upstream code was copied into the implementation.

## Proposed stacking workaround

Temporarily lifting occluding windows A and B above an activated target C can
preserve its visual occlusion, but does not by itself route global mouse input to
C. The initial global mouse-down still needs to reach C at the source point.
An input-accepting A or B over that point can receive it instead. Later tracking
may remain with the original mouse-down recipient; native drop hit-testing is
another separate step. App activation, key-window state, stacking and pointer
routing must be measured independently.

Making both windows floating also does not freeze their relative order. Exact
levels, within-level order, existing floating windows, window ownership, Spaces,
new dialogs and concurrent user changes all matter. Foreign-window level changes
are not equivalent to setting `NSWindow.level` on our own windows.

We will not enable this workaround automatically. Making occluders ignore mouse
input, suppressing physical input, or showing click-through visual replicas would
change how the user's visible applications behave. Restoring cursor position and
focus afterward cannot undo a misdirected click or keystroke. A snapshot overlay
could conceal global automation but would still consume the shared input channel;
it is not concurrent background automation.

A bounded experiment can use only owned A/B/C test windows, recording initial
mouse-down recipient, held-button state, final drop, active PID, cursor and window
order. Test occluded and exposed source points separately. Any temporary state
must have a cleanup guard, avoid altering pre-existing floating levels, and avoid
restoring over newer user changes. This remains a proposed experiment, not a
validated provider.

For ordinary operation, prefer supported semantic or targeted input. A global
gesture must be explicitly selected and treated as exclusive shared input. Abort
on detected conflicting user input or a Space/target change, with best-effort
release; do not promise race-free coexistence or replay suppressed user input.

Reference: [Apple event dispatch and mouse tracking](https://developer.apple.com/library/archive/documentation/Cocoa/Conceptual/EventOverview/EventArchitecture/EventArchitecture.html).
