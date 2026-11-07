import { NextResponse, type NextFetchEvent, type NextRequest } from "next/server";
import { AsyncLocalStorage } from "node:async_hooks";

// A data-driven proxy without `config.matcher` (it runs for every path):
// it fetches from an upstream service before deciding to continue,
// redirect, rewrite (internally or to another origin) or answer itself,
// and schedules background work with waitUntil.
const upstream = process.env.NEXTRS_UPSTREAM;
const closed = process.env.NEXTRS_CLOSED_PORT ?? "9";
const unlisted = process.env.NEXTRS_UNLISTED_PORT ?? "10";
const identity = new AsyncLocalStorage<string>();
const delay = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

function at(request: NextRequest, path: string) {
  const url = request.nextUrl.clone();
  const [pathname, search] = path.split("?");
  url.pathname = pathname;
  url.search = search ? `?${search}` : "";
  return url;
}

const failure = (error: unknown) => {
  const e = error as { name?: string; message?: string; cause?: { code?: string } };
  return { name: e?.name ?? null, message: e?.message ?? null, code: e?.cause?.code ?? null };
};

async function flag(key: string, init?: RequestInit & { status?: number; delayMs?: number }) {
  const params = new URLSearchParams({ key });
  if (init?.status) params.set("status", String(init.status));
  if (init?.delayMs) params.set("delay", String(init.delayMs));
  return fetch(`${upstream}/data?${params}`, init);
}

export async function proxy(request: NextRequest, event: NextFetchEvent) {
  const { pathname, searchParams } = request.nextUrl;
  if (!pathname.startsWith("/gate/")) {
    const response = NextResponse.next();
    response.headers.set("x-platform-proxy", "seen");
    return response;
  }
  const name = pathname.slice("/gate/".length);

  if (name === "open") {
    const r = await flag("gate-open");
    const data = await r.json();
    const headers = new Headers(request.headers);
    headers.set("x-flag", `${data.key}:${r.status}`);
    return NextResponse.next({ request: { headers } });
  }
  if (name === "closed") {
    const r = await flag("gate-closed", { status: 403 });
    await r.text();
    if (!r.ok) return NextResponse.redirect(at(request, `/denied?from=closed&status=${r.status}`));
  }
  if (name.startsWith("status/")) {
    const code = Number(name.slice("status/".length));
    const r = await flag(`gate-status-${code}`, { status: code });
    await r.text();
    if (r.status === 401) return NextResponse.redirect(at(request, "/denied?from=401"));
    if (r.status === 404) return NextResponse.rewrite(at(request, "/variant/missing"));
    if (r.status >= 500) return NextResponse.json({ upstreamStatus: r.status }, { status: 502 });
    return NextResponse.rewrite(at(request, `/variant/ok-${r.status}`));
  }
  if (name === "variant") {
    const r = await flag("gate-variant");
    const data = await r.json();
    return NextResponse.rewrite(at(request, `/variant/${data.key}`));
  }
  if (name === "direct") {
    const r = await flag("gate-direct");
    const data = await r.json();
    return NextResponse.json({ key: data.key, upstreamStatus: r.status }, { status: 202, headers: { "x-direct": "1" } });
  }
  if (name === "timeout") {
    try {
      await flag("gate-timeout", { delayMs: 3000, signal: AbortSignal.timeout(200) });
      return NextResponse.json({ timedOut: false });
    } catch (error) {
      return NextResponse.json({ timedOut: true, error: failure(error) }, { status: 504 });
    }
  }
  if (name === "abort") {
    const controller = new AbortController();
    setTimeout(() => controller.abort(), 50);
    try {
      await flag("gate-abort", { delayMs: 3000, signal: controller.signal });
      return NextResponse.json({ aborted: false });
    } catch (error) {
      return NextResponse.json({ aborted: true, error: failure(error) }, { status: 503 });
    }
  }
  if (name === "refused" || name === "unlisted") {
    const target = name === "refused" ? `http://127.0.0.1:${closed}/x` : `http://127.0.0.1:${unlisted}/data?key=unlisted`;
    try {
      const r = await fetch(target);
      return NextResponse.json({ reached: true, status: r.status });
    } catch (error) {
      return NextResponse.json({ reached: false, error: failure(error) }, { status: 503 });
    }
  }
  if (name === "large") {
    const bytes = Number(searchParams.get("bytes") ?? 4 * 1024 * 1024);
    const r = await fetch(`${upstream}/large?bytes=${bytes}`);
    const body = await r.arrayBuffer();
    return NextResponse.json({ bytes: body.byteLength });
  }
  if (name === "identity") {
    // The request's identity must survive fetches, timers and promise
    // continuations, and reach the route as its own header.
    const id = searchParams.get("id") ?? "none";
    return identity.run(id, async () => {
      const seen: string[] = [];
      await flag(`identity-${id}`).then((r) => r.text());
      seen.push(identity.getStore() ?? "lost");
      await delay(Number(searchParams.get("ms") ?? 5));
      seen.push(identity.getStore() ?? "lost");
      await Promise.resolve().then(() => seen.push(identity.getStore() ?? "lost"));
      const headers = new Headers(request.headers);
      headers.set("x-identity", id);
      headers.set("x-identity-seen", [...new Set(seen)].join(","));
      const response = NextResponse.next({ request: { headers } });
      response.cookies.set("identity", id, { path: "/" });
      return response;
    });
  }
  if (name === "wait-until") {
    const key = searchParams.get("key") ?? "wu";
    event.waitUntil(
      (async () => {
        await delay(Number(searchParams.get("ms") ?? 300));
        await fetch(`${upstream}/mutations?key=${encodeURIComponent(key)}&id=wait-until`, { method: "POST" });
      })(),
    );
    return NextResponse.json({ scheduled: key });
  }
  if (name === "wait-until-fail") {
    event.waitUntil(Promise.reject(new Error("background failure (synthetic)")));
    return NextResponse.json({ scheduled: "failure" });
  }
  if (name === "wait-until-forever") {
    event.waitUntil(new Promise(() => {}));
    return NextResponse.json({ scheduled: "forever" });
  }
  if (name === "external") {
    const response = NextResponse.rewrite(new URL(`${upstream}/echo/from-proxy?via=proxy&${searchParams}`));
    response.headers.set("x-proxy-external", "1");
    response.cookies.set("proxy-external", "1", { path: "/" });
    return response;
  }
  if (name === "external-unlisted") {
    return NextResponse.rewrite(new URL(`http://127.0.0.1:${unlisted}/echo/unlisted`));
  }
  if (name === "spin") {
    // Busy CPU in the proxy (the soak's stall injection), not an await.
    const end = Date.now() + Math.min(Number(searchParams.get("ms") ?? 100), 30_000);
    while (Date.now() < end) {
      // spin
    }
    return NextResponse.json({ spun: true });
  }
  if (name === "instrumentation") {
    const state = (globalThis as { __platform?: { registrations: number } }).__platform;
    return NextResponse.json({ registrationsSeenByProxy: state?.registrations ?? 0 });
  }
  if (name.startsWith("post/")) return NextResponse.rewrite(at(request, `/posts/${name.slice("post/".length)}`));

  return NextResponse.next();
}
