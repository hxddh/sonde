// A stand-in for the AWS SDK's smithy Client (its send runs the middleware
// stack the hook adds to), for callers_check.mjs.
class Client {
  constructor() {
    const added = [];
    this.middlewareStack = {
      add(fn) {
        added.push(fn);
      },
      resolve(end) {
        return added.reduceRight((next, fn) => fn(next), end);
      },
    };
  }
  send(command) {
    return command(this.middlewareStack);
  }
}
exports.Client = Client;
