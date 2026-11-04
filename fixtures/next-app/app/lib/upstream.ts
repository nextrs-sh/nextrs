// The upstream API the data fixtures fetch from (tests/compat/upstream.mjs).
// Read per request: NEXTRS_UPSTREAM is set by the test harness.
export function upstream(path: string): string {
  const base = process.env.NEXTRS_UPSTREAM;
  if (!base) throw new Error("NEXTRS_UPSTREAM is not set");
  return `${base}${path}`;
}

type NextFetchInit = RequestInit & { next?: { revalidate?: number | false; tags?: string[] } };

// Fetch options from query parameters: cache=<mode>, revalidate=<s>, tag=<t>.
export function fetchOptions(params: URLSearchParams, headers?: Record<string, string>): NextFetchInit {
  const init: NextFetchInit = {};
  const cache = params.get("cache");
  if (cache) init.cache = cache as RequestCache;
  const revalidate = params.get("revalidate");
  const tags = params.getAll("tag");
  if (revalidate !== null || tags.length) {
    init.next = { tags };
    if (revalidate !== null) init.next.revalidate = Number(revalidate);
  }
  if (headers && Object.keys(headers).length) init.headers = headers;
  return init;
}
