// RE-5C: multipart bodies are parsed in place (no copy of the body). The
// parts JavaScript gets must still be its own: changing the source bytes
// after formData() returns changes no part. A 1 MiB file part, a text
// field, and 200 small fields; the source array is then overwritten.
exports.handler = async (_req, res) => {
  const enc = new TextEncoder();
  const file = new Uint8Array(1024 * 1024);
  for (let i = 0; i < file.length; i++) file[i] = (i * 31 + 7) & 0xff;
  const pieces = [enc.encode('--B\r\nContent-Disposition: form-data; name="f"; filename="big.bin"\r\nContent-Type: application/octet-stream\r\n\r\n'), file, enc.encode('\r\n--B\r\nContent-Disposition: form-data; name="t"\r\n\r\ncafé 📚\r\n')];
  for (let i = 0; i < 200; i++) pieces.push(enc.encode(`--B\r\nContent-Disposition: form-data; name="k${i}"\r\n\r\nv${i}\r\n`));
  pieces.push(enc.encode("--B--\r\n"));
  const total = pieces.reduce((n, p) => n + p.length, 0);
  const body = new Uint8Array(total);
  let at = 0;
  for (const p of pieces) { body.set(p, at); at += p.length; }
  const form = await new Response(body, { headers: { "content-type": "multipart/form-data; boundary=B" } }).formData();
  body.fill(0x58);
  const part = new Uint8Array(await form.get("f").arrayBuffer());
  let same = part.length === file.length;
  for (let i = 0; same && i < part.length; i++) same = part[i] === file[i];
  const fields = [];
  for (let i = 0; i < 200; i++) if (form.get(`k${i}`) !== `v${i}`) fields.push(i);
  res.end(JSON.stringify({ fileIntact: same, fileSize: part.length, text: form.get("t"), wrongFields: fields, keys: [...form.keys()].length }));
};
