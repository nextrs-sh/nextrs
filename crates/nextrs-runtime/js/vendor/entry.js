// Exposes the vendored Node.js built-in implementations to bootstrap.js.
// Nothing here is installed as a global; bootstrap.js decides that.
globalThis.__nextrsVendor = {
  buffer: require("buffer"),
  events: require("events"),
  path: require("path-browserify"),
  stream: require("readable-stream"),
  string_decoder: require("string_decoder"),
};
