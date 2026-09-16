# Android cursor capture

Native HID pointer input and pointer visibility in a screenshot are separate
capabilities. On the tested Samsung Android 16 device, hover highlights change but
the cursor arrow was absent from the captures examined. A later Sprite dump
showed near-zero alpha, so those captures alone do not distinguish cursor fading
from screenshot exclusion.

Android 16's [screencap command](https://android.googlesource.com/platform/frameworks/base/+/refs/tags/android-16.0.0_r1/cmds/screencap/screencap.cpp)
has no include-cursor option. Cursor surfaces can be marked `eSkipScreenshot` in
[SpriteController](https://android.googlesource.com/platform/frameworks/base/+/refs/tags/android-16.0.0_r1/libs/input/SpriteController.cpp).
The [capture arguments](https://android.googlesource.com/platform/frameworks/native/+/refs/tags/android-16.0.0_r1/libs/gui/aidl/android/gui/CaptureArgs.aidl)
do not provide a cursor-specific override.

## Annotation boundary

An optional screenshot annotation should compose with any capture backend, retaining
its original image. It needs a cursor observation with:

- Position, display identity, coordinate basis and timestamp.
- Provenance: observed native position or explicitly supplied annotation position.
- A validated mapping into the captured image, including crop and scaling.
- Cursor hotspot, visibility and held-button state when known.

Unknown native position must remain unknown. Relative HID deltas are not absolute
coordinates: Android applies pointer acceleration and clips at display edges.
A caller-supplied marker can still be useful, but must be labelled as a marker,
not as an observed native cursor. Stale positions and mismatched display mappings
must not silently become precise-looking overlays.

Standard Android 16 [MouseCursorController dumps](https://android.googlesource.com/platform/frameworks/base/+/refs/tags/android-16.0.0_r1/libs/input/MouseCursorController.cpp)
do not include current x/y. The native cursor-position method is exposed through
in-process InputManagerInternal, not the ordinary input shell command or public
Binder interface. SurfaceFlinger `Sprite` layer bounds are a possible diagnostic,
but require verified hotspot, parent transforms and display mapping before reuse.

This is a design boundary and source audit. Automatic native cursor annotation is
not implemented. Screenshots taken immediately after a dispatched HID report may
precede the UI update; an ADB acknowledgement does not establish visual completion.
