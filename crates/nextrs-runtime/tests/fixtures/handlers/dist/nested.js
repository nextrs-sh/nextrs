// Nested-request tests (RE-5 prerequisite): a parent fetches a child from
// its own server with Next's internal marker, as a redirecting Server
// Action does. /parent?children=N fetches /child N times at once and
// answers their bodies; /recurse?depth=N fetches /recurse?depth=N-1 until
// 0. x-delay delays the child (ms). /report answers the most parents and
// children that ran at once.
globalThis.__nested ??= { parents: 0, children: 0, maxParents: 0, maxChildren: 0 };

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

exports.handler = async (req, res) => {
  const state = globalThis.__nested;
  const url = new URL(req.url, "http://localhost");
  if (url.pathname === "/report") {
    res.end(JSON.stringify({ maxParents: state.maxParents, maxChildren: state.maxChildren }));
    return;
  }
  if (url.pathname === "/child") {
    state.children++;
    state.maxChildren = Math.max(state.maxChildren, state.children);
    try {
      await sleep(Number(req.headers["x-delay"] ?? 0));
      res.end("child");
    } finally {
      state.children--;
    }
    return;
  }
  if (url.pathname === "/recurse") {
    const depth = Number(url.searchParams.get("depth"));
    if (depth <= 0) {
      res.end("bottom");
      return;
    }
    try {
      const reply = await fetch(`http://localhost/recurse?depth=${depth - 1}`, {
        next: { internal: 1 },
      });
      res.statusCode = reply.status;
      res.end(`${depth}>${await reply.text()}`);
    } catch (error) {
      res.statusCode = 502;
      res.end(`${depth}!${error.message}`);
    }
    return;
  }
  // A parent.
  state.parents++;
  state.maxParents = Math.max(state.maxParents, state.parents);
  try {
    const children = Number(url.searchParams.get("children") ?? 1);
    const delay = req.headers["x-delay"] ?? "0";
    const replies = await Promise.all(
      Array.from({ length: children }, () =>
        fetch("http://localhost/child", { next: { internal: 1 }, headers: { "x-delay": delay } }),
      ),
    );
    const bodies = await Promise.all(replies.map((r) => r.text()));
    res.end(replies.map((r, i) => `${r.status}:${bodies[i]}`).join(","));
  } finally {
    state.parents--;
  }
};
