---
title: Platforms
description: Compare native providers for Windows, macOS, Linux, Android, and Apple devices. Review input routes, prerequisites, and current validation limits.
---

| Platform | Implemented routes | Current limits |
| --- | --- | --- |
| Windows | UI Automation, native input and capture, cursor overlays, shell controls | Semantic actions can activate targets. Full shell and workspace coverage remain incomplete. |
| macOS | Accessibility, Quartz, SkyLight window input, ScreenCaptureKit, cursor overlays | Private APIs and window attachment need OS-specific validation. |
| Linux | AT-SPI, X11 input, supported Wayland input and capture, Hyprland targeting, overlays | Compositor protocols determine availability. Universal portal input is not implemented. |
| Android | Direct device connections, accessibility and HID routes | Pairing, permissions, and device support vary. |
| iOS and iPadOS | Simulator accessibility and HID; physical-device discovery and capture | Simulator and physical-device capabilities differ. |

Query `actuate capabilities` on the selected host or device. An implementation
does not imply that every app, OS version, display arrangement, or permission
configuration has been tested.

## Windows validation

Tests cover WPF, WinForms, Win32, and Electron fixtures, plus native Save/Open
dialogs. Live checks include Calculator, Settings, shell controls, soft cursors,
taskbar auto-hide, and Explorer file transfers.

Multi-display and workspace transitions, elevated dialogs, network and cloud
files, and some shell interactions still need coverage. See the
[Windows results](https://github.com/Rajaniraiyn/actuate/blob/docs/_specs/windows-interactive-results.md).

## Implementation notes

Read the [backend guide](https://github.com/Rajaniraiyn/actuate/blob/docs/_specs/backends.md)
and [validation records](https://github.com/Rajaniraiyn/actuate/blob/docs/_specs/validation.md)
for provider-specific requirements and evidence.
