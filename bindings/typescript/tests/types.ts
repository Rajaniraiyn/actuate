import { Actuate, defineOperation, observeOptions, type Json, type Delivery, type SemanticAction } from "../src/index.js";
const options = observeOptions({ pid: 123, maxNodes: 10 });
const operation = defineOperation({request: (name: string) => ({op:"extension",name}), parse: (value: Json) => String(value)});
async function types() {
 const session = await Actuate.connect();
 const snapshot = await session.observe(options);
 const text: string = await session.run(operation, "name");
 // @ts-expect-error Extension input is inferred.
 await session.run(operation, 123);
 // @ts-expect-error Branded shape prevents using a point as an element.
 await session.inspect({x:1,y:2});
 // @ts-expect-error Action payload is discriminated by kind.
 const badAction: SemanticAction = {kind:"perform",value:true};
 // @ts-expect-error Delivery mode requires its PID.
 const badDelivery: Delivery = {kind:"process"};
 await session.close();
}
