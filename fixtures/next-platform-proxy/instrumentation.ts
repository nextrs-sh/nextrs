// Registration and request-error hooks with observable effects: a counter
// and a list on globalThis (per process under next start, per isolate under
// NextRS), read back by /api/instrumentation and by the proxy.
type State = { registrations: number; registeredAt: number; errors: { message: string; path: string; routeType: string }[] };
const state = () => ((globalThis as { __platform?: State }).__platform ??= { registrations: 0, registeredAt: 0, errors: [] });

export async function register() {
  const s = state();
  s.registrations += 1;
  s.registeredAt = Date.now();
}

export async function onRequestError(
  error: unknown,
  request: { path: string },
  context: { routeType: string },
) {
  state().errors.push({ message: (error as Error)?.message ?? String(error), path: request.path, routeType: context.routeType });
}
