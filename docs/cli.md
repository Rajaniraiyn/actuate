# Provider selection

Shared commands select their provider through `--provider` or `UNIMATION_PROVIDER`.
The explicit flag takes precedence over the environment. The default `native` uses
the host backend, currently macOS. It never selects a simulator merely because one
is installed. The default build accepts `native`, `macos`, `apple-simulator`, and `apple-device`.

```sh
unimation snapshot --format text
unimation --provider apple-simulator --device UUID --device-set /absolute/device/set session
UNIMATION_PROVIDER=apple-simulator UNIMATION_DEVICE=UUID UNIMATION_DEVICE_SET=/absolute/device/set unimation snapshot --format text
unimation apple simulators list
unimation completions zsh
```

`UNIMATION_DEVICE` and `UNIMATION_DEVICE_SET` also have explicit flag overrides.
The iOS provider requires both to avoid connecting to an unrelated device. Device
selection is separate from guest application selection: an optional snapshot PID
identifies an application inside that device. Without a PID, snapshot reads the
foreground application. Simulator discovery does not boot, install, or delete anything.

Provider, format, discovery scope, snapshot scope, capture route, and completion
shell are Rust value enums. Usage-rs derives their parsing, allowed choices, help,
and completion metadata. Device-set paths carry directory completion hints.
`spec` exports this same declaration as Usage KDL. Shell scripts use usage-rs's
embedded completion responder; completion does not need a separate Usage executable.

Commands remain stable regardless of currently installed devices. Provider-specific
resource discovery belongs under `ios simulators`; common observation, capture,
and session commands use provider selection. Unsupported operations report errors.
iOS currently supports these shared entry points: `snapshot`/`observe`, `capture`,
`capabilities`, and `session`. Its session protocol still exposes provider-specific
operations and scopes; this does not imply every macOS session request works on iOS.

The iOS simulator provider currently runs on macOS. Missing CoreSimulator tooling,
missing devices, and non-booted devices are connection/discovery errors. Simulator
creation and cleanup remain test-script responsibilities. Live capability reports
require a valid selected device.

References: [usage-rs derive](https://docs.rs/usage-derive/6.9.1/usage_derive/),
[agent-browser commands](https://github.com/vercel-labs/agent-browser).

## Output

Data commands default to plain text, both in a terminal and when piped. There are
no colors, progress bars or terminal-only decorations. Trees and diffs use bounded
presentation views. Other records use tab-separated field paths and values, with
control characters escaped so each value remains on one line. Simulator inventory
text lists runtimes and devices; full metadata and device types require JSON.

`--json` opts into full JSON. `--format text|compact|json` is the global alternative;
passing both flags is an error. `compact` is structured JSON presentation, not a
synonym for plain text. Session input remains JSONL; default replies are framed
text, while `session --json` preserves the JSONL response protocol. Scripts that
parse JSON must opt in. `spec`, `protocol`, and `completions` emit their documented
artifact formats regardless of the data-output selection.

The Apple simulator command and native macOS provider compile only on macOS.
Physical Apple discovery has its own feature gate. Saved-snapshot tools are portable.

## Physical iOS devices

The default CLI build includes `--provider apple-device`. Library users enable the
`ios/physical` feature; `cargo build -p cli --no-default-features` omits it.

```sh
unimation --provider apple-device discover
unimation --provider apple-device capabilities
unimation --provider apple-device --device DEVICE_UDID discover
unimation --provider apple-device --device DEVICE_UDID capture /absolute/new-frame.bin
```

Capture preserves the device's encoded image format and reports that format in
metadata; a file extension does not request transcoding. The first adapter uses
the legacy screenshotr service and does not imply modern iOS screenshot, streaming,
accessibility or input support. It requires an existing usbmuxd service and pairing
record. It does not launch a daemon, pair, install software or mount an image.
`--device-set` belongs to the CoreSimulator provider and is rejected for apple-device.

## Apple resource groups

`apple simulators list` enumerates installed simulator resources on macOS.
`apple devices list` enumerates physical-device records through idevice. The latter
is feature-gated independently of the macOS-only simulator branch. These groups can
accept future Apple device transports without introducing an iOS-specific top-level
command. Their presence does not claim automation support for watchOS or tvOS.
