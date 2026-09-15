# Android source review

## droidrun-rs

Reviewed commit [ba5280bed0c33fe372a69e10fef6271f2c273178](https://github.com/Sikrid25/droidrun-rs/tree/ba5280bed0c33fe372a69e10fef6271f2c273178), MIT. This is a reference, not an added dependency or copied implementation.

Its host protocol client avoids invoking the adb CLI for operations, but connects
to an existing adb server at port 5037 using host:devices and selected transports.
It therefore does not replace our direct USB or paired wireless connections.
The full UI pipeline requires the DroidRun Portal Android app, accessibility
service and keyboard. The Portal manager can download/install the APK, enable
accessibility and change the keyboard. Unimation does not perform this setup
implicitly. Sources: [server](https://github.com/Sikrid25/droidrun-rs/blob/ba5280bed0c33fe372a69e10fef6271f2c273178/crates/droidrun-adb/src/server.rs), [Portal manager](https://github.com/Sikrid25/droidrun-rs/blob/ba5280bed0c33fe372a69e10fef6271f2c273178/crates/droidrun-core/src/portal/manager.rs).

### Patterns to adopt

- Separate raw observation, filtering and formatting. Its generic
  `AndroidStateProvider<F: TreeFilter, M: TreeFormatter>` fits our provider and
  presentation split. Retain the complete raw snapshot while projecting compact
  output. [Provider](https://github.com/Sikrid25/droidrun-rs/blob/ba5280bed0c33fe372a69e10fef6271f2c273178/crates/droidrun-core/src/ui/provider.rs)
- An optional app-backed observation/text-input provider can complement direct
  ADB input and screenshots. Keep app setup explicit and provider-owned.
- Record operations through a driver decorator, with credential/text redaction,
  rather than adding logging independently to each command.

### Behaviors requiring changes before adoption

The indexed formatter starts numbering at one for each observation. These are
snapshot positions, not stable native identities. Any future Portal adapter must
scope them to the observation revision and revalidate before acting. Do not
promote an index into a stable ElementRef. [Formatter](https://github.com/Sikrid25/droidrun-rs/blob/ba5280bed0c33fe372a69e10fef6271f2c273178/crates/droidrun-core/src/ui/formatter.rs)

Its concise filter discards whole off-screen/tiny subtrees. Keep off-screen state
in our raw observation and avoid removing potentially visible children just
because their parent's bounds are incomplete. The provider substitutes 1080 by
2400 when dimensions are missing. Unimation should report an unknown mapping
rather than inject coordinates using guessed dimensions. Normalized conversion
must distinguish a rectangle's outer edge from the final addressable pixel.
[Filter](https://github.com/Sikrid25/droidrun-rs/blob/ba5280bed0c33fe372a69e10fef6271f2c273178/crates/droidrun-core/src/ui/filter.rs), [coordinate conversion](https://github.com/Sikrid25/droidrun-rs/blob/ba5280bed0c33fe372a69e10fef6271f2c273178/crates/droidrun-core/src/ui/coord.rs).

Its clear-point search treats later indexed overlapping elements as blockers.
Tree order alone does not prove visual stacking or hit-test ownership. This is a
candidate-point heuristic, not evidence that a click is safe. Require current
window/modal context and an independent hit test or explicitly uncertain result.
[UI state](https://github.com/Sikrid25/droidrun-rs/blob/ba5280bed0c33fe372a69e10fef6271f2c273178/crates/droidrun-core/src/ui/state.rs).

## adb_client

Reviewed [b88e3324627f4e7e38dbf7f9997ddacf73f12dbe](https://github.com/cocool97/adb_client/tree/b88e3324627f4e7e38dbf7f9997ddacf73f12dbe), version 3.2.3, MIT. Initially used for direct USB/TCP with local EOF and shell-v2 fixes. Removed during consolidation because DroidMux already provides those transports and shares the wireless protocol engine. No adb_client source remains in this repository.

The direct API does not implement Android's code/QR pairing exchange. Its pair
method delegates to a host adb server. Do not infer direct native pairing from
the method name. Wireless pairing/transport is a separate composable provider.

## adb-wireless

Reviewed [d7936aa4acf7a9a8e84f6e4e8915ee7f3a924428](https://github.com/teamclouday/adb-wireless/tree/d7936aa4acf7a9a8e84f6e4e8915ee7f3a924428), version 0.1.3, Apache-2.0. Its QR and mDNS workflow is a useful UX reference. Its execution layer invokes `adb version`, `adb start-server`, `adb devices`, `adb pair`, and connection/port commands using `std::process::Command`. It therefore does not supply the binary-free pairing protocol needed here. [ADB execution](https://github.com/teamclouday/adb-wireless/blob/d7936aa4acf7a9a8e84f6e4e8915ee7f3a924428/src/utility/adb.rs)

The QR payload uses `WIFI:T:ADB;S:<service>;P:<password>;;`. Discovery distinguishes the pairing and connection service ports. Retain those distinctions. Its initial pairing wait blocks without an overall deadline; its service-name substring match and IPv4-address-only connection match are too weak for selecting among multiple devices. Unimation uses bounded discovery, a session-specific service name, scoped address handling and paired identity checks. This reference also logs the password in debug output, which we must not adopt. [Pair service](https://github.com/teamclouday/adb-wireless/blob/d7936aa4acf7a9a8e84f6e4e8915ee7f3a924428/src/utility/pair.rs)

Decision: use the interaction pattern as a reference; do not add this CLI as a dependency or subprocess.

## rsadb

Reviewed [414674af19bdde3041ef8a222a0551ac467496ee](https://github.com/tbraun96/rsadb/tree/414674af19bdde3041ef8a222a0551ac467496ee), version 0.1.1, dual MIT/Apache-2.0. It is a credible future direct USB provider. Its `usb` feature uses `nusb 0.2` instead of rusb/libusb. `usb::list`, `find(Some(serial))`, and `open` provide selected device access, with manufacturer/product/serial and attachment ID in `UsbDeviceInfo`. It can distinguish identical VID/PID devices better than the former adb_client adapter. Host permissions and appropriate USB drivers still apply. [Manifest](https://github.com/tbraun96/rsadb/blob/414674af19bdde3041ef8a222a0551ac467496ee/Cargo.toml), [USB implementation](https://github.com/tbraun96/rsadb/blob/414674af19bdde3041ef8a222a0551ac467496ee/src/transport/usb/mod.rs)

The architecture separates `Transport` into message source/sink traits, exposes `StreamTransport<AsyncRead + AsyncWrite>`, and composes sessions with `Device<C>`. Shell v2, binary screenshots and explicit negotiated message limits are useful patterns. An adapter could fit our `CommandTransport` boundary without changing agent commands. Default features include its CLI and host-server client, so any adoption should explicitly select only required library features. [Transport traits](https://github.com/tbraun96/rsadb/blob/414674af19bdde3041ef8a222a0551ac467496ee/src/transport/mod.rs), [device methods](https://github.com/tbraun96/rsadb/blob/414674af19bdde3041ef8a222a0551ac467496ee/src/device/mod.rs)

This commit does not support Android wireless-debugging TLS. The handshake explicitly rejects `Command::StartTls` with an unsupported error. Its TCP path supports legacy RSA-authenticated adbd connections, not QR/code-paired Android 11+ wireless debugging. Its README comparison of adb_client TCP support is outdated relative to the reviewed adb_client source, so we rely on implementation evidence. [Handshake](https://github.com/tbraun96/rsadb/blob/414674af19bdde3041ef8a222a0551ac467496ee/src/session/handshake.rs)

Before adopting it, wrap connect, handshake and command execution in whole-operation deadlines. The exposed authentication wait bounds only reads after the public key is offered, and raw TCP connection does not itself specify an application deadline. Cancellation/disconnection and bounded output tests are required before replacing the current provider.

Decision: defer replacement during wireless bring-up. Evaluate an optional nusb-based USB adapter with real-device tests later; retain the paired wireless route for QR/code connections. No rsadb code or dependency was added by this review.

## Consolidated transport decision

Use DroidMux for direct USB, legacy TCP and paired TLS. All three feed one bounded
shell executor. This removed 564 KB of duplicate source and reduced normal Android
dependency package labels from 141 to 115 at the reviewed lockfile. USB remains
feature-gated; actual USB hardware testing is pending. Three local protocol patches
remain until upstream equivalents are available. Moving them into a generated
vendor directory would not remove their maintenance cost.

UHID is a separate persistent input capability. Android applies acceleration to
relative mouse motion, so deltas do not map directly to screenshot pixels. The
built-in Android `hid` tool can keep a virtual input device alive over a shell
stream on supported devices. Its availability and readiness must be probed.
See [scrcpy mouse modes](https://github.com/Genymobile/scrcpy/blob/master/doc/mouse.md)
and [its UHID manager](https://github.com/Genymobile/scrcpy/blob/master/server/src/main/java/com/genymobile/scrcpy/control/UhidManager.java).
