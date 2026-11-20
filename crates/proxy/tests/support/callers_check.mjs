// Run by callers.rs's test with sonde's hook in NODE_OPTIONS: an ES module
// that imports the SDK's client (as Node loads it, CommonJS) and one that
// requires it, each sending a request to AWS_ENDPOINT_URL. Prints, for each,
// the x-sonde-caller the request was given.
import { createRequire } from "node:module";
import { Client as Imported } from "./sdk/@smithy/core/dist-cjs/submodules/client/index.js";

const { Client: Required } = createRequire(import.meta.url)("./sdk/@smithy/smithy-client/dist-cjs/index.js");
const to = new URL(process.env.AWS_ENDPOINT_URL);
const request = () => ({ hostname: to.hostname, port: to.port, protocol: to.protocol, headers: {} });
const ask = (client) =>
  client.send((stack) => stack.resolve(async (args) => args.request.headers["x-sonde-caller"] ?? "-")({ request: request() }));

async function imported() {
  return ask(new Imported());
}
async function required() {
  return ask(new Required());
}
console.log("import", await imported());
console.log("require", await required());
