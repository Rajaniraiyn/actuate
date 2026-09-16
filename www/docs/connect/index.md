---
title: Connect an agent
description: Connect an agent or application to Actuate through local commands or a persistent JSONL session. Preserve target references and structured errors.
---

An agent that can run local commands can use Actuate. Build the CLI with the
[installation guide](/installation), then expose the executable to your agent's
command runner.

## Run individual commands

Use individual commands for discovery, screenshots, or a standalone observation:

```sh
actuate capabilities --json
actuate discover --json
actuate snapshot 1234 --json
```

Replace `1234` with the selected process ID. Parse JSON results directly and
retain any reported observation issues.

## Keep an interaction session

Start `actuate session --json` when the agent needs to inspect and act on the same
references. Send one request per line and associate each reply with its request
ID. Keep the process open until the interaction ends.

Give the agent capability information alongside the observation. Before an
action, select its target and delivery route explicitly. After dispatch, observe
the application to decide whether the intended change occurred.

See [sessions](/automate/sessions) for message framing and the
[quick start](/quick-start) for an observe-and-act example.

## Integration availability

| Integration | Availability |
| --- | --- |
| Local commands and persistent sessions | Available from source |
| MCP automation server | Planned |
| Claude Code and Codex integrations | Planned |
| Packaged skills and agent plugins | Planned |

For a custom adapter, follow the [developer guide](/developers). Keep reference
ownership, delivery scope, and native error effects intact across the transport.
