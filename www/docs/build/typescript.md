---
title: TypeScript
description: Follow the proposed TypeScript SDK design for typed requests, promises, and session lifetimes. Use the existing JSONL CLI transport today.
---

The TypeScript SDK is planned. There is no package to install yet.

Today, a TypeScript application can launch `actuate session --json` and exchange
line-delimited requests over stdin and stdout. See the [session protocol](/automate/sessions).

The proposed SDK uses asynchronous methods, discriminated unions for action and
delivery types, and `AbortSignal` for waiting. A session owns its references and
closes its transport through an explicit `close()` method.

API names and lifecycle examples are in the [language API design](/design/language-apis).
