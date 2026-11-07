// Registered once per runtime instance; route errors are reported here.
declare global {
  // eslint-disable-next-line no-var
  var __jwksRegistrations: number | undefined;
}

export function register() {
  globalThis.__jwksRegistrations = (globalThis.__jwksRegistrations ?? 0) + 1;
}

export function onRequestError(error: unknown, request: { path: string }) {
  console.error(`request error on ${request.path}:`, (error as Error)?.message);
}
