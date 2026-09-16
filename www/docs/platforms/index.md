---
title: Platforms
description: Choose native providers for desktop and mobile automation. Understand accessibility, input delivery, capture, and platform-specific requirements.
---

Use `actuate capabilities` on the selected host or device to find available
operations. Permissions, OS versions, and device connections affect the result.

| Platform | Accessibility | Input and capture |
| --- | --- | --- |
| Windows | UI Automation | Global SendInput, native capture, cursor overlays |
| macOS | Accessibility API | Quartz, SkyLight window input, ScreenCaptureKit, cursor overlays |
| Linux | AT-SPI | X11 and supported Wayland routes, compositor-dependent capture and overlays |
| Android | Device accessibility | Device HID routes over direct connections |
| iOS and iPadOS | Simulator accessibility | Simulator HID; physical-device discovery and capture |

## Windows

Inspect the actions a control advertises. WPF, WinForms, Win32, UWP, and Electron
apps expose different UI Automation patterns; the application framework alone
does not determine whether a particular control supports an action.

UIA semantic actions may activate their target. Pointer and keyboard input use
the shared desktop through SendInput. A separate Task View workspace does not
isolate Run, Start, the taskbar, or other shared shell controls from the user.

Dialogs and menus may have separate top-level windows. Discover the window and
observe its owning process. Use display-scoped cursor rendering for shell controls
that do not belong to the application window being automated.

## macOS

Grant Accessibility permission for observation and input, and Screen Recording
permission for capture. Window-directed delivery depends on the selected route
and target application. Private SkyLight APIs can vary with the OS version.

Inspect native action names such as `AXPress` instead of assuming Windows UIA
names apply. Window coordinates and captured image pixels need explicit mapping.

## Linux

AT-SPI observation requires the desktop accessibility bus and application support.
Input and capture depend on the display server. On Wayland, check the compositor's
supported protocols; X11 routes do not imply equivalent Wayland capabilities.
Hyprland has provider-specific targeting support.

## Mobile devices

Select the device explicitly with the [configuration options](/automate/configuration).
Android connections depend on pairing, credentials, and device permissions.
Apple simulator operations require an existing booted simulator and Xcode.
Physical Apple devices expose different capabilities from simulators.

See [installation](/installation) for build prerequisites and
[input and cursors](/guides/input-and-cursors) for delivery and visualization scope.
