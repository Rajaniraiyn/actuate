# macOS drag investigation

On 2026-09-16, the same VS Code editor-tab drag failed through targeted SkyLight
input and succeeded through explicit foreground Quartz input. Repeated coordinate
adjustments did not establish the cause. A native AppKit probe isolated event
construction from application behavior.

## Observed defects and changes

- Private integer field 58 was treated as a gesture identifier. On this host it
  changed `NSEvent.timestamp` to `0.000000001`. Use public
  `CGEventField::MouseEventNumber` for event numbering instead. A regression test
  checks that routing leaves the timestamp intact.
- Prepared drag events had no explicit movement deltas. They now carry deltas
  between consecutive rounded desktop positions.
- Events now receive delivery-time uptime timestamps, using `CLOCK_UPTIME_RAW`.
- Relative sleeps added validation and dispatch overhead to the gesture duration.
  The loop now waits against cumulative monotonic deadlines.
- An interrupted drag previously released at its planned destination. Cleanup now
  releases at the last dispatched position with a fresh timestamp. Cleanup is
  best effort; the error still reports an unknown effect.

## Remaining native drag limitation

The probe logs received events, `NSEvent.pressedMouseButtons`, native dragging
session callbacks and drop coordinates. Before the fixes, SkyLight packets had
near-zero timestamps. With corrected timestamps and deltas, SkyLight still
reported `pressedMouseButtons == 0`, and native dragging sessions ended near the
source while further drag packets arrived. Quartz reported a held button and
completed one continuous session at the intended local destination, `(395, 120)`.

This is evidence that per-process event delivery does not establish the button
state this native dragging session needs. It does not establish that all controls
reject targeted dragging. The slider experiment did not demonstrate a successful
slider drag and must not be counted as support.

Keep targeted delivery and global input separate. Do not silently post global
button events or duplicate packets through multiple routes to repair a background
gesture. That changes the shared pointer/focus contract. A dispatch receipt means
packets were sent, not that the application accepted a drop. Verify the resulting
layout or another application-specific postcondition before retrying.

## Repeating the probe

Build and run `scripts/macos/drag-probe.m` on macOS:

```sh
clang -fobjc-arc -framework AppKit -framework ApplicationServices \
  scripts/macos/drag-probe.m -o /tmp/unimation-drag-probe
/tmp/unimation-drag-probe
```

Read the printed PID and window ID, discover the current window bounds, and use
one persistent Unimation session. Drag the blue rectangle to the green rectangle
first through `skylight_pointer`, then through explicitly selected global
`pointer` delivery with the probe foreground. Convert AppKit content coordinates
to current desktop coordinates; do not reuse coordinates after moving the window.
Compare button state, native session count and final drop position. The retained
probe only starts a native drag after a press inside the blue source rectangle.

The packet regression test does not validate native drag-and-drop. Display/Space
changes, process termination and native session acceptance require live tests.
