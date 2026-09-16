# Windows host validation, 2026-09-17

This report describes the earlier isolated run. Subsequent authorized visible
tests and cursor changes are recorded in [interactive results](windows-interactive-results.md).

Tested `linux-windows` on Windows 11 Home Single Language, build 26200,
with one 1920 by 1200 display. Electron fixture version: 44.4.1.
These results cover the controls and environments below, not every application
using those frameworks.

## Foreground interference found

The initial test ran nonactivating fixtures on the user's shared desktop.
UIA Invoke on both WPF and WinForms activated the target window. The user
reported interference while working on other workspaces. This was a failed
background-safety test. Showing a fixture without activation does not prevent
its accessibility provider from activating it later.

Visible testing stopped and the fixtures were closed. No global keyboard or
mouse input was dispatched by this suite. The sampled physical pointer stayed
unchanged across the offending invokes, but foreground ownership changed.
Restoring foreground afterward does not undo the interruption and is not a
background-safety guarantee.

The UIA capability now reports `may_activate_target: true`. Its
`live_validated: true` describes the limited native tests here. The overall
session and global input retain `live_validated: false`; neither full desktop
coverage nor native SendInput consumption has been validated.

## Isolated acceptance suite

`tests/windows_e2e.py` creates a named Win32 desktop with `CreateDesktopW` and
starts its worker using native `CreateProcessW` with `STARTUPINFO.lpDesktop`.
It never calls `SwitchDesktop`. The worker checks its actual thread desktop
against the input desktop before launching fixtures. A Windows Job Object
contains the worker and descendants, closes remaining processes on exit, and
enforces a 180-second suite deadline. Fixtures are disposable and have no
connection to user documents.

Python's `subprocess.STARTUPINFO` does not marshal `lpDesktop`; assigning that
attribute silently left an early worker on `Default`. The runtime desktop
check refused to launch fixtures. The native launcher fixes this test-runner
error and keeps the runtime check.

A separate Win32 desktop is not a Windows Task View virtual desktop. Passing
here establishes action consumption in that isolated environment. It does not
establish background safety on another Task View workspace, or concurrent
physical keyboard and mouse use in an active graphical session. Foreground and
cursor queries return null on this inactive desktop; those null values are not
evidence that the user's foreground and cursor were measured as unchanged.

| Test | Result |
| --- | --- |
| WPF | UIA invoke increments a counter; ValuePattern writes and reads `Hið🦀`; checkbox state changes from 0 to 1; both application windows are discovered and observed |
| WinForms | Counter, Unicode value and checkbox state verified through UIA |
| Native Win32 | Actual STATIC, BUTTON and EDIT HWND controls; counter, Unicode value and checkbox state verified through UIA |
| Electron | Chromium accessibility enabled for this disposable fixture; HTML button, input and checkbox effects verified through UIA |
| References | Surviving button reference remains stable; short references resolve; foreign-session references are rejected |
| Protocol | Correlated requests work; unknown actions and process-directed text reject without effects; truncated observations report incomplete traversal; native diffs return |
| Overlay without shell membership | Helper stays alive, accepts commands and remains hidden on the isolated desktop |

Each action is followed by a state read or bounded polling. A dispatch receipt
alone does not pass a test. Some snapshots have incomplete traversal because
auxiliary windows expose invalid runtime IDs; their node errors remain in the
snapshot. The suite does not turn those observations into complete-tree claims.

Run from the repository:

```powershell
python scripts/prepare-deps.py
cargo build -p cli --no-default-features -p overlay
python tests/windows_e2e.py
```

For Electron, use a local fixture-only installation:

```powershell
npm.cmd install --prefix target/windows-e2e/electron --no-audit --no-fund electron@44.4.1
node target/windows-e2e/electron/node_modules/electron/install.js
python tests/windows_e2e.py --electron target/windows-e2e/electron/node_modules/electron/dist/electron.exe
```

The explicit installer command also handles environments where npm disables
dependency lifecycle scripts. Electron is optional; omission produces a skip,
not a pass. JSON transcripts, subprocess diagnostics and results are written to
the chosen `--output` directory. Host evidence is under `target/windows-e2e/`.
Do not run the fixtures directly on the user's desktop.

## Shell and broader coverage

Only read-only shell inspection continued after the interference report.

