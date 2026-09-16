---
title: Connect
description: Run Actuate through local command tools today. Track planned MCP, Claude Code, Codex, skill, and plugin adapters that preserve native behavior.
---

An agent or application that can run local commands can use the Actuate CLI.
For repeated interactions, keep one `actuate session --json` process and forward
requests to it. This preserves native connections and element references.

| Integration | Status |
| --- | --- |
| Local CLI and session transport | Available from source |
| MCP automation server | Planned |
| Claude Code integration | Planned |
| Codex integration | Planned |
| Skills and agent plugins | Planned |

There are no dedicated install commands for the planned adapters yet.

## Adapter contract

Adapters should call the shared implementation and preserve capability reports,
target scope, receipts, and structured errors. They must reject unsupported
delivery modes rather than fall back to global input. Keep references inside the
session that created them.

A skill can teach a workflow, and a plugin can package an adapter. Neither should
implement a separate version of targeting or action semantics.

## Documentation access

This static documentation site provides search and Markdown exports. It does
not run an Actuate automation server or a documentation MCP server.
