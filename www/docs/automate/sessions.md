---
title: Sessions
description: Keep native connections and element references in one process. Send JSONL requests, correlate replies, and preserve structured error effects.
---

```sh
actuate session --json
```

Send one JSON object per line. Actuate writes one reply per line, in order.
An optional `id` is echoed on valid JSON requests, including operation errors.
Malformed JSON does not end the session.

```json
{"id":1,"op":"capabilities"}
{"id":2,"op":"discover"}
```

Replies contain either `result` or an `error` with `code`, `message`, and `effect`.
See [errors and effects](/guides/errors) for recovery behavior.

## Reference lifetime

Observe and act within the same session. A short reference such as `@e7` belongs
to that session. A new CLI process cannot reuse it. Closing stdin releases the
session; there is no shared background daemon in the current CLI.

## Actions

Inspect a reference to learn its native action names before calling `semantic`.
Pointer, keyboard, and text input use an explicit delivery mode. Supported modes
vary by provider. Cursor visualization is separate from input delivery.

The [protocol specification](https://github.com/Rajaniraiyn/actuate/blob/main/_specs/session.md)
lists requests and provider extensions. `actuate protocol` prints the same
embedded specification. Consult the selected platform's capabilities before
using an extension.
