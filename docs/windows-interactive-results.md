# Windows interactive results

These tests ran on the Windows host after the user explicitly made the shared
session available. They exercise the compiled `unimation` and
`unimation-overlay` executables. Python launches fixtures, sends JSON commands,
and checks effects; production input uses native Windows APIs.

## Verified in this session

- WPF overlay animation, following window movement, occlusion, rectangular edge
  clipping, minimize/restore, wrong-owner rejection and physical click-through.
- Calculator UIA arithmetic, 2 + 3 = 5; Quick Settings Wi-Fi details navigation;
  Run text entry and cancellation; classic Mouse Properties Hardware selection.
- Settings About observation and Store Apps navigation. Opening Start succeeded,
  but its foreground Search HWND exposed only the search box. This is not full
  Start-menu coverage.
- Taskbar auto-hide: soft-cursor movement to the edge left it hidden; physical
  movement revealed it. The original taskbar flags were restored.
- Owned tray menu: the purple cursor appeared over the menu. UIA exposed the
  legacy ToolStrip root without items; native keyboard navigation invoked the
  owned counter action, whose result was read back. This does not validate UIA
  invocation of arbitrary tray menus.
- File and folder moves between two Explorer windows. The successful recording
  run verified destination bytes, SHA-256 hashes and removal of source entries.
  Only disposable files under `target/windows-interactive` were transferred.
  Evidence: `target/windows-interactive/explorer-clean-recording/results.json`.

The user confirmed that only the purple cursor was visible during cursor-hiding
tests. GDI screenshots omit the physical cursor, so screenshots alone cannot
prove its absence. Other attempts were interrupted by recording controls or
foreground windows; those failures remain in their evidence directories.

## Fixes and reusable behavior

Window-scoped UIA observation accepts an explicit HWND and validates its owner.
`uia_roots` provides bounded discovery through UIA's desktop children, including
roots that desktop-only `EnumWindows` can omit. Root discovery is not a complete
desktop snapshot. `inspect` uses the same physical-coordinate DPI context as
snapshot and reports UIA's optional `clickable_point`.

Explorer's row bounds can extend into a clipped area. Its clickable point can
also identify a selection area rather than a drag handle. The Explorer test
uses the visible Name cell, clips it to Items View, selects the item, and starts
within the text inset. A successful input receipt alone never passes a transfer.
One Ctrl-drag moved instead of copying; modifier-dependent copy behavior remains
unverified. The successful recording test requests an ordinary move.

All three desktop renderers now use shared ripple geometry, timing and cursor
color. Shared idle settings accept `wave` as well as `bob` and `off`; reduced
motion disables these decorations. Windows rendered these changes live.
macOS and Linux runtime behavior has not been retested on this Windows host.

## Native Save/Open dialogs

`crates/windows/examples/windows_dialog_fixture.rs` is a compiled Rust fixture
using Microsoft's [IFileSaveDialog](https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nn-shobjidl_core-ifilesavedialog)
and IFileOpenDialog. It restricts file reads and writes to the supplied disposable
directory and excludes selections from Recent documents. This fixture does not
add an application-specific route to the automation provider.

The visible Unicode Save/Open case passed. Other visible attempts were interrupted
by workspace switching, so their screenshots are not acceptance evidence for
modal behavior. The complete suite then passed through UIA on a private inactive
Win32 desktop, without global pointer or keyboard input:

| Case | Verified effect |
| --- | --- |
| Unicode Save and Open | Saved bytes match; reopening returns those exact bytes |
| Closed dialog reference | Inspection rejects the old reference with no effect; the session continues to operate |
| Decline overwrite, then cancel | Original contents remain intact and the fixture reports cancellation |
| Accept overwrite | Contents change to the fixture payload only after confirmation |
| Invalid filename, then cancel | Native error prompt appears; no target file remains |
| Nested path and default extension | Unicode filename is saved in the existing nested folder with `.txt` appended |
| Missing Open file, then cancel | Native error prompt appears; no file is created or opened |

Evidence is in `target/windows-e2e/dialogs-final/results.json`. To repeat:

```powershell
cargo build -p windows@0.1.0 --example windows_dialog_fixture
python tests/windows_dialogs.py --isolated
```

The harness can also run visibly with `--interactive` when the shared session is
available. It waits for controls to appear, distinguishes the filename edit from
the same-named combo box, and observes modal results before proceeding. It never
retries an action whose effect is uncertain. Shell parsing uses an ordinary
absolute path; containment checks use a separate canonical path because the
Shell rejected the canonical `\\?\` form during fixture development.

Network and disconnected paths, ACL denial, read-only and locked files, cloud
placeholders, long paths, multiselect, localized labels, elevated targets and
secure-desktop prompts remain untested. The fixture results do not establish
support for custom application dialogs that expose different accessibility
providers or implement their own file handling.

## Explicit cursor policies

The Windows session starts its native helper with independent tracking and
physical-cursor policies:

```json
{"op":"cursor_overlay","action":{"kind":"start","executable":"D:/unimation/target/debug/unimation-overlay.exe","physical_cursor":"hide_while_visible","tracking":"physical_pointer"}}
{"op":"cursor","command":{"op":"scope","scope":{"kind":"desktop"}}}
{"op":"cursor","command":{"op":"show"}}
{"op":"cursor_overlay","action":{"kind":"stop"}}
```

Defaults are `preserve` and `commands`. `physical_pointer` tracks the actual
pointer at the renderer's refresh cadence, including native drags.
`hide_within_scope` preserves the physical cursor outside the attached window;
`hide_while_visible` requests session-wide suppression while the soft cursor
is presented. Neither policy disables mouse input.

Windows uses [MagShowSystemCursor](https://learn.microsoft.com/en-us/windows/win32/api/magnification/nf-magnification-magshowsystemcursor).
A separate native guard restores visibility on pipe EOF or a missed 500 ms
heartbeat. Ordinary exit was exercised; forced-crash restoration still needs an
acceptance test. This API changes shared visibility without a reference count;
concurrent cursor-hiding applications are not coordinated. The implementation
does not replace cursor schemes. Non-Windows controller starts reject these
optional policies rather than silently claiming support.

Shell popups may lack application-view workspace identity. Explicit desktop
scope can present over those popups; window scope retains its membership checks.
This distinction must not be mistaken for native popup attachment.

## Remaining acceptance work

Explorer tab-to-tab drag, Settings Start-to-taskbar pinning, Task View transitions,
mixed-DPI multi-display behavior, gestures and full shell accessibility coverage
remain outstanding. UIA can activate its target. The tested cases do not establish
universal background safety or complete support for any application framework.

To repeat the clean recording with ordinary windows minimized:

```powershell
python tests/windows_drag.py --interactive --clean-background --lead-in 10
```

This option intentionally leaves those windows minimized. The script closes its
owned Explorer windows, restores cursor visibility and retains evidence files.

Before commit, the native core, Windows and overlay checks passed 83 unit tests
and one documentation test. Strict Clippy and the native CLI/helper build passed.
The isolated suite also verifies that HWND scope retains native traversal errors
instead of claiming that scoping makes a provider's tree complete.
