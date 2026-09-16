import { createRequire } from "node:module";
import { existsSync } from "node:fs";
import { fileURLToPath } from "node:url";
import type { ConnectOptions, CursorCommand, CursorOverlayAction, Delivery, Effect, Json, KeyChord, Middleware, NativeRequest, Next, Node, ObserveOptions, Operation, PointerAction, Receipt, SemanticAction, Snapshot, Target, Transport } from "./types.js";
export * from "./types.js";

export class NativeError extends Error {
  constructor(readonly code: string, message: string, readonly effect: Effect, options?: ErrorOptions) {
    super(message, options); this.name = "NativeError";
  }
}
function decode(text: string): Json {
  return JSON.parse(text, (_key, value: unknown) => {
    if (typeof value === "number" && Number.isInteger(value) && !Number.isSafeInteger(value)) {
      throw new NativeError("unsafe_integer", "Native result contains an integer outside JavaScript's safe range", "unknown");
    }
    return value;
  }) as Json;
}
function unwrap(text: string): Json {
  const envelope = decode(text) as { result?: Json; error?: { code: string; message: string; effect: Effect } };
  if (envelope.error) throw new NativeError(envelope.error.code, envelope.error.message, envelope.error.effect);
  if (!("result" in envelope)) throw new NativeError("invalid_response", "Missing result in native reply", "unknown");
  return envelope.result!;
}
function connectError(error: unknown): never {
  if (error instanceof Error) {
    let detail: {code?: string; message?: string; effect?: Effect} = {};
    try { detail = JSON.parse(error.message); } catch { /* Preserve addon loading errors below. */ }
    if (detail.code && detail.message && detail.effect) throw new NativeError(detail.code, detail.message, detail.effect, { cause: error });
  }
  throw error;
}
function wire(value: unknown): Json {
  const text = JSON.stringify(value, (_key, part: unknown) => {
    if (typeof part === "number" && (!Number.isFinite(part) || (Number.isInteger(part) && !Number.isSafeInteger(part)))) {
      throw new NativeError("invalid_request", "Numbers must be finite and integer values must be safe integers", "none");
    }
    return part;
  });
  if (text === undefined) throw new NativeError("invalid_request", "A JSON value is required", "none");
  return JSON.parse(text) as Json;
}
interface NativeHandle { request(request: string): Promise<string>; close(): Promise<string> }
interface Addon { connect(options: string): Promise<NativeHandle> }
async function nativeTransport(options: ConnectOptions): Promise<Transport> {
  const binary = fileURLToPath(new URL("../native/actuate.node", import.meta.url));
  if (!existsSync(binary)) throw new NativeError("binding_not_installed", "Native addon missing. Run bun run --cwd node_modules/actuate install-native, or build from source.", "none");
  const addon = createRequire(import.meta.url)(binary) as Addon;
  let native: NativeHandle;
  try {
    native = await addon.connect(JSON.stringify({
      provider: options.provider ?? "native", device: options.device,
      device_set: options.deviceSet, credentials: options.credentials,
      trust_first_connection: options.trustFirstConnection ?? false,
    }));
  } catch (error) { connectError(error); }
  return {
    request: async request => unwrap(await native.request(JSON.stringify(request))),
    close: async () => { unwrap(await native.close()); },
  };
}

export class Session {
  private queue: Promise<unknown> = Promise.resolve();
  private closing?: Promise<void>;
  private readonly execute: Next;
  constructor(private readonly transport: Transport, middleware: readonly Middleware[] = []) {
    this.execute = middleware.reduceRight<Next>((next, wrap) => request => wrap(request, next), request => transport.request(request));
  }
  /** Calls are queued in invocation order, including close. */
  request(request: NativeRequest): Promise<Json> {
    if (this.closing) return Promise.reject(new NativeError("session_closed", "The session is closed", "none"));
    const owned = wire(request) as NativeRequest;
    const result = this.queue.then(() => this.execute(owned));
    this.queue = result.catch(() => {});
    return result;
  }
  run<Input, Output>(operation: Operation<Input, Output>, input: Input): Promise<Output> {
    return this.request(operation.request(input)).then(operation.parse);
  }
  capabilities(): Promise<Json> { return this.request({ op: "capabilities" }); }
  discover(): Promise<Json> { return this.request({ op: "discover" }); }
  windows(): Promise<Json> { return this.request({ op: "windows" }); }
  displays(): Promise<Json> { return this.request({ op: "displays" }); }
  observe(options: ObserveOptions): Promise<Snapshot> {
    return this.request({ op: "observe", request: { pid: options.pid, max_nodes: options.maxNodes ?? 1000, max_depth: options.maxDepth ?? 30 },
      ...(options.windowId === undefined ? {} : { window_id: options.windowId }) }) as unknown as Promise<Snapshot>;
  }
  inspect(target: Target): Promise<Node> { return this.request({ op: "inspect", target: wire(target) }) as unknown as Promise<Node>; }
  semantic(target: Target, action: SemanticAction): Promise<Receipt> {
    return this.request({ op: "semantic", target: wire(target), action: wire(action) }) as unknown as Promise<Receipt>;
  }
  pointer(delivery: Delivery, action: PointerAction): Promise<Receipt> {
    return this.request({ op: "pointer", delivery: wire(delivery), action: wire(action) }) as unknown as Promise<Receipt>;
  }
  text(delivery: Delivery, text: string): Promise<Receipt> {
    return this.request({ op: "text", delivery: wire(delivery), text }) as unknown as Promise<Receipt>;
  }
  key(delivery: Delivery, chord: KeyChord): Promise<Receipt> {
    return this.request({ op: "key", delivery: wire(delivery), chord: wire(chord) }) as unknown as Promise<Receipt>;
  }
  capture(path: string, options: Record<string, Json> = {}): Promise<Json> { return this.request({ ...options, op: "capture", path }); }
  cursor(command: CursorCommand): Promise<Json> { return this.request({ op: "cursor", command: wire(command) }); }
  cursorOverlay(action: CursorOverlayAction): Promise<Json> { return this.request({ op: "cursor_overlay", action: wire(action) }); }
  cursorState(): Promise<Json> { return this.request({ op: "cursor_state" }); }
  snapshots(): Promise<Json> { return this.request({ op: "snapshots" }); }
  diff(before: number, after?: number): Promise<Json> { return this.request({ op: "diff", before, after }); }
  close(): Promise<void> {
    return this.closing ??= this.queue.then(() => this.transport.close());
  }
  [Symbol.asyncDispose](): Promise<void> { return this.close(); }
}
export const Actuate = {
  async connect(options: ConnectOptions = {}): Promise<Session> { return new Session(await nativeTransport(options)); },
  fromTransport(transport: Transport, options: { middleware?: readonly Middleware[] } = {}): Session {
    return new Session(transport, options.middleware);
  },
};
