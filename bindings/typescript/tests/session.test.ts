import { describe, expect, test } from "bun:test";
import { Actuate, NativeError, observeOptions, defineOperation, type Transport, type Json } from "../src/index.js";
import { createHash } from "node:crypto";
import { checkedAsset, target } from "../scripts/install.mjs";

test("middleware order, typed extensions, request ownership, close ordering", async () => {
  const events: string[] = [];
  const transport: Transport = {
    async request(request) { events.push(String(request.op)); return request.value ?? null; },
    async close() { events.push("close"); },
  };
  const session = Actuate.fromTransport(transport, { middleware: [
    async (request, next) => { events.push("outer"); const result = await next(request); events.push("outer-end"); return result; },
    async (request, next) => { events.push("inner"); return next(request); },
  ] });
  const request = { op: "echo", value: 3 };
  const first = session.request(request);
  request.value = 7;
  expect(await first).toBe(3);
  const operation = defineOperation({ request: (value: number) => ({ op: "echo", value }), parse: (result: Json) => {
    if (typeof result !== "number") throw new Error("Expected number"); return result;
  } });
  const reply: number = await session.run(operation, 4);
  expect(reply).toBe(4);
  await Promise.all([session.close(), session.close()]);
  expect(events).toEqual(["outer", "inner", "echo", "outer-end", "outer", "inner", "echo", "outer-end", "close"]);
  await expect(session.capabilities()).rejects.toMatchObject({code:"session_closed",effect:"none"});
});

test("native errors and invalid input retain their effect", async () => {
  const session = Actuate.fromTransport({ async request() { throw new NativeError("sent", "uncertain", "unknown"); }, async close() {} });
  await expect(session.capabilities()).rejects.toMatchObject({ code: "sent", effect: "unknown" });
  expect(() => session.request({ op: "pointer", value: NaN })).toThrow();
  expect(() => session.request({ op: "pointer", value: Number.MAX_SAFE_INTEGER + 1 })).toThrow();
  await session.close();
  expect(Object.isFrozen(observeOptions({ pid: 1 }))).toBe(true);
});

test("installer rejects unsupported platforms and corrupt bytes", async () => {
  expect(target("win32", "x64")).toBe("x86_64-pc-windows-msvc");
  expect(target("win32", "arm64")).toBe("aarch64-pc-windows-msvc");
  expect(() => target("linux", "riscv64")).toThrow();
  const bytes = Buffer.from("native-addon");
  const manifest = {assets:{"addon.node":{size:bytes.length,sha256:createHash("sha256").update(bytes).digest("hex")}}};
  expect(await checkedAsset("addon.node", manifest, async () => bytes)).toEqual(bytes);
  await expect(checkedAsset("addon.node", manifest, async () => Buffer.from("bad"))).rejects.toThrow("Checksum mismatch");
});
