---
title: Python
description: Follow the proposed Python SDK design for typed requests and context-managed sessions. Use subprocess clients with the existing CLI today.
---

The Python SDK is planned. The repository's Python fixtures and test clients are
not an SDK.

Today, use `subprocess.Popen` to run `actuate session --json`. Write one request
per line, flush stdin, and read one complete reply line. Keep that process alive
while using its element references. See [sessions](/automate/sessions).

The proposed SDK uses `snake_case` methods, typed request models, and context
managers for sessions. An asynchronous client should use `async with` and preserve
the same request and error semantics as the synchronous client.

See the [language API design](/design/language-apis).
