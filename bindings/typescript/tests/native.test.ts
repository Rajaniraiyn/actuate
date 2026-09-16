import { expect, test } from "bun:test";
import { Actuate } from "../src/index.js";

test("compiled addon validates connection options", async () => {
  await expect(Actuate.connect({provider:"native",device:"invalid"})).rejects.toMatchObject({code:"invalid_request",effect:"none"});
});

test.skipIf(process.platform === "linux")("compiled addon preserves provider errors and session closure", async () => {
  const session = await Actuate.connect();
  try {
    expect(await session.capabilities()).toBeObject();
    await expect(session.request({op:"does_not_exist"})).rejects.toMatchObject({code:"invalid_request",effect:"none"});
    expect(await session.capabilities()).toBeObject();
  } finally { await session.close(); }
  await expect(session.capabilities()).rejects.toMatchObject({code:"session_closed"});
});
