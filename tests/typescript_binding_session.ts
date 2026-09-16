// Fixture transport adapter exercising the compiled napi-rs binding.
import { createInterface } from "node:readline";
import { Actuate, NativeError } from "../bindings/typescript/src/index.js";
const session = await Actuate.connect();
try {
  for await (const line of createInterface({ input: process.stdin })) {
    const {id, ...request} = JSON.parse(line);
    try { console.log(JSON.stringify({ id, result: await session.request(request) })); }
    catch (error) {
      if (!(error instanceof NativeError)) throw error;
      console.log(JSON.stringify({id, error:{code:error.code,message:error.message,effect:error.effect}}));
    }
  }
} finally { await session.close(); }
