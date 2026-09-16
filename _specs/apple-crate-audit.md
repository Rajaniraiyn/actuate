# Apple dependency and integration audit

Reviewed 2026-09-16 against shallow source checkouts. Versions below are the checked-out manifests, not a claim that these are the latest published releases. No dependencies or native integrations were changed by this audit.

## Decisions

| Candidate | Decision | Boundary |
| --- | --- | --- |
| `idevice` 0.1.68, MIT | Adopt as the first candidate for an optional physical-device adapter, after device testing | Discovery, paired transports and individual device services |
| `screencapturekit` 10.0.3, MIT OR Apache-2.0 | Evaluate for streaming/audio/recording; retain current direct objc2 still capture | A replaceable capture provider |
| `apple-cf` 0.10.0, MIT OR Apache-2.0 | Use if needed by a selected media provider; do not replace all native object types | Private implementation dependency of that provider |
| Servo framework crates, MIT OR Apache-2.0 | Do not migrate to deprecated Cocoa wrappers | Keep the existing objc2 family |
| `apple-metal`, `videotoolbox` | Defer until GPU frame processing or video encoding is required | Optional frame processing/encoding adapters |

These are architecture recommendations. None of the candidates has been compiled or benchmarked in Actuate during this review. A safe public Rust API reduces local FFI code, but does not prove compatibility, complete thread safety or better performance.

## Existing bindings are already modern

The macOS and iOS manifests use `objc2` 0.6.4 and generated framework crates 0.3.2. macOS still capture already uses `objc2-screen-capture-kit`, `block2` and ImageIO, retains native work inside callbacks, returns owned PNG bytes, and rejects geometry changes before providing a click mapping. Replacing that implementation solely because another crate describes itself as idiomatic would add a second object ownership family without an established benefit.

