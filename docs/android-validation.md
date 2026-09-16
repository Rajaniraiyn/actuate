# Android device validation

Validated on a Samsung SM-S931B running Android 16, SDK 36, over paired wireless
ADB. No host adb executable/server or helper APK was used. Android changes are
still experimental.

## Observed behavior

- Native code pairing, pinned-key reconnection and exact-device mDNS discovery.
- Device information and launcher-activity inventory.
- Home followed by an upward swipe opened the app drawer, confirmed by screenshot.
- Explicit Calculator launch, then coordinate taps for `2 + 3 =`, produced `5`.
- Back navigation, Settings launch, list scrolling and return to Home were
  confirmed with screenshots. Launching an existing app may resume its current
  screen; it does not guarantee the app's root screen.
- Five consecutive screenshots on one connection completed after fixing a
  protocol receiver bug. Android sent separate stdout and exit packets before an
  ACK; bounded buffering now accepts that observed sequence.
- The built-in `hid` utility registered a native mouse in InputReader. Relative
  movement, primary/secondary/middle button transitions, wheel reports and clean
  teardown completed. No helper was uploaded. Screenshot capture does not prove
  whether Android included its pointer layer in the image.

The pointer adapter is currently a Rust-library API, separate from the CLI's
ordinary touch session. It exposes relative counts, held-button state and wheels;
it does not claim a mapping from mouse deltas to screenshot pixels. Samsung's
InputReader heading differs from AOSP, and both forms have regression coverage.

## Remaining validation

USB hardware, live QR scanning, other Android vendors, HID keyboards and
accessibility observation remain unvalidated or unimplemented. Successful command
receipts alone are not evidence of a resulting UI state. Input errors with unknown
effects are not automatically replayed.

## Build and dependency checks

Workspace tests, CLI contract scripts and Clippy passed. Protocol regressions
cover legacy packet bursts, receive-byte limits and empty-packet floods. The
patch preparation step was checked for reproducibility, no-network reuse and
preservation of local source edits. A `cargo vendor --locked` snapshot resolved
successfully using offline Cargo metadata; the temporary snapshot was removed
following validation.


## Theme round trip

A sub-agent opened Settings and used only native HID wheel, relative hover and
primary clicks to navigate to Display and select Light. Dark was initially selected;
Android's read-only night-mode query confirmed the initial and intermediate states.
The Settings process was then closed and the HID device torn down.

A separate touch-input pass cold-launched Settings, swiped the list, tapped Display,
and tapped Dark. Screenshots confirmed Light before the final tap and Dark afterward.
No direct theme-setting command was used. The original theme is restored.

HID screenshots required time for the UI to settle after dispatch. Row hover
highlights and diagnostic Sprite geometry helped target the HID clicks. Standard
captures did not reliably include the native arrow; see
[cursor capture limitations](android-cursor-capture.md).


The shared smooth-motion example also completed on the phone, with a 650-ms
curved relative path, button transitions, wheel input and screenshot capture over
one retained ADB connection. The observer borrowed the live pointer transport;
no second observer connection was opened. This validates dispatch and reuse, not
frame-by-frame motion timing or exact screen displacement.


## HID-only round trip on one session

A subsequent sub-agent test navigated Display settings with HID wheel input,
shared curved motion and primary clicks, changing Dark to Light and back to Dark.
The actual round trip retained one ADB connection and one HID device; screenshots
and read-only theme queries shared that same connection. Screenshots and night-mode
queries confirmed `yes -> no -> yes`. Explicit HID cleanup completed and the
process exited successfully. The phone was left on Display with Dark selected.

## Native pointer feedback validation

On SM-S931B, native Pointer Location reported `(644.2, 698.0)` while
SurfaceFlinger cursor metadata decoded to `(644.1592, 698.0371)`. The
hotspot was `(2, 2)` relative to the sprite origin. The temporary diagnostic
setting was restored to its original value, zero.

`cursor::observe` reads actual native hotspot coordinates on the existing
connection. `Pointer::move_to_observed` uses bounded corrections and requires
two observations within two pixels before returning. It checks display
identity and geometry on each observation. It does not convert relative HID
counts into pixels or infer position from an animated path. The observation
adapter currently accepts only full, unrotated, unscaled physical displays.

Live targets and measured endpoints in original 1080 × 2340 display pixels:

| Target | Observed endpoint |
| --- | --- |
| 400, 600 | 399.166, 600.040 |
| 700, 800 | 699.282, 799.098 |
| 300, 900 | 301.168, 900.157 |
| 170, 1445 | 169.483, 1445.268 |

One primary click at the last endpoint entered `7` in Calculator, verified
by screenshot. This validates movement and one click, not arbitrary drag
paths, scroll destinations, rotated displays, or other OEMs. The controller
retains held buttons for drag movement; callers must release buttons on
failure. Application hit testing and completion still require observation.
Native cursor idle movement remains disabled pending feedback-aware anchor
restoration. Opposite relative deltas do not guarantee a return to origin.


