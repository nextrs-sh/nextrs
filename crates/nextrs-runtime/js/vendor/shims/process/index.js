// readable-stream requires `process/`; route it to the runtime's process
// object so nextTick keeps the caller's async context.
module.exports = globalThis.process;
