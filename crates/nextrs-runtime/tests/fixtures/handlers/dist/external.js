// External (ArrayBuffer) memory tests (RE-4D). x-mode:
//   alloc   one x-mib ArrayBuffer, kept with x-retain: 1
//   churn   x-count x-mib Uint8Arrays, none kept
//   fill    keep x-mib ArrayBuffers until one is refused (or 4 GiB)
//   release drop everything kept
//   apis    with the budget exhausted (run fill first), try a list of
//           Web and Node APIs that allocate buffers; report each outcome
// Errors are caught and reported as JSON, so a refusal is a 200 here.
globalThis.__external ??= { retained: [] };

function reply(res, body) {
  res.setHeader("content-type", "application/json");
  res.end(JSON.stringify(body));
}

const MIB = 1024 * 1024;

exports.handler = async (req, res) => {
  const state = globalThis.__external;
  const mib = Number(req.headers["x-mib"] ?? 1);
  try {
    switch (req.headers["x-mode"]) {
      case "alloc": {
        const buffer = new ArrayBuffer(mib * MIB);
        new Uint8Array(buffer).fill(1, 0, 4096);
        if (req.headers["x-retain"] === "1") state.retained.push(buffer);
        return reply(res, { ok: true });
      }
      case "churn": {
        const count = Number(req.headers["x-count"] ?? 1);
        let total = 0;
        for (let i = 0; i < count; i++) {
          const view = new Uint8Array(mib * MIB);
          view[i % view.length] = 1;
          total += view.length;
        }
        return reply(res, { ok: true, total });
      }
      case "fill": {
        let held = 0;
        try {
          while (held < 4096) {
            state.retained.push(new ArrayBuffer(mib * MIB));
            held += mib;
          }
        } catch (error) {
          return reply(res, { ok: true, held, error: error.name, message: error.message });
        }
        return reply(res, { ok: false, held });
      }
      case "release":
        state.retained = [];
        return reply(res, { ok: true });
      case "apis": {
        const tries = {
          small_typed_array_buffer: () => new Uint8Array(16).buffer.byteLength,
          small_buffer_alloc: () => Buffer.alloc(1024).length,
          buffer_from_string: () => Buffer.from("x".repeat(1000)).length,
          text_encoder_large: () => new TextEncoder().encode("x".repeat(200000)).length,
          typed_array_large: () => new Uint8Array(MIB).length,
          buffer_alloc_large: () => Buffer.alloc(MIB).length,
          buffer_alloc_unsafe_large: () => Buffer.allocUnsafe(MIB).length,
          buffer_concat_small: () => Buffer.concat([Buffer.alloc(1000), Buffer.alloc(1000)]).length,
          buffer_concat_large: () => Buffer.concat([Buffer.alloc(1000), Buffer.alloc(MIB)]).length,
          array_buffer_slice: () => new ArrayBuffer(100).slice(0, 10).byteLength,
          structured_clone_small: () => structuredClone(new Uint8Array(32)).length,
          structured_clone_large: () => structuredClone(new Uint8Array(MIB)).length,
          random_values: () => crypto.getRandomValues(new Uint8Array(65536)).length,
          json_large: () => JSON.stringify({ a: "x".repeat(100000) }).length,
          data_view_large: () => new DataView(new ArrayBuffer(MIB)).byteLength,
          resizable_large: () => new ArrayBuffer(10, { maxByteLength: 4 * MIB }).byteLength,
        };
        const out = {};
        for (const [name, run] of Object.entries(tries)) {
          try {
            out[name] = { ok: true, value: run() };
          } catch (error) {
            out[name] = { ok: false, error: error.name };
          }
        }
        for (const [name, run] of Object.entries({
          response_array_buffer: async () =>
            (await new Response("x".repeat(200000)).arrayBuffer()).byteLength,
          blob_array_buffer: async () =>
            (await new Blob(["x".repeat(200000)]).arrayBuffer()).byteLength,
        })) {
          try {
            out[name] = { ok: true, value: await run() };
          } catch (error) {
            out[name] = { ok: false, error: error.name };
          }
        }
        return reply(res, out);
      }
    }
    reply(res, { ok: false, error: "unknown mode" });
  } catch (error) {
    reply(res, { ok: false, error: error.name, message: error.message });
  }
};
