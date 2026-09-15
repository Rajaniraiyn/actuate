# Physical iOS services

Enable the `physical` Cargo feature to use `ios::physical`. It pins idevice 0.1.68 with only usbmuxd, screenshotr and ring TLS. It adds no Swift build. The feature is independent of the macOS-only simulator modules, whose existing public exports remain intact.

`PhysicalDevices` provides asynchronous discovery, device-info and screenshot calls. `PhysicalCapture` adapts those calls to the synchronous `Capture` trait with one runtime per instance. Async callers should use `PhysicalDevices` directly. Both use an explicit positive deadline. `PhysicalCapture::info(udid)` provides the synchronous read-only metadata probe. `USBMUXD_SOCKET_ADDRESS` selects an existing daemon endpoint. Discovery only sends ListDevices, and captures resolve the UDID again before connecting because transport IDs can change.

Discovery reports incomplete evidence because upstream `get_devices()` omits malformed records during conversion. Finding a device does not establish pairing, developer-service availability, screen capture or UI interaction support. Duplicate transports for the same UDID produce an ambiguity error rather than a silent preference.

Capture uses an existing pairing record and starts the screenshotr service. It never pairs, mounts an image, installs a runner or launches an app. Upstream documents the lockdownd screenshotr route for iOS below 17. The separately implemented upstream RSD shim and DVT screenshot clients require additional connection providers and are not enabled here. Modern iOS capture, display streaming, accessibility and input are not claimed by this adapter.

Frames keep the original encoded bytes, detected image format and dimensions when the image header is understood. Orientation and click mapping remain unknown. `save()` refuses to overwrite an existing file. Repeated still capture is not a video stream.

CoreSimulator is not a usbmuxd device: this service path cannot replace native AXPTranslator/Indigo or SimulatorKit. There is no guest binary in the current simulator path to remove or embed.

Reviewed sources at d32c8189c51c2789496b0768039419c3705498c3:

- [usbmuxd discovery](https://github.com/jkcoxson/idevice/blob/d32c8189c51c2789496b0768039419c3705498c3/idevice/src/usbmuxd/mod.rs)
- [service/provider connection](https://github.com/jkcoxson/idevice/blob/d32c8189c51c2789496b0768039419c3705498c3/idevice/src/provider.rs)
- [screenshotr](https://github.com/jkcoxson/idevice/blob/d32c8189c51c2789496b0768039419c3705498c3/idevice/src/services/screenshotr.rs)
- [DVT screenshot](https://github.com/jkcoxson/idevice/blob/d32c8189c51c2789496b0768039419c3705498c3/idevice/src/services/dvt/screenshot.rs)

Tests exercise a mock usbmuxd ListDevices exchange, deadlines, explicit identity selection, duplicate transports, encoded image metadata and runtime boundaries. They do not establish success on a connected physical phone.

## Device information and physical validation

`info(udid)` resolves the current transport and makes one lockdownd `GetValue` request. It does not start a pairing session or developer service. The report exposes optional device name, product type and OS version, alongside all returned properties as native XML plist. This preserves data and date values without guessing JSON conversions. Missing convenience fields remain unknown. Device metadata is not evidence of capture or input availability.

The info probe uses the upstream `IdeviceProvider` boundary internally, so later transports can share the lockdownd protocol implementation. The simulator keeps its own CoreSimulator connection and native accessibility/input providers. Neither a simulator UUID nor an arbitrary stream is sufficient to turn a simulator into a lockdownd/usbmuxd device.

Modern physical interaction needs further work. Current upstream has RSD/DVT, XCTest and WDA bootstrap helpers, but WDA input depends on a signed, installed runner and developer services. We have not enabled these features or made an untested modern capture path the default. Once a physical device is available, validate its OS version, trust/developer mode, service discovery, capture orientation, then app and system interaction separately. Existing screenshotr support is not a claim that a current iPhone is fully automatable.

The mock lockdownd test checks the actual length-framed request/response, preserves data/date fields, and fails if the provider is asked for a pairing record. Empty UDIDs fail before any connection. These tests establish protocol behavior only; no physical iPhone has been tested here.

References:

- [Pinned lockdownd GetValue implementation](https://github.com/jkcoxson/idevice/blob/d32c8189c51c2789496b0768039419c3705498c3/idevice/src/services/lockdown.rs)
- [Upstream transport features and XCTest/WDA prerequisites](https://github.com/jkcoxson/idevice#xctest--wda)