## Coordinate audit across providers

Relative HID counts are not screenshot pixels on Android or the iOS simulator.
Acceleration depends on movement and timing, so a smooth path's summed counts
cannot establish its screen endpoint. The Android feedback controller observes
the native hotspot after corrections; this does not establish equivalent
feedback support for the iOS mouse provider.

Android's ordinary tap and swipe commands currently target the default display
with caller-supplied coordinates. Capture reports `click_mapping: null`; there
is no general screenshot-to-input mapping for rotation, display overrides or
external displays. The iOS simulator similarly keeps screenshot pixels, AX
coordinates and unrotated framebuffer touch ratios separate and reports no
implicit capture-to-input mapping.

The macOS image-click path checks the retained frame's action epoch and current
geometry, then scales each axis independently and includes the display or
window origin. Unit tests cover unequal axis scales, negative display origins
and stale geometry. This audit found no shared arithmetic offset in that path;
it does not constitute new live validation of macOS, iOS, Windows or Linux.

## Unaccelerated HID session and theme roundtrip

A second implementation, `hid::LinearPointer`, temporarily disables
`mouse_pointer_acceleration_enabled` and sets `pointer_speed` to -7. These
are current-user settings affecting all mice, so this route is explicit opt-in.
It preserves the original values, including absent settings, and restores them
on checked `close`. Drop attempts restoration, but cannot recover from process
termination or a lost device connection. External setting changes are reported
and not overwritten. No additional dependencies or phone helper were added.

Why both settings: disabling acceleration removes velocity-dependent gain;
pointer speed still supplies a fixed scale. AOSP's flat-curve gain differs
from the Samsung implementation tested here, and display scaling contributes
too. Never hardcode that scale from one device or an AOSP formula.

At speed zero with acceleration disabled, 100/50 HID counts moved the pointer
by approximately 306.238/153.125 display pixels. Reversing the same counts at
a different cadence returned within 0.006 pixels. At speed -7, the library's
four signed axis probes, alternating 100 ms and 400 ms paths, measured
0.3062439 pixels per count in both axes. Calibration rejects cadence-dependent
gain, cross-axis motion, out-of-range gain, and failure to return to origin.
It requires room for the hover probes and no held buttons.

The September 16 live run used one paired TLS connection and one HID pointer
for the complete Settings traversal, wheel scrolling, and Dark → Light → Dark
roundtrip. Each click followed a smooth path and native endpoint verification.
No coordinate retargeting or repeated clicks were needed.

| Action | Requested point | Observed hotspot |
| --- | --- | --- |
| Open Display | 500, 1530 | 499.9248, 1530.1074 |
| Select Light | 291, 827 | 291.06445, 826.9795 |
| Restore Dark | 790, 827 | 789.9326, 826.9795 |

Screenshots confirmed both radio selections. Read-only `cmd uimode night`
reported `yes`, `no`, then `yes`. Checked session close restored acceleration
to absent and pointer speed to zero, verified by separate reads. Evidence
files are `/tmp/unimation-linear-light-selected.png` and
`/tmp/unimation-linear-dark-restored.png` on the development host.

### Primary-source findings

- [Android InputSettings](https://android.googlesource.com/platform/frameworks/base/+/android16-release/core/java/android/hardware/input/InputSettings.java): the acceleration toggle is feature-gated, so writing a setting alone does not establish support.
- [CursorInputMapper](https://android.googlesource.com/platform/frameworks/native/+/android16-release/services/inputflinger/reader/mapper/CursorInputMapper.cpp): disabled acceleration selects a flat curve; pointer capture is a separate mode that disables scaling and changes input semantics.
- [AccelerationCurve](https://android.googlesource.com/platform/frameworks/native/+/android16-release/libs/input/AccelerationCurve.cpp): pointer sensitivity still affects flat-curve gain.
- [EventHub](https://android.googlesource.com/platform/frameworks/native/+/android16-release/services/inputflinger/reader/EventHub.cpp): normal cursor classification requires mouse buttons and relative X/Y axes. Changing the HID descriptor to absolute coordinates is not an equivalent native mouse route.
- [scrcpy mouse modes](https://github.com/Genymobile/scrcpy/blob/master/doc/mouse.md): absolute SDK injection and relative physical mouse simulation are distinct backends. Keep both kinds of route composable rather than disguising one as the other.

This validates one phone in portrait mode. Wheel values remain wheel counts,
not content pixels. Rotated displays, other OEMs, and application-specific
drag behavior need their own validation. Ordinary `Pointer` remains available
without changing user settings; `LinearPointer` is a separately selected route.
