// A small server-rendered response for the dispatch microbenchmark
// (benches/dispatch.rs): headers, then HTML written as strings in chunks
// (the path Next's renderer takes through Node's response and Buffer).
// x-chunks: number of 2 KiB chunks (default 8).
"use strict";

const chunk = `<div class="row"><span>Ünïcödé — 日本語</span>${"<p>lorem ipsum dolor sit amet</p>".repeat(56)}</div>\n`;

exports.handler = async (req, res) => {
  const chunks = Number(req.headers["x-chunks"] ?? 8);
  res.statusCode = 200;
  res.setHeader("content-type", "text/html; charset=utf-8");
  res.setHeader("cache-control", "private, no-cache, no-store, max-age=0, must-revalidate");
  res.setHeader("x-bench", req.headers["x-id"] ?? "-");
  res.write("<!DOCTYPE html><html><body>");
  for (let i = 0; i < chunks; i++) res.write(chunk);
  res.end("</body></html>");
};
