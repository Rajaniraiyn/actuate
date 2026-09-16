---
title: API design
description: Design CLI and language adapters around shared native behavior. Preserve capabilities, target identity, coordinate spaces, delivery, and effects.
---

These pages describe proposed interfaces. The [command reference](../www/docs/automate/commands.md)
and [Rust guide](../www/docs/build/rust.md) describe what is implemented.

The documentation flow takes cues from [agent-browser](https://agent-browser.dev/):
installation, a short observe-and-act workflow, then focused command and session
references. Native desktop automation needs explicit window, device, and input
scope alongside that workflow.

## Shared behavior

Every adapter preserves capability reporting, target identity, coordinate space,
delivery mode, and effect state. Language conventions can change spelling and
lifecycle syntax, but not the operation's meaning.

Keep native action names available for platform-specific operations. Portable
convenience methods should exist only where providers can report their support.
Reject unsupported requests without changing their delivery mode.

- [CLI design](cli-design.md) covers command shape and persistent state.
- [Language API design](language-api-design.md) covers Rust, TypeScript, and Python.
