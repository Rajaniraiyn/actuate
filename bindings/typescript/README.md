# Actuate for TypeScript

Inspect and control native apps through a persistent session.

```typescript
import { Actuate } from "actuate";

const session = await Actuate.connect();
try {
  console.log(await session.capabilities());
  console.log(await session.discover());
} finally {
  await session.close();
}
```

Use Bun 1.4 or newer. The package installer downloads its version-matched native
addon from GitHub Releases and verifies the release manifest checksum. Bun may
require `bun pm trust actuate` to run the installation hook. Alternatively run
`bun run --cwd node_modules/actuate install-native` explicitly.

There are no platform-specific npm packages. Set `GH_TOKEN` for private release
access, or `ACTUATE_RELEASE_BASE_URL` for an HTTPS release mirror. Set
`ACTUATE_SKIP_DOWNLOAD=1` only when supplying a locally built addon.

For source builds, see the repository's [binding guide](../../_specs/bindings.md).
Native operations execute on a dedicated provider thread. `request()` exposes
platform operations; `defineOperation()` adds typed request and result adapters.
`Actuate.fromTransport()` accepts a custom transport and ordered middleware.

See the [TypeScript guide](https://rajaniraiyn.dev/actuate/build/typescript/).
