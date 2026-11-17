import { expect, type Page } from '@playwright/test';

/** Collects what a clean page must not produce: page errors, console
 * errors, failed requests (other than ones the test aborts), and React
 * hydration warnings. */
export function guard(page: Page) {
  const problems: string[] = [];
  page.on('pageerror', (e) => problems.push(`pageerror: ${e.message}`));
  page.on('console', (m) => {
    const text = m.text();
    if (m.type() === 'error' || /hydrat/i.test(text)) problems.push(`console ${m.type()}: ${text}`);
  });
  page.on('requestfailed', (r) => {
    const failure = r.failure()?.errorText ?? '';
    // Navigations abort in-flight prefetches; that is not a failure.
    if (!/ERR_ABORTED/.test(failure)) problems.push(`requestfailed: ${r.url()} ${failure}`);
  });
  page.on('response', (r) => {
    if (r.status() >= 500) problems.push(`HTTP ${r.status()}: ${r.url()}`);
  });
  return { assertClean: () => expect(problems, problems.join('\n')).toEqual([]) };
}
