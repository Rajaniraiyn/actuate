---
title: Introduction
description: Inspect native apps and control windows with Actuate. Start with Rust providers, the CLI, or a persistent session for your agent.
---

Actuate inspects interfaces and controls native apps, windows, and desktops.
Use its Rust libraries directly, run the CLI, or keep a persistent session open
for repeated observations and actions.

Start with [installation](/installation) and the [quick start](/quick-start).
The workflow is to discover a target, observe its interface, act through a
supported route, and observe the result.

## Choose how to use it

| Entry point | Status |
| --- | --- |
| [Rust](/build/rust) | Available from source |
| [CLI](/automate/commands) and [sessions](/automate/sessions) | Available from source |
| [TypeScript](/build/typescript) and [Python](/build/python) | Proposed SDKs |
| [MCP, skills, and agent plugins](/connect) | Planned integrations |

## Target explicitly

An accessibility action, a window-directed pointer event, and shared desktop
input have different effects. Query the selected provider's capabilities before
choosing a route. Do not assume that an accessibility action leaves focus alone.

Actuate keeps native handles inside providers. Element references belong to the
session that observed them. A dispatched action is not proof that an app accepted
it. Read [targeting and references](/automate/targeting) before writing a loop.

## Platform coverage

Providers exist for Windows, macOS, Linux, Android, and Apple devices and
simulators. Their capabilities and validation differ. The
[platform guide](/platforms) records those limits.

The [API design pages](/design) describe proposed CLI and language interfaces.
