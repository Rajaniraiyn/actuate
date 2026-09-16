# Windows and Linux acceptance runs

The first Windows/Linux implementations are built and checked from macOS. Compile
checks cannot verify a desktop session, permissions, app event handling or pixels.
Use the platform guides for the exact supported operations and known gaps.

## Build and collect observations

Prepare the existing patched dependencies before Cargo resolves the workspace:

```sh
python3 scripts/prepare-deps.py
cargo build -p cli --no-default-features
cargo run -p cli --no-default-features -- capabilities
```

`--no-default-features` isolates the host desktop backend from mobile transports.
`native` selects Windows on Windows and Linux on Linux. Neither backend starts
another OS, installs a desktop service or automatically chooses a global fallback
for a rejected targeted action.

Run from the logged-in desktop session. On Windows, the binary is
`target/debug/unimation.exe`. For Linux X11, use the logged-in user's DISPLAY and
Xauthority. A Wayland session must not be presented as a complete X11 desktop.

```sh
python3 tests/desktop_smoke.py --binary target/debug/unimation --pid 1234 --capture
```

Substitute a real test-app PID. The script reads capabilities, applications,
windows, displays, two accessibility snapshots and their diff through one persistent session.
It optionally saves a screenshot. It does not inject keyboard or mouse input.
Inspect the printed evidence directory. Native operation errors produce a failing
exit code and remain in the transcript, including intentionally unsupported routes.
A received result is not proof of complete or accurate native data.

## Manual action matrix

Use disposable documents and test-app windows. Keep a session open so references
remain valid, and obtain coordinates from current geometry. Record the request,
response, before/after observation and expected result for each case.

| Case | Evidence required |
| --- | --- |
| Native button and checkbox | Advertised action, exact reference, observed state change |
| Text field | Unicode roundtrip, focus recipient, no silent clipboard replacement |
| Browser or Electron content | Native tree coverage and unsupported elements |
| Scroll | Declared units, actual movement, new snapshot and diff |
| Repeated snapshot | Identity retained for surviving elements, revision advanced |
| Closed/replaced control | Stale reference rejected, no label-based rebinding |
| Dialog/modal | Actual active target and blocked underlying controls |
| Two monitors and mixed DPI | Bounds, screenshot dimensions, verified click positions |
| Drag | Correct down recipient, held state, release and actual drop |
| Missing permission/display/session | Structured error before input, no fallback escalation |
| Partial input failure | Unknown effect and documented release cleanup |

Separate slider movement, text selection and native drag-and-drop results. Do not
mark all dragging supported after one succeeds. Restore any intentionally changed
app state after the run. Do not suppress or replay the user's physical input.

## Feedback to implementation

Classify each result as implemented-and-observed, dispatched-but-unverified,
unsupported, or failed. Keep the OS/session type, display configuration and app
version beside the evidence. Agent usability feedback should include ambiguous
output, repeated discovery, missing fields, excessive steps and unclear errors.
Record impossible-with-current-provider cases as missing capabilities rather than
retrying coordinates or changing delivery routes silently.


## Checks completed on the macOS development host

- `cargo test --workspace --lib` passed. Platform-gated Windows/Linux native
  modules do not run under this host test command.
- Windows and Linux native providers and the desktop CLI passed cross-target
  checks with `--no-default-features`. Provider test targets were type-checked;
  this did not execute their native tests or link runnable foreign binaries.
- Native-provider Clippy checks passed with warnings denied.
- The existing provider CLI contracts and the presentation CLI tests passed,
  including native Windows/AT-SPI field rendering and querying.
- The read-only smoke protocol ran against the macOS provider. Its subprocess,
  reply-ID and snapshot handling also ran against a disposable fake session.

No live Windows or Linux UI acceptance result is claimed by these checks.

## Overlay acceptance

Build the separate visual helper with `cargo build -p overlay`. Start
`target/debug/unimation-overlay` or `target/debug/unimation-overlay.exe` and keep
its stdin open while sending one command per line. It displays feedback only;
`click` in this protocol does not click an application.

```json
{"op":"move","x":200,"y":200,"duration_ms":500}
{"op":"click","x":200,"y":200}
{"op":"scope","scope":{"kind":"window","window_id":12345,"pid":1234}}
{"op":"move","x":250,"y":250,"duration_ms":500}
{"op":"hide"}
{"op":"quit"}
```

Use a real target's native window ID and PID, and coordinates from that provider.
Confirm that the overlay receives no clicks or focus. Cover the target with another
window, move and resize it, minimize it, switch workspaces, move across displays,
and close it. The overlay must follow or hide without raising the target. Test
mixed DPI separately. Poll-based ordering may lag native compositor transitions;
record those failures instead of treating compilation as proof of attachment.

See [Windows](windows-overlay.md) and [Linux](linux-overlay.md) for platform limits.
Native Wayland overlay startup currently reports unsupported. Its separate
[input portal guide](wayland.md) describes consent and persistent-session tests.