Servo explicitly deprecates [Cocoa](https://github.com/servo/core-foundation-rs/blob/88cb72cb2e0ac3f1e4f39e04d80f0b24f8374401/cocoa/src/lib.rs), [cocoa-foundation](https://github.com/servo/core-foundation-rs/blob/88cb72cb2e0ac3f1e4f39e04d80f0b24f8374401/cocoa-foundation/src/lib.rs) and [io-surface](https://github.com/servo/core-foundation-rs/blob/88cb72cb2e0ac3f1e4f39e04d80f0b24f8374401/io-surface/src/lib.rs) in favor of objc2 counterparts. This is not a blanket deprecation of every package in that repository. The [objc2 project](https://github.com/madsmtm/objc2) remains the appropriate default for our direct framework access.

## Physical devices with idevice

The useful separation is already present upstream. [`IdeviceProvider`](https://github.com/jkcoxson/idevice/blob/d32c8189c51c2789496b0768039419c3705498c3/idevice/src/provider.rs) supplies connections and pairing material independently of service clients. [`usbmuxd`](https://github.com/jkcoxson/idevice/blob/d32c8189c51c2789496b0768039419c3705498c3/idevice/src/usbmuxd/mod.rs) exposes device enumeration and attachment events. [`screenshotr`](https://github.com/jkcoxson/idevice/blob/d32c8189c51c2789496b0768039419c3705498c3/idevice/src/services/screenshotr.rs) returns screenshot bytes. [`springboardservices`](https://github.com/jkcoxson/idevice/blob/d32c8189c51c2789496b0768039419c3705498c3/idevice/src/services/springboardservices.rs) exposes orientation and icon information. These are useful physical-device capabilities, not proof of unrestricted touch injection or whole-OS accessibility traversal.

The [manifest](https://github.com/jkcoxson/idevice/blob/d32c8189c51c2789496b0768039419c3705498c3/idevice/Cargo.toml) separates usbmuxd, TCP, tunneling, screenshot, developer tools, XCTest and WDA features. It uses Tokio and plist; default TLS selects aws-lc. Choose a reviewed feature set and crypto provider instead of enabling every service. Upstream warns of breaking point releases before 0.2, so initially pin an exact tested version.

Proposed integration:

- Put `physical` and macOS-only `simulator` modules behind separate dependency and module gates in the iOS crate. Its current crate-wide macOS gate is appropriate for CoreSimulator, but would incorrectly exclude a transport adapter on other hosts.
- Return typed discovered targets with stable device identity, connection kind and availability evidence. Keep transport reconnect identity separate from a live session generation. Invalidate element references on session replacement.
- Discover without starting XCTest, pairing a device, mounting an image or launching an app. Report unpaired, locked, disconnected and service-unavailable distinctly. Connection discovery must not imply action support.
- Implement `Capture` and application services independently. Add an AX/action adapter only after testing the selected XCTest/WDA route. Simulator AXPTranslator and Indigo remain separate implementations.
- Keep async transport ownership inside the adapter. Avoid constructing a new Tokio runtime for each command or calling blocking runtime entry from another runtime. Preserve deadlines and disconnect cancellation.
- Preserve encoded image type, orientation and dimensions. A screenshot service returning bytes does not establish the coordinate transform for touch. Publish a click mapping only when the input route and orientation are verified together.

## ScreenCaptureKit and shared Apple wrappers

The [ScreenCaptureKit manifest](https://github.com/doom-fish/screencapturekit-rs/blob/5e2c581c780a873b203cee2d54d0b9724611e07e/Cargo.toml) has opt-in async and cumulative OS-version features. Its [build script](https://github.com/doom-fish/screencapturekit-rs/blob/5e2c581c780a873b203cee2d54d0b9724611e07e/build.rs) compiles a Swift bridge. This is a real toolchain and distribution consideration, even though callers use Rust. Our current direct objc2 route does not require that bridge.

Two inspected designs are useful independently of adopting the dependency:

- [Completion ownership](https://github.com/doom-fish/screencapturekit-rs/blob/5e2c581c780a873b203cee2d54d0b9724611e07e/src/utils/completion.rs) uses registered opaque tokens, bounded waits and removal on timeout. Late or repeated callbacks cannot dereference a freed Rust context. Our retained blocks already keep callback captures alive, so do not add a second registry without a corresponding raw-context problem.
- [Frame status](https://github.com/doom-fish/screencapturekit-rs/blob/5e2c581c780a873b203cee2d54d0b9724611e07e/src/cm/frame_status.rs) distinguishes complete, idle, blank, suspended, started and stopped frames. A future stream adapter should retain status, timestamp and geometry revision instead of treating every callback as a fresh usable screenshot.

A streaming adapter needs bounded queues, a declared frame-drop policy, explicit shutdown, callback panic containment and owned frame lifetimes. Keep native buffers inside that adapter; expose a frame lease or owned bytes according to the consumer's needs. Zero-copy buffers require a lifetime and synchronization contract, not just a raw pointer. Exclude the visual agent cursor from capture when requested, without excluding unrelated application windows.

[`apple-cf`](https://github.com/doom-fish/apple-cf-rs/blob/ae3871caf1cc6065864fcb3312202cf4ce3ec996/README.md) documents borrowed versus owned raw constructors and aliasing obligations for mapped buffers. Its [manifest](https://github.com/doom-fish/apple-cf-rs/blob/ae3871caf1cc6065864fcb3312202cf4ce3ec996/Cargo.toml) and [build script](https://github.com/doom-fish/apple-cf-rs/blob/ae3871caf1cc6065864fcb3312202cf4ce3ec996/build.rs) show feature-selected frameworks, a `doom-fish-utils` dependency and a Swift build. Do not rely on the README's dependency-free wording as a literal dependency audit. Avoid converting back and forth between its wrappers and objc2 objects in shared code; own that conversion in a single adapter with explicit retain rules.

Related [`apple-metal`](https://github.com/doom-fish/apple-metal-rs) and [`videotoolbox`](https://github.com/doom-fish/videotoolbox-rs) are candidates for GPU processing and encoding. They are outside the current still-image requirement. Their README-level review does not constitute a source safety audit or an integration recommendation today.

## Acceptance criteria for an adapter

1. Feature-disabled builds keep current dependency and toolchain requirements.
2. Provider selection never silently changes the target device, desktop focus or input route.
3. Availability reports distinguish compile support, runtime/framework presence, permissions, connectivity and tested capabilities.
4. Tests cover disconnect, late completion, unavailable service, rotation and stale geometry, in addition to success.
5. Compare binary size, build time and capture latency before claiming an optimization.
6. Retain upstream license notices for copied code. Keep these commit links alongside any adopted design so later changes can be checked against the reviewed source.
