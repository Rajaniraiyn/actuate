# SkyLight window input

`SkyLightInput` loads `SLEventPostToPid`, `CGEventSetWindowLocation`, and `SLEventSetIntegerValueField` at runtime. If any required symbol is absent, construction fails before input. The framework handle stays loaded while its function pointers are usable.

Each request supplies a PID, native WindowServer window ID, desktop point, and window-local point. The caller must obtain both points from the same window geometry observation and check native window ownership immediately before dispatch. Local coordinates start at the window's top-left corner. Desktop coordinates use Quartz points, including negative origins on secondary displays.

The provider sends a move before a click or wheel event to initialize the receiving window's pointer tracking. It stamps the target PID, window number, both windows-under-pointer fields, and one gesture ID. Clicks carry the selected button, click count, and down/up pressure. Scrolls have an explicit location and pixel units. Each event is posted once, through SkyLight only.

The sender does not activate an application or warp the system cursor. A target application can change focus in response to an event. Dispatch provides no acknowledgement of consumption. Callers need a fresh accessibility observation or screenshot to verify the requested outcome.

The input sequence allocates every event before sending the first event. It uses short gaps between events for application tracking loops. These gaps are compatibility pacing and do not establish that the target is ready.

## Evidence and limits

The symbol names and window-routing fields were checked against [Cua's private API bridge](https://github.com/trycua/cua/blob/5fcd67326dd0406bca823e5488cf39f47491c869/libs/cua-driver/rust/crates/platform-macos/src/input/skylight.rs), [Cua mouse input](https://github.com/trycua/cua/blob/5fcd67326dd0406bca823e5488cf39f47491c869/libs/cua-driver/rust/crates/platform-macos/src/input/mouse.rs), and [Dioxus mouse events](https://github.com/DioxusLabs/accessibility-cli/blob/a8464024f84a0c59b769dc526555037e6227fce5/packages/accessibility-macos-sys/src/macos/events.rs). Unimation does not copy their implementation or assume their compatibility claims hold on every macOS version.

Keyboard authentication is not implemented. The inspected Cua implementation extracts a pointer from guessed offsets inside an opaque `CGEvent`. A non-null pointer does not establish a valid event-record layout, so this provider does not perform that memory access. Native keyboard authentication needs a separately validated layout or an accessor with a known signature.

Focus-without-raise is a separate capability. It changes AppKit/key-window state and must not be hidden inside this provider. Chromium-specific synthetic touch subtypes and off-window activation clicks are also separate compatibility policies; this provider does not silently attempt them after an unverified click.

## Host validation

`python3 tests/skylight_e2e.py` exercises the native fixture with a foreground setup and then with Calculator active. The input calls themselves do not request activation. Live checks observed button results, both scroll axes in both directions, stable references, unchanged shared cursor coordinates, and unchanged set of applications reporting `AXFrontmost`. Auxiliary UI services can also report this attribute, so the test does not infer a unique frontmost PID. Single, double, and triple clicks and a Shift-modified click were consumed.

The full three-part live suite passed on the host after these checks were added.

The test also compiles a temporary native event probe to inspect received events. It verifies native click counts, the Shift flag, right/middle down and up pairs, and drag motion followed by release. This Swift program is a disposable test fixture, not a backend dependency.

A generic NSView with the default `acceptsFirstMouse` behavior ignored a background click in the probe. Explicitly activating the probe before the test allowed the event sequence to work. This is a concrete limit of the no-activation route; it must not trigger a hidden activation retry.

Modifiers are event flags. They do not press physical modifier keys or claim keyboard-state equivalence. Drag requests interpolate desktop and window-local points together, allocate the release before dispatch, and require a duration between 16 and 10000 milliseconds. Window geometry must remain stable during that gesture.