| Target or behavior | Evidence or remaining gap |
| --- | --- |
| Explorer, desktop and tray host | Explorer process observation returns partial trees after the node-error fix. Samples included File Explorer, tray overflow and shell helper windows. This does not establish complete desktop or tray controls |
| Start | Running host process had no enumerable top-level window; PID observation returned `target_not_found`. The panel was not opened |
| Search | Same limitation on the inactive Search host; no query entered |
| Quick Settings / shell experience host | Same limitation on the inactive host; no volume, network or display setting changed |
| Settings, Microsoft Store, UWP and WinUI controls | No live interaction acceptance run. Package installation alone is not test coverage |
| Overlay animation and visible pixels | Not validated. Initial shared-desktop helper exited with `TYPE_E_ELEMENTNOTFOUND` while querying shell desktop membership. It now initializes a transparent window and hides/retries unavailable membership instead of exiting; isolated hidden behavior passed |
| Task View, workspace transitions and window movement between workspaces | Not validated; no workspace switches performed for testing |
| Multiple physical displays and mixed DPI | Host has one display. Negative-origin geometry has unit coverage only |
| Window capture and occlusion | Current Windows provider captures the composed desktop with GDI. It has no WGC window capture route |
| Drags, wheel input and physical keyboard input | Global provider exists; no live consumption test on the user's desktop |
| Touch, pinch, swipe and pen | No Windows gesture provider in this branch |
| Elevated, secure, locked and protected windows | Not validated |

## Fixes from this run

- Strip the correlation ID before strict Windows request parsing and expand
  short references through the latest observation's namespace.
- Expose toggle, selection and expansion state for action verification.
- Preserve a failed native node query as a parent issue and continue traversal,
  with incomplete coverage, instead of aborting the entire shell observation.
- Keep the overlay alive and hidden while shell desktop identity is unavailable.
- Make the capture-path unit test use an absolute path valid on Windows.

Targeted Rust tests passed: 79 unit tests and one compile-fail documentation
test across the core, Windows and overlay packages. Native CLI/overlay builds
and strict Clippy also passed. The fixture suite covers four application stacks
and the overlay's unavailable-desktop behavior; visible compositor acceptance
is explicitly skipped.

## Reference review and follow-up work

Shallow, sparse source checkouts are under `target/windows-e2e/references`.
No reference project's driver was installed or executed. No upstream
implementation was copied into Unimation.

- [Cua delivery routing at f6be6008](https://github.com/trycua/cua/blob/f6be60087b8d1bb3d0a822ea11736761b8b8ace0/libs/cua-driver/rust/crates/platform-windows/src/input/delivery.rs)
  separates event families and rejects known message-delivery failures for
  particular frameworks. A `PostMessage` return is not proof of consumption.
- [Cua foreground bypass](https://github.com/trycua/cua/blob/f6be60087b8d1bb3d0a822ea11736761b8b8ace0/libs/cua-driver/rust/crates/platform-windows/src/uia/fg_bypass.rs)
  documents UIA self-activation and a temporary `EnableWindow` guard for some
  targets, with an explicit WPF limitation. Disabling another app's window is
  itself an interaction effect; this run did not adopt that guard as a universal
  fix. These are upstream observations, not validation on this host.
- [Pi Windows bridge at 4b8dbd7e](https://github.com/injaneity/pi-computer-use/blob/4b8dbd7eaa13328ab1a8a4b55d0be0b077de7d62/docs/windows-bridge.md)
  distinguishes semantic actions, raw-input policy, value read-back and separate
  menu roots. Its per-root design is relevant to shell popups and dialogs.
- [Open computer use at 5b433b98](https://github.com/opensymph/open-computer-use/blob/5b433b98019c18201a15d11e8c3cb0010879a3d8/apps/OpenComputerUseWindows/native_actions.go)
  explicitly gates foreground actions and ValuePattern text fallback. Its
  runtime-ID/name fallback is not adopted as exact-reference identity here.
- [Microsoft winapp UI automation](https://learn.microsoft.com/en-us/windows/apps/dev-tools/winapp-cli/ui-automation)
  documents window capture, semantic patterns and foreground touch/drag routes.
  Those foreground routes cannot supply a nonintrusive background guarantee.

Next acceptance work needs a dedicated interactive Windows test session or VM
where foreground actions, UWP activation and workspace switching cannot
interrupt the user's desktop. Test Task View and mixed-DPI layouts there.
Keep semantic actions, per-window messages, global input, shell discovery,
capture and gesture injection as separate providers with explicit effects.
In particular, background policy must be checked for UIA actions as well as
raw input. Adding private APIs alone does not establish that policy.
