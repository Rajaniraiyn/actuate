---
title: Configuration
description: Select providers, device IDs, simulator device sets, and credentials with flags or environment variables. Check which providers your build includes.
---

| Flag | Environment variable | Purpose |
| --- | --- | --- |
| `--provider` | `ACTUATE_PROVIDER` | Select the automation provider. |
| `--device` | `ACTUATE_DEVICE` | Select a device identifier. |
| `--device-set` | `ACTUATE_DEVICE_SET` | Select an existing simulator device set. |
| `--credentials` | `ACTUATE_CREDENTIALS` | Select an Android credential directory or key. |

Flags override their corresponding environment variables. `native` selects the
host platform. Providers and device commands depend on the platform and compiled
features; inspect `actuate --help` on the host where you run it.

```sh
actuate --provider native capabilities
actuate --provider native session --json
```

Connection credentials and device identities are explicit. The CLI does not
choose another device or delivery route when the selected one fails.
