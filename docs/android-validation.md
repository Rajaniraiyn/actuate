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
