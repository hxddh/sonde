// sonde: names the place in the program that sent each S3 request.
//
// Loaded with NODE_OPTIONS=--require by `sonde run --callers` (and the app's
// "Name the code"), only then. For the AWS SDK for JavaScript v3 it adds one
// header to each request to sonde, before it is signed: x-sonde-caller:
// <file>:<line> <function>, the first frame of the program's own code (not
// Node's, not an installed package's). sonde reads it and never forwards it
// to the store. A program loaded as CommonJS (require) is named; nothing
// else changes, and nothing here may stop the program.
"use strict";

(function sondeCallers() {
  let endpoint;
  try {
    endpoint = new URL(process.env.AWS_ENDPOINT_URL || "");
  } catch {
    return;
  }
  const path = require("node:path");
  const { fileURLToPath } = require("node:url");
  const { AsyncLocalStorage } = require("node:async_hooks");
  const Module = require("node:module");
  const here = __filename;
  const at = new AsyncLocalStorage();

  // the program's own code: not Node's, not a package's, not this file
  const own = (file) => file && file !== here && !file.startsWith("node:") && !/^internal[\\/]/.test(file) && !/[\\/]node_modules[\\/]/.test(file);

  function place() {
    const lines = String(new Error().stack || "").split("\n").slice(1);
    for (const line of lines) {
      const m = /^\s*at (?:(.+?) \()?(.+?):(\d+):\d+\)?$/.exec(line);
      if (!m) continue;
      let file = m[2];
      if (file.startsWith("file://")) {
        try {
          file = fileURLToPath(file);
        } catch {
          continue;
        }
      }
      if (!own(file)) continue;
      let shown = path.relative(process.cwd(), file);
      if (!shown || shown.startsWith("..") || path.isAbsolute(shown)) shown = file;
      // one separator on every system
      shown = shown.split(path.sep).join("/");
      const fn = (m[1] || "<anonymous>").replace(/^async /, "").replace(/^new /, "");
      // a header is ASCII: anything else is written as ?
      return `${shown}:${m[3]} ${fn}`.replace(/[^\x20-\x7e]/g, "?").slice(0, 200);
    }
    return null;
  }

  // only requests to sonde carry the header: never one to another store
  const toSonde = (req) => {
    try {
      const host = String(req.hostname || "");
      const port = String(req.port || (req.protocol === "https:" ? 443 : 80));
      const want = endpoint.port || (endpoint.protocol === "https:" ? "443" : "80");
      return port === want && (host === endpoint.hostname || host.endsWith(`.${endpoint.hostname}`));
    } catch {
      return false;
    }
  };

  const middleware = (next) => async (args) => {
    try {
      const p = at.getStore();
      const req = args && args.request;
      if (p && req && req.headers && toSonde(req)) req.headers["x-sonde-caller"] = p;
    } catch {
      // the request goes as it was
    }
    return next(args);
  };

  const patched = new WeakSet();
  function patch(Client) {
    if (typeof Client !== "function" || !Client.prototype || typeof Client.prototype.send !== "function" || patched.has(Client)) return;
    patched.add(Client);
    const send = Client.prototype.send;
    Client.prototype.send = function sondeSend(...args) {
      try {
        if (this && this.middlewareStack && !this.__sondeCaller) {
          this.middlewareStack.add(middleware, { step: "build", name: "sondeCaller", override: true });
          Object.defineProperty(this, "__sondeCaller", { value: true });
        }
        const p = place();
        if (p) return at.run(p, () => send.apply(this, args));
      } catch {
        // as if this file were not here
      }
      return send.apply(this, args);
    };
  }

  // every v3 client is a smithy Client (older ones an @aws-sdk/smithy-client one)
  const CLIENT = /(^|[\\/])@(?:smithy|aws-sdk)[\\/]smithy-client([\\/]|$)/;
  const load = Module._load;
  Module._load = function sondeLoad(request, parent, isMain) {
    const m = load.apply(this, arguments);
    try {
      if (CLIENT.test(String(request)) && m) patch(m.Client);
    } catch {
      // the module is what it was
    }
    return m;
  };
})();
