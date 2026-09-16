---
title: Commands
description: Use Actuate commands to discover applications, inspect accessibility trees, capture windows, and compare saved observations. Check compiled help.
---

## Discover

```sh
actuate capabilities
actuate discover
actuate windows
actuate displays
```

Use capabilities to check routes and permissions. Discovery returns application
identities; windows and displays return native IDs and geometry where supported.

## Observe

```sh
actuate snapshot 1234
actuate snapshot 1234 --interactive --limit 100
actuate snapshot 1234 --json
```

Replace `1234` with a process ID. `observe` and `snapshot` name the same command.
Use `--max-nodes` and `--max-depth` to bound traversal; `--limit` bounds the
presentation. Filtering does not make an incomplete tree complete.

## Capture

```sh
actuate capture screen.png
actuate capture window.png --window 42
```

Select IDs returned by the provider. Coordinate mappings and supported capture
routes depend on the platform.

## Work with saved observations

```sh
actuate view snapshot.json --interactive
actuate query snapshot.json --name Save
actuate diff before.json after.json
```

The query reads a saved snapshot. It does not locate a new live element. Compare
observations from the same session; foreign or reversed revisions are rejected.

## Run a session

```sh
actuate session --json
actuate protocol
actuate spec
actuate completions zsh
```

Use the [session protocol](/automate/sessions) to inspect references and perform
actions within the process that owns them.

## Cursor renderer

`actuate overlay` reads cursor commands from stdin. Sessions manage its lifecycle
through `cursor_overlay`. See [input and cursors](/guides/input-and-cursors).

## Output

Text is the default. `--json` returns structured output. `--format compact`
returns a bounded JSON presentation; it does not mean plain text.
Do not pass both `--json` and `--format`. Use `--help` on any command for its
compiled options and providers.

See [configuration](/automate/configuration) for provider and device selection.
