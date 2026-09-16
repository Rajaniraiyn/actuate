# Native language bindings

`actuate-bindings` owns one worker per session. A `Dispatcher` factory crosses the
thread boundary; its provider does not. Construction, requests, and destruction
run on that thread. Close serializes behind active work and joins the worker.
Abandoned language objects disconnect their worker without blocking a finalizer.
Worker failure returns an unknown effect; mutations are never retried.

The napi-rs addon runs blocking waits in `AsyncTask`. The PyO3 extension releases
the GIL during connection, request, and close. Language adapters convert at the
FFI boundary and share existing provider dispatchers, without launching the CLI.
The CLI executable is needed only for the optional out-of-process cursor renderer.

## Build locally

```sh
mise install
mise run setup
mise run stage
mise run test
mise run release:wheel
```

Mise pins Rust, Bun, Python, and uv in `mise.toml`. uv locks the Python development
dependencies; Bun locks the TypeScript and docs dependencies. `stage` builds and
copies the local native libraries into their language packages. Release tasks
build optimized binaries and wheels. The release sdist backend downloads wheels;
source development invokes maturin through the mise task.

## Provider access

Both adapters expose every request accepted by the selected provider's session
through `request()`. Common desktop calls have typed convenience methods.
Simulator observation uses a scope instead of a PID and is sent through the
provider request protocol. Android requires an explicit endpoint and credentials;
USB and legacy TCP accept a key file, while paired wireless uses a credential
directory. Physical Apple devices use usbmuxd.

Connection and device permissions remain platform-dependent. Android wireless
credential storage currently requires Unix file permissions. The wrappers do
not claim to add routes or permissions that the underlying providers lack.

`Operation` adapters validate extension results. Custom transports and middleware
are per session. They do not install a native plugin or extend a global registry.
TypeScript JSON conversion rejects unsafe integer results instead of rounding
native IDs. Python preserves integer values.

## Distribution

The native workflow builds the CLI, napi addon, and abi3 Python wheel on each
supported host. PRs upload artifacts. A matching `vX.Y.Z` tag publishes those
assets and versioned package archives to GitHub Releases after all jobs pass.
Registry publication is separate. No credentials for npm or PyPI are required.

The TypeScript archive contains no addon; its install hook downloads the exact
version and target from the release. The Python source package's PEP 517 backend
selects a wheel using `packaging.tags`, downloads it, and verifies the manifest's
SHA-256 and byte count. Missing targets fail without compiling or choosing another
version. `GH_TOKEN` supports private releases. HTTPS mirrors use
`ACTUATE_RELEASE_BASE_URL/<tag>/<asset>`.

`manifest.json` maps asset names to sizes and SHA-256 hashes. SHA256SUMS also
covers the manifest. This detects corruption; its trust comes from the HTTPS
release origin, not from a detached signature.

## Renderer ownership

One executable provides CLI and overlay subcommands. A renderer process retains
its own platform event loop; Windows also retains the cursor visibility guard.
macOS AppKit requires a main thread, so embedding a renderer on a worker is not
a portable replacement. Moving rendering into a host requires a separately owned
UI event loop and explicit stop, join, and crash-recovery behavior.
