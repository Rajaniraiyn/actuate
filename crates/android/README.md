# Android provider

DroidMux supplies direct USB, legacy TCP and paired wireless transports. They
share one protocol client and bounded shell executor. No adb executable or host
adb server is launched. USB support links libusb at build time; host drivers and
permissions still apply.

`Android<D: CommandTransport>` owns automation separately from connection setup.
`WirelessHost` manages code/QR pairing and explicit connection-key trust.
`DirectDevice` handles USB and legacy TCP using an RSA identity. Device-side USB
authorization is still required. USB discovery reports bus/address; VID/PID
selection rejects ambiguous matches.

Operations include device information, launcher activities, explicit activity
launch, PNG capture, pixel tap/swipe, navigation keys and restricted ASCII text.
Components are validated at construction and deserialization. No arbitrary-shell
CLI is exposed. JSONL is an external adapter over typed library operations.

Shell-v2 exit status is required. A dispatched receipt means Android accepted the
command; it does not prove the intended UI changed. Uncertain actions are never
retried automatically. Output size and whole-operation timeouts are bounded.

## Capability boundaries

Screenshots retain native display pixels. Input does not infer rotation, another
display, or a host logical-point mapping. ASCII text rejects unsupported characters.
Accessibility snapshots and stable Android element references are not implemented.
UHID pointer input is a separate persistent capability; relative motion must not
be advertised as exact screen coordinates. No APK is installed automatically.

The remaining local DroidMux patches cover protocol races and wireless trust/QR
handling. See the patch records and [source audit](../../docs/android-reference-audit.md).
Plain TCP is the legacy ADB route; Android wireless debugging uses paired TLS.

## Validation

Unit tests cover typed validation, quoting, output bounds and uncertain exit status.
Live device findings are recorded separately; USB and QR scanning need their own
hardware validation. No simulator test establishes physical-device behavior.

See [live validation](../../docs/android-validation.md) for observed behavior and remaining gaps.
