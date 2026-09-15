# iOS and iPadOS Simulator

The `ios` crate runs on a macOS host and connects to one explicitly selected simulator.
iPhone and iPad use the same backend; the runtime's device family determines the guest UI.
This does not implement automation of a physical iPhone or iPad.
See [provider composition](composition.md) for the typed embedding and transport boundaries.

## Installed runtimes and isolated devices

`unimation ios simulators list` returns CoreSimulator's native JSON for installed devices, device
types and runtimes. `--device-set /absolute/path` selects an isolated set. The executable
comes from the installed CoreSimulator framework, bypassing Xcode's first-launch wrapper.
No backend operation downloads a runtime.

The library exposes `ios::simulator::Simctl` with `create`, `boot`, `boot_status`,
`shutdown`, `delete`, `launch` and `screenshot`. Device operations require a UUID;
ambiguous aliases such as `booted` and `all` are rejected. `with_set` selects a caller-owned
set and `with_timeout` sets a subprocess deadline. Timeouts after possible mutation have
unknown effects; inspect the device before retrying.

For disposable testing, create a temporary directory, create devices inside that set,
and use the returned UUIDs. In cleanup, shut down and delete those devices before removing
the owned directory. Never run deletion against the user's default set. The library does
not perform deletion in `Drop`, where errors would be difficult to report.

`python3 tests/ios_simulator_e2e.py` follows this lifecycle for one iPhone and one iPad
using an available installed iOS runtime. It installs no guest applications. First boot
still initializes guest data temporarily; the test removes its devices and directory.

## Typed sessions and JSONL

```sh
unimation --provider ios --device DEVICE_UUID --device-set /absolute/device/set session
```

Rust callers use `ios::session::connect` and typed `Session::execute` directly.
`ios::jsonl` owns the optional agent transport; there is no session implementation in the
umbrella CLI. The platform-owned `unimation-ios session` binary is available with the
iOS crate's optional `cli` feature.

Each line is one request. Replies contain `result` or a structured `error`. Optional `id`
values are echoed. Unknown fields and unsupported operations are rejected. The device
must already be booted; connecting does not implicitly create or boot another device.

```json
{"id":1,"op":"capabilities"}
{"op":"launch","bundle_id":"com.apple.Preferences"}
{"op":"observe","max_nodes":1000,"max_depth":30}
{"op":"snapshot","format":"text","options":{"actionable_only":true}}
{"op":"semantic","target":"@e7","action":{"kind":"perform","name":"AXPress"}}
{"op":"observe"}
{"op":"diff_view","before":1,"after":3}
{"op":"capture","path":"/absolute/new-screenshot.png"}
```

References and revisions in this example are illustrative. Always use the actual returned
reference and revision. Raw observations remain available beneath text/compact projections.
The session retains its latest 32 observations. `view`, `query`, `diff` and `diff_view`
operate on those revisions with the same core query/diff/presentation functions as macOS.
A short `@eN` target resolves only within this session. Full `{session,id}` references are
also accepted. Navigation completion must be verified through observation.

Touch, hardware keys and keyboard requests are explicit:

```json
{"op":"button","button":"home"}
{"op":"touch","action":{"kind":"tap","point":{"x":0.5,"y":0.5}}}
{"op":"touch","action":{"kind":"swipe","from":{"x":0.98,"y":0.001},"to":{"x":0.98,"y":0.55},"duration_ms":600,"edge":"top"}}
{"op":"hid_key","usage":4,"modifiers":[]}
```

These touch ratios address the raw unrotated framebuffer. They are not AX points.
Keyboard usages belong to USB HID page 0x07; guest keyboard layout and capitalization
can change the resulting text. Hardware controls have real guest effects.

`observe` and `snapshot` optionally accept `scope`, defaulting to `"frontmost"`.
`{"point":{"x":100,"y":100}}` selects a native AX hit-test subtree in translator
coordinates. `{"application":{"pid":123}}` attempts an explicit guest application
translation. These are separate observation requests, not implicit fallback routes.
The SpringBoard PID translation returned no native root on the validation host.
The native point scope did return a home-icon container containing Settings, which a
semantic press successfully opened. This is a hit-tested subtree, not a full-display root.
A frontmost result may identify a service such as DockFolderViewService and omit home
icons; it must not be presented as a complete display inventory.

## Native bridge and ownership

`SimulatorAccessibility::connect(udid, device_set_path)` loads CoreSimulator and
AccessibilityPlatformTranslation. An `AXPTranslator` delegate routes requests by a unique
bridge token to the selected `SimDevice`. The delegate preserves the translator's frame
coordinates; they are tagged `ax_translator_system`, not assumed to be screenshot pixels.

`observe_frontmost` translates the foreground guest application's accessibility elements.
The provider retains elements and reuses references according to native object equality.
It does not guess identity using labels, tree positions or screen coordinates. Advertised
native actions implement the portable `SemanticActions` trait. `AXPress` uses the native
press method. Unsupported setters return no-effect errors; no pointer fallback occurs.

The response queue and device remain alive for outstanding callbacks after a timeout.
A five-second bridge response deadline applies per callback, not to the entire observation
or every synchronous private-framework call. A native crash can still terminate the host;
worker-process isolation remains future work. Keep the provider on its owning thread.

## Current limits

- Reads known translated attributes rather than enumerating every native property.
  Snapshots therefore report `complete=false`; `traversal_complete` separately reports
  whether the requested hierarchy traversal finished within its limits.
- Does not yet expose exact Unicode text injection, value setters, a complete
  system-wide display root, multi-app scene selection, or physical-device services.
- The separate native guest mouse provider is experimental. Service creation and
  dispatch worked; visible cursor movement and pointer click consumption were not verified.
  It is not enabled by default or exposed as an implicit touch fallback.
- Screenshots use simctl's native PNG output. There is no validated screenshot-to-touch
  mapping, so capture results explicitly report `click_mapping: null`.
- References are retained for the session. Destruction notifications, retirement and
  native-handle retention budgets remain follow-up work.
- Private selectors are checked where required at connection, but Apple may change the
  private protocol. A successful dispatch receipt does not prove action consumption.

## Implementation references

The bridge design was checked against DioxusLabs' direct CoreSimulator implementation,
particularly [the reader](https://github.com/DioxusLabs/accessibility-cli/blob/a8464024f84a0c59b769dc526555037e6227fce5/packages/accessibility-ios-sys/src/macos/reader.rs)
and [the token dispatcher](https://github.com/DioxusLabs/accessibility-cli/blob/a8464024f84a0c59b769dc526555037e6227fce5/packages/accessibility-ios-sys/src/macos/dispatcher.rs).
These informed native API routing; the backend does not invoke that CLI.
