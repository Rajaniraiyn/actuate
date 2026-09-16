---
title: Actuate
description: Inspect native apps and control windows with Actuate. Start with Rust providers, the CLI, or a persistent session for your agent.
---

Native control for your agents and applications.

Actuate is a native UI automation toolkit. Inspect interfaces and orchestrate
interactions across platforms. Compose Rust providers in your code, run the CLI,
or keep a persistent session open for your agent.

## Get started

After [building Actuate](/installation), inspect what is available on your host:

```sh
actuate capabilities
actuate discover
actuate session --json
```

The [quick start](/quick-start) walks through discovery, observation, and an action
in one session.

## Choose how to use it

| Entry point | Status |
| --- | --- |
| [Rust](/build/rust) | Available from source |
| [CLI](/automate/commands) and [sessions](/automate/sessions) | Available from source |
| [TypeScript](/build/typescript) and [Python](/build/python) | Proposed SDKs |
| [MCP, skills, and agent plugins](/connect) | Planned integrations |

## Learn the workflow

- Read [snapshots](/guides/snapshots) and select [targets](/automate/targeting).
- Choose an [input route and cursor](/guides/input-and-cursors).
- Handle [file dialogs and drag-and-drop](/guides/files-and-dialogs).
- Interpret [errors and effects](/guides/errors) before deciding whether to retry.

## Platform coverage

Providers exist for Windows, macOS, Linux, Android, and Apple devices and
simulators. Their capabilities and validation differ. The
[platform guide](/platforms) records those limits.

The [API design pages](/design) describe proposed CLI and language interfaces.
