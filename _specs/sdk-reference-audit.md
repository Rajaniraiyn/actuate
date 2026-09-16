# SDK and CLI reference review

Reviewed 2026-09-17. These are design references, not dependencies or copied
implementations. The interface proposals live in
[language APIs](language-api-design.md) and
[CLI design](cli-design.md).

## Source revisions

| Project | Revision | Inspected areas |
| --- | --- | --- |
| TanStack Query | `c08f5766ddb5dfd6f2e430eb7c235de866cd669e` | `packages/react-query/src/queryOptions.ts`, TypeScript and query-options documentation |
| TanStack Table | `21d713fc4947d2a08cc2136bb055889a61412ded` | `packages/table-core/src/types/TableFeatures.ts` |
| Vercel AI SDK | `c97fcad34df41cd6d589375881f200154b8e5599` | ProviderV4, provider registry, language-model middleware, tool schema documentation |
| Vercel Labs AI Python | `142b074550b81b230e4c8077bf8a32527319791c` | Provider base, model API, README |
| agent-browser | `aff6125c023b810ea3f2e5deec5379e9a4270bdc` | CLI output, plugin protocol, command and snapshot documentation |

## Decisions for Actuate

| Concern | Reference pattern | Actuate decision |
| --- | --- | --- |
| Type inference | TanStack Query carries request and result types through an options helper. | Infer from typed providers and reusable options. Do not require repeated generics at call sites. |
| Optional features | TanStack Table derives contributed types from selected feature maps. | Scope extension methods to the configured instance. Types must not advertise capabilities an instance did not register. |
| Provider interchange | AI SDK separates versioned provider contracts from its public operations. | Keep native provider contracts separate from language adapters. Version external protocols without promising a Rust ABI. |
| Registry typing | AI SDK derives provider identifiers from registry keys. | Infer registered provider and extension names; validate dynamically loaded entries before exposing operations. |
| Middleware | AI SDK wraps operations and defines input and result order. | Wrap shared operations once. Revalidate transformed requests and retain scope, owner, and effect. |
| Tool schemas | AI SDK's tool helper infers execution parameters from an input schema. | Reuse operation schemas for agent adapters, with validation before dispatch. Keep provider-specific attributes explicitly typed or unknown. |
| Python ownership | AI Python uses executor protocols, immutable request data, and generic async streams. | Use typed protocols, context-managed sessions, and explicit sync or async clients. Avoid hidden event loops. |
| CLI output | agent-browser separates output handling and supports bounded snapshot text. | Keep all views in the shared presentation module. Preserve partial coverage and produce valid complete JSON records. |
| External plugins | agent-browser has a versioned stdio protocol and declared capabilities. | Use a process boundary for external native extensions. A restart changes reference ownership and invalidates cached state. |

## Boundaries to retain

Types describe request shapes and statically selected capabilities. They cannot
establish permission, focus behavior, a live native handle, a current screenshot
mapping, or successful input consumption. These stay runtime checks with evidence.

Observation middleware may bound output. It must preserve omissions and read
errors. Mutation middleware may validate or reject a request, but must not hide
an uncertain effect or automatically retry it. A changed provider cannot reuse
the old provider's element references.

JSONL remains the process boundary. Direct Rust users call typed providers.
TypeScript and Python SDKs should validate at the boundary and avoid rebuilding
native semantics in their transport wrappers.

## Existing implementation and gaps

The Rust core already has independent provider traits, a typed `Backend`, shared
session framing, snapshot history, coordinate checks, motion planning, and output
presentation. The CLI exports its command specification from the parser.

Not implemented: TypeScript and Python packages, options helpers in those
languages, a dynamic provider registry, operation middleware, named CLI session
services, and external automation plugins. The design pages do not claim these
exist.

Before an SDK ships, type tests should cover inference and unavailable methods.
Protocol tests should cover invalid and foreign references, unknown fields,
malformed extension results, cancellation, and effect propagation. Native tests
must still establish behavior on the target OS.

## References

- [TanStack Query options](https://tanstack.com/query/latest/docs/framework/react/guides/query-options)
- [TanStack TypeScript guidance](https://tanstack.com/query/latest/docs/framework/react/typescript)
- [TanStack feature maps](https://github.com/TanStack/table/blob/21d713fc4947d2a08cc2136bb055889a61412ded/packages/table-core/src/types/TableFeatures.ts)
- [AI SDK provider contract](https://github.com/vercel/ai/blob/c97fcad34df41cd6d589375881f200154b8e5599/packages/provider/src/provider/v4/provider-v4.ts)
- [AI SDK provider registry](https://github.com/vercel/ai/blob/c97fcad34df41cd6d589375881f200154b8e5599/packages/ai/src/registry/provider-registry.ts)
- [AI SDK middleware](https://github.com/vercel/ai/blob/c97fcad34df41cd6d589375881f200154b8e5599/packages/ai/src/middleware/wrap-language-model.ts)
- [AI SDK tools](https://ai-sdk.dev/docs/ai-sdk-core/tools-and-tool-calling)
- [AI Python model API](https://github.com/vercel-labs/ai-python/blob/142b074550b81b230e4c8077bf8a32527319791c/src/ai/models/core/api.py)
- [AI Python provider base](https://github.com/vercel-labs/ai-python/blob/142b074550b81b230e4c8077bf8a32527319791c/src/ai/providers/base.py)
- [agent-browser commands](https://agent-browser.dev/commands)
- [agent-browser snapshots](https://agent-browser.dev/snapshots)
- [agent-browser output](https://github.com/vercel-labs/agent-browser/blob/aff6125c023b810ea3f2e5deec5379e9a4270bdc/cli/src/output.rs)
- [agent-browser plugins](https://github.com/vercel-labs/agent-browser/blob/aff6125c023b810ea3f2e5deec5379e9a4270bdc/cli/src/plugins.rs)
