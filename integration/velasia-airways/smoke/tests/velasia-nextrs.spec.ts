import { expect, test, type Page, type Request, type Response } from '@playwright/test';

/*
 * Velasia Airways through NextRS (@nextrs/cli@0.1.0-rc.1): browser smoke tests.
 *
 * Only existing Velasia routes and UI are used (routes from apps/web/src/app, selectors from
 * Velasia's own tests/e2e, read-only). Dates are relative to the real "today" in Asia/Manila,
 * because this environment does not pin VELASIA_TODAY. If it is pinned (.env), set
 * SMOKE_TODAY to the same date and the browser clock is pinned to match.
 *
 * Every test records uncaught page errors, console errors (React hydration errors are console
 * errors) and failed requests, and fails on any that are not expected by that test.
 */

const TODAY =
  process.env.SMOKE_TODAY ??
  new Date().toLocaleDateString('en-CA', { timeZone: 'Asia/Manila' });
const addDays = (days: number) =>
  new Date(Date.parse(`${TODAY}T00:00:00Z`) + days * 86_400_000).toISOString().slice(0, 10);
const DEPART = addDays(21);
const SEARCH = `/flights?trip=one-way&from=MNL&to=SIN&depart=${DEPART}&adults=1&cabin=economy`;
const API_FLIGHTS = `/api/flights?from=MNL&to=SIN&departureDate=${DEPART}&adults=1`;

type Problems = { errors: string[]; rsc: string[]; documents: string[]; appCspProbe: number };

/*
 * Known app-level (not runtime) console message, Firefox only: zod v4 probes whether eval is
 * allowed with `try { Function("") } catch {}` in a client chunk. Velasia's CSP has no
 * 'unsafe-eval', so the probe is blocked (as intended) and caught; Firefox still logs the CSP
 * report as a console error, Chromium does not. The chunk bytes come from `next build`, so the
 * official Next.js server shows the same. Counted separately and reported, never hidden.
 */
const APP_CSP_EVAL_PROBE =
  /Content-Security-Policy: The page’s settings blocked a JavaScript eval \(script-src\).*Missing 'unsafe-eval'/;

function watch(page: Page, allow: RegExp[] = []): Problems {
  const p: Problems = { errors: [], rsc: [], documents: [], appCspProbe: 0 };
  const allowed = (text: string) => {
    if (APP_CSP_EVAL_PROBE.test(text)) {
      p.appCspProbe += 1;
      try {
        test.info().annotations.push({ type: 'app-csp-eval-probe', description: text.slice(0, 200) });
      } catch {
        // between tests (listener from beforeAll): still counted in p.appCspProbe
      }
      return true;
    }
    return allow.some((re) => re.test(text));
  };
  page.on('pageerror', (e) => {
    const t = `pageerror: ${e.message}`;
    if (!allowed(t)) p.errors.push(t);
  });
  page.on('console', (m) => {
    if (m.type() !== 'error') return;
    const t = `console: ${m.text()}`;
    if (!allowed(t)) p.errors.push(t);
  });
  page.on('requestfailed', (r: Request) => {
    const t = `requestfailed: ${r.method()} ${r.url()} (${r.failure()?.errorText})`;
    // Aborted prefetches/navigations are normal when a newer navigation supersedes them.
    if (/ERR_ABORTED|NS_BINDING_ABORTED|cancelled/i.test(t)) return;
    if (!allowed(t)) p.errors.push(t);
  });
  page.on('response', (r: Response) => {
    const t = `http ${r.status()}: ${r.request().method()} ${r.url()}`;
    if (r.status() >= 400 && !allowed(t)) p.errors.push(t);
  });
  page.on('request', (r) => {
    const h = r.headers();
    if (h['rsc'] === '1' || /[?&]_rsc=/.test(r.url())) p.rsc.push(`${r.method()} ${r.url()}`);
    if (r.isNavigationRequest() && r.frame() === page.mainFrame())
      p.documents.push(r.url());
  });
  return p;
}

test.beforeEach(async ({ page }) => {
  if (process.env.SMOKE_TODAY)
    await page.clock.setFixedTime(new Date(`${process.env.SMOKE_TODAY}T10:00:00+08:00`));
});

const shown = (page: Page, testId: string) => page.getByTestId(testId).filter({ visible: true });

async function waitForSearch(page: Page) {
  await expect(page.locator('[data-testid="flight-search"][data-ready="true"]').first()).toBeAttached();
}

// ---------------------------------------------------------------------------------------------
test.describe('runtime and delivery', () => {
  test('homepage renders server HTML that is the Velasia app', async ({ request }) => {
    const r = await request.get('/');
    expect(r.status()).toBe(200);
    expect(r.headers()['content-type']).toContain('text/html');
    const html = await r.text();
    expect(html).toMatch(/Beyond every/);
    expect(html).toContain('/_next/static/');
    expect(r.headers()['x-powered-by']).toBeUndefined();
  });

  test('HTML, CSS and JavaScript are delivered and applied', async ({ page }) => {
    const problems = watch(page);
    const assets: { url: string; status: number; type: string; cache: string }[] = [];
    page.on('response', (r) => {
      if (r.url().includes('/_next/static/'))
        assets.push({
          url: r.url(),
          status: r.status(),
          type: r.headers()['content-type'] ?? '',
          cache: r.headers()['cache-control'] ?? '',
        });
    });
    await page.goto('/');
    await page.waitForLoadState('networkidle');
    const css = assets.filter((a) => /\.css(\?|$)/.test(a.url));
    const js = assets.filter((a) => /\.js(\?|$)/.test(a.url));
    expect(css.length, 'stylesheets').toBeGreaterThan(0);
    expect(js.length, 'scripts').toBeGreaterThan(0);
    for (const a of [...css, ...js]) {
      expect(a.status, a.url).toBe(200);
      expect(a.cache, a.url).toContain('immutable');
    }
    expect(css.every((a) => a.type.includes('text/css'))).toBe(true);
    expect(js.every((a) => /javascript/.test(a.type))).toBe(true);
    // CSS applied: the document's stylesheets have rules, and body is not unstyled.
    const rules = await page.evaluate(() =>
      [...document.styleSheets].reduce((n, s) => n + (s.cssRules?.length ?? 0), 0),
    );
    expect(rules).toBeGreaterThan(50);
    const font = await page.evaluate(() => getComputedStyle(document.body).fontFamily);
    expect(font).not.toMatch(/^(serif|"Times New Roman")$/);
    expect(problems.errors).toEqual([]);
  });

  test('homepage hydrates without hydration or console errors', async ({ page }) => {
    const problems = watch(page);
    await page.goto('/');
    await expect(
      page.getByRole('heading', { level: 1, name: /Beyond every\s*horizon\./ }),
    ).toBeVisible();
    await expect(page.getByRole('search', { name: 'Search flights' })).toBeVisible();
    // data-ready is set by a client effect: present only after React hydrated the widget.
    await waitForSearch(page);
    await page.waitForLoadState('networkidle');
    expect(problems.errors.filter((e) => /hydrat/i.test(e))).toEqual([]);
    expect(problems.errors).toEqual([]);
  });

  test('public images and next/image optimisation', async ({ page, request }) => {
    await page.goto('/');
    const src = await page
      .locator('img[src*="/_next/image"], img[srcset*="/_next/image"]')
      .first()
      .evaluate((img: HTMLImageElement) => img.currentSrc || img.src);
    const optimised = await request.get(src, { headers: { accept: 'image/avif,image/webp,*/*' } });
    expect(optimised.status(), src).toBe(200);
    expect(optimised.headers()['content-type']).toMatch(/^image\//);
    const original = new URL(src).searchParams.get('url')!;
    const raw = await request.get(original);
    expect(raw.status(), original).toBe(200);
    expect(raw.headers()['content-type']).toMatch(/^image\//);
  });

  test('metadata routes: robots.txt, sitemap.xml, manifest', async ({ request }) => {
    const robots = await request.get('/robots.txt');
    expect(robots.status()).toBe(200);
    expect(await robots.text()).toMatch(/Sitemap: http:\/\/localhost:3000\/sitemap\.xml/);
    const sitemap = await request.get('/sitemap.xml');
    expect(sitemap.status()).toBe(200);
    expect(await sitemap.text()).toContain('<urlset');
    const manifest = await request.get('/manifest.webmanifest');
    expect(manifest.status()).toBe(200);
    expect((await manifest.json()).name).toMatch(/Velasia/);
  });

  test('security headers from next.config are sent', async ({ request }) => {
    for (const path of ['/', SEARCH, API_FLIGHTS]) {
      const h = (await request.get(path, { maxRedirects: 0 })).headers();
      expect(h['content-security-policy'], path).toContain("default-src 'self'");
      expect(h['content-security-policy'], path).not.toContain('unsafe-eval');
      expect(h['x-frame-options'], path).toBe('DENY');
      expect(h['x-content-type-options'], path).toBe('nosniff');
      expect(h['referrer-policy'], path).toBe('strict-origin-when-cross-origin');
    }
  });

  test('cache-control: static homepage cacheable, personal pages private', async ({ request }) => {
    expect((await request.get('/')).headers()['cache-control']).toMatch(/s-maxage=\d+/);
    for (const path of ['/booking/travellers', '/manage-booking', SEARCH]) {
      const cc = (await request.get(path)).headers()['cache-control'] ?? '';
      expect(cc, path).toMatch(/private/);
      expect(cc, path).toMatch(/no-store/);
    }
  });
});

// ---------------------------------------------------------------------------------------------
test.describe('routing', () => {
  test('client-side navigation uses RSC payloads, no full reloads', async ({ page }) => {
    const problems = watch(page);
    const rscResponses: { url: string; status: number; type: string }[] = [];
    page.on('response', (r) => {
      const h = r.request().headers();
      if (h['rsc'] === '1')
        rscResponses.push({ url: r.url(), status: r.status(), type: r.headers()['content-type'] ?? '' });
    });
    await page.goto('/');
    await waitForSearch(page);
    await page.evaluate(() => ((window as unknown as { __smoke: number }).__smoke = 42));
    const docsBefore = problems.documents.length;
    const nav = page.getByRole('navigation', { name: 'Primary' });
    for (const [link, url, heading] of [
      ['Explore destinations', /\/destinations$/, 'Destinations across Asia'],
      ['Book flights', /\/flights$/, 'Where would you like to go?'],
      ['Manage booking', /\/manage-booking$/, 'Your trip, in your hands'],
      ['Help', /\/help$/, 'How can we help?'],
    ] as const) {
      await nav.getByRole('link', { name: link }).click();
      await expect(page).toHaveURL(url);
      await expect(page.getByRole('heading', { level: 1, name: heading })).toBeVisible();
      await expect(nav.getByRole('link', { name: link })).toHaveAttribute('aria-current', 'page');
    }
    await page.getByRole('link', { name: 'Velasia Airways home' }).first().click();
    await expect(page).toHaveURL(/\/$/);
    // Same JS realm (no document load) and RSC requests answered with the Flight format.
    expect(await page.evaluate(() => (window as unknown as { __smoke?: number }).__smoke)).toBe(42);
    expect(problems.documents.length).toBe(docsBefore);
    expect(rscResponses.length).toBeGreaterThan(0);
    for (const r of rscResponses) {
      expect(r.status, r.url).toBe(200);
      expect(r.type, r.url).toContain('text/x-component');
    }
    expect(problems.errors).toEqual([]);
  });

  test('dynamic route: destination detail, then refresh of the nested route', async ({ page }) => {
    const problems = watch(page);
    await page.goto('/destinations');
    const link = page.locator('main a[href^="/destinations/"]').first();
    const href = (await link.getAttribute('href'))!;
    await link.click();
    await expect(page).toHaveURL(new RegExp(`${href}$`));
    const h1 = page.getByRole('heading', { level: 1 });
    await expect(h1).toBeVisible();
    const title = await h1.textContent();
    const reload = await page.reload();
    expect(reload!.status()).toBe(200);
    await expect(page.getByRole('heading', { level: 1 })).toHaveText(title!);
    expect(problems.errors).toEqual([]);
  });

  test('browser back and forward navigation', async ({ page }) => {
    const problems = watch(page);
    await page.goto('/');
    await waitForSearch(page);
    const nav = page.getByRole('navigation', { name: 'Primary' });
    await nav.getByRole('link', { name: 'Explore destinations' }).click();
    await expect(page).toHaveURL(/\/destinations$/);
    await nav.getByRole('link', { name: 'Help' }).click();
    await expect(page).toHaveURL(/\/help$/);
    await page.goBack();
    await expect(page).toHaveURL(/\/destinations$/);
    await expect(page.getByRole('heading', { level: 1, name: 'Destinations across Asia' })).toBeVisible();
    await page.goBack();
    await expect(page).toHaveURL(/\/$/);
    await expect(page.getByRole('heading', { level: 1, name: /Beyond every/ })).toBeVisible();
    await page.goForward();
    await expect(page).toHaveURL(/\/destinations$/);
    await expect(page.getByRole('heading', { level: 1, name: 'Destinations across Asia' })).toBeVisible();
    expect(problems.errors).toEqual([]);
  });

  test('unknown page: real 404 status with the app not-found page', async ({ page }) => {
    // The 404 is expected: the network entry, and the browser's own console line about it.
    const problems = watch(page, [
      /http 404: GET .*\/this-page-does-not-exist/,
      /console: Failed to load resource: the server responded with a status of 404/,
    ]);
    const r = await page.goto('/this-page-does-not-exist');
    expect(r!.status()).toBe(404);
    await expect(page.getByRole('heading', { name: 'This page has flown off course' })).toBeVisible();
    expect(problems.errors).toEqual([]);
  });

  test('redirects from next.config', async ({ request }) => {
    const a = await request.get('/booking/travelers', { maxRedirects: 0 });
    expect(a.status()).toBe(308);
    expect(a.headers()['location']).toMatch(/\/booking\/travellers$/);
    const b = await request.get('/checkout', { maxRedirects: 0 });
    expect(b.status()).toBe(307);
    expect(b.headers()['location']).toMatch(/\/booking\/checkout$/);
  });

  test('mobile layout: menu dialog navigation', async ({ browser }) => {
    const context = await browser.newContext({ viewport: { width: 390, height: 844 }, isMobile: false });
    const page = await context.newPage();
    const problems = watch(page);
    await page.goto('/');
    await expect(page.getByRole('navigation', { name: 'Primary' })).toBeHidden();
    await page.getByRole('button', { name: 'Open menu' }).click();
    const menu = page.getByRole('dialog', { name: 'Menu' });
    await expect(menu).toBeVisible();
    await menu.getByRole('link', { name: 'Explore destinations' }).click();
    await expect(page).toHaveURL(/\/destinations$/);
    const overflow = await page.evaluate(() => document.documentElement.scrollWidth - window.innerWidth);
    expect(overflow).toBeLessThanOrEqual(1);
    expect(problems.errors).toEqual([]);
    await context.close();
  });
});

// ---------------------------------------------------------------------------------------------
test.describe('API route handlers (via NextRS, which calls the retailing API)', () => {
  test('reference data: airports, destinations, fare families', async ({ request }) => {
    for (const path of ['/api/airports', '/api/destinations', '/api/fare-families', '/api/aircraft']) {
      const r = await request.get(path);
      expect(r.status(), path).toBe(200);
      expect(r.headers()['content-type'], path).toContain('application/json');
      const body = await r.json();
      expect(JSON.stringify(body).length, path).toBeGreaterThan(20);
    }
  });

  test('live flight search JSON is private and has results', async ({ request }) => {
    const r = await request.get(API_FLIGHTS);
    expect(r.status()).toBe(200);
    expect(r.headers()['cache-control']).toBe('private, no-store');
    const body = await r.json();
    const list = body.data?.flights ?? body.data ?? body.flights ?? body;
    expect(JSON.stringify(list)).toMatch(/NV\d+/);
  });

  test('health endpoints', async ({ request }) => {
    expect((await request.get('/api/internal/health/live')).status()).toBe(200);
    expect((await request.get('/api/internal/health/ready')).status()).toBe(200);
  });

  test('invalid search returns a client error, not a crash', async ({ request }) => {
    const r = await request.get('/api/flights?from=MNL&to=MNL&departureDate=2001-01-01&adults=0');
    expect(r.status()).toBeGreaterThanOrEqual(400);
    expect(r.status()).toBeLessThan(500);
  });
});

// ---------------------------------------------------------------------------------------------
test.describe('flight search and results', () => {
  test('search form from the homepage shows results from the API', async ({ page }) => {
    const problems = watch(page);
    await page.goto('/');
    await waitForSearch(page);
    for (const [label, query, option] of [
      ['From', 'Manila', /Manila/],
      ['To', 'Singapore', /Singapore/],
    ] as const) {
      const input = page.getByRole('combobox', { name: label });
      await input.click();
      await input.fill(query);
      await page.getByRole('listbox', { name: `${label} airports` }).getByRole('option', { name: option }).first().click();
    }
    await page.getByLabel('Depart', { exact: true }).fill(DEPART);
    await page.getByLabel('Return', { exact: true }).fill(addDays(28));
    await page.getByRole('button', { name: 'Search flights' }).click();
    await expect(page).toHaveURL(/\/flights\?.*from=MNL.*to=SIN/);
    await expect(page.getByTestId('search-summary')).toBeVisible();
    await expect(page.getByTestId('flight-card').first()).toBeVisible();
    expect(await page.getByTestId('flight-card').count()).toBeGreaterThan(0);
    expect(problems.errors).toEqual([]);
  });

  test('results filter (stops) and nearby-dates navigation', async ({ page }) => {
    const problems = watch(page);
    await page.goto(`/flights?trip=one-way&from=CEB&to=SIN&depart=${addDays(22)}&adults=1&cabin=economy`);
    await expect(page.getByTestId('flight-card').first()).toBeVisible();
    const all = await page.getByTestId('flight-card').count();
    await page.getByRole('radio', { name: /Direct flights only/ }).check();
    await expect(page.locator('[data-pending]')).toHaveCount(0);
    await expect(page).toHaveURL(/stops=/);
    const direct = await page.getByTestId('flight-card').count();
    expect(direct).toBeLessThanOrEqual(all);
    await page.getByRole('radio', { name: /Any number of stops/ }).check();
    await expect(page.locator('[data-pending]')).toHaveCount(0);
    const strip = page.getByRole('navigation', { name: 'Nearby dates' });
    await strip.getByRole('link').last().click();
    await expect(page).not.toHaveURL(new RegExp(`depart=${addDays(22)}`));
    await expect(page.getByRole('heading', { name: 'Choose your flight' })).toBeVisible();
    expect(problems.errors).toEqual([]);
  });

  test('invalid search shows the app error, not a crash', async ({ page }) => {
    const problems = watch(page);
    await page.goto('/flights?trip=round-trip&from=MNL&to=MNL&depart=2001-01-01&adults=0');
    await expect(page.getByRole('alert').filter({ hasText: 'We couldn’t run this search' })).toBeVisible();
    await expect(page.getByTestId('flight-card')).toHaveCount(0);
    expect(problems.errors).toEqual([]);
  });
});

// ---------------------------------------------------------------------------------------------
test.describe.serial('booking journey (Server Actions, PPR, cookies, retailing API writes)', () => {
  let page: Page;
  let problems: Problems;
  let reference = '';
  let orderUrl = '';

  test.beforeAll(async ({ browser }) => {
    page = await browser.newPage();
    if (process.env.SMOKE_TODAY)
      await page.clock.setFixedTime(new Date(`${process.env.SMOKE_TODAY}T10:00:00+08:00`));
    problems = watch(page);
  });
  test.afterAll(async () => page.close());

  test('select a flight and fare', async () => {
    await page.goto(SEARCH);
    const card = page.getByTestId('flight-card').first();
    await expect(card).toBeVisible();
    await card.getByRole('link', { name: /View fares/ }).click();
    await expect(page.getByTestId('fare-cards')).toBeVisible();
    await page
      .locator('[data-testid="fare-card"][data-family="CLASSIC"]')
      .getByRole('button', { name: /^(Select|Continue with) Classic/ })
      .click();
    await expect(page.getByTestId('trip-summary')).toBeVisible();
    await page.getByRole('button', { name: 'Continue to traveller details' }).click();
    await expect(page).toHaveURL(/\/booking\/travellers$/);
    await expect(page.getByTestId('traveller-form')).toBeVisible();
  });

  test('passenger information', async () => {
    const card = page.getByTestId('traveller-ADT1');
    await card.getByLabel('Given names').fill('Amihan');
    await card.getByLabel('Family name').fill('Dela Cruz');
    await card.getByLabel('Date of birth').fill('1990-04-12');
    await card.getByLabel('Nationality').selectOption('PH');
    await page.getByLabel('Email').fill('amihan.delacruz@example.com');
    await page.getByLabel('Mobile phone').fill('+63 917 555 0123');
    await page.getByTestId('save-travellers').click();
    await expect(page).toHaveURL(/\/booking\/seats/);
  });

  test('seat selection', async () => {
    await expect(page.locator('[data-testid="seat-selector"][data-ready="true"]')).toBeVisible();
    const free = page.locator('[data-testid="seat-map"] [data-look="available"]');
    const n = await free.count();
    expect(n).toBeGreaterThan(0);
    await free.nth(Math.floor(Math.random() * Math.min(n, 30))).click();
    await page.getByTestId('choose-seat').click();
    await expect(page.getByTestId('seat-saved')).toBeVisible();
    await page.getByTestId('continue-seats').click();
    await expect(page).toHaveURL(/\/booking\/extras$/);
  });

  test('extras, then booking review', async () => {
    await expect(page.locator('[data-testid^="baggage-"][data-ready="true"]').first()).toBeVisible();
    await page.getByTestId('continue-extras').click();
    await expect(page).toHaveURL(/\/booking\/review$/);
    await expect(shown(page, 'review-ready')).toBeVisible();
  });

  test('refresh of the review keeps the booking state (server-side session)', async () => {
    const r = await page.reload();
    expect(r!.status()).toBe(200);
    await expect(shown(page, 'review-ready')).toBeVisible();
    await page.waitForLoadState('networkidle');
  });

  test('checkout: the API holds the seats', async () => {
    await page.getByTestId('continue-checkout').click();
    await expect(page).toHaveURL(/\/booking\/checkout$/, { timeout: 30_000 });
    await expect(page.getByTestId('checkout')).toHaveAttribute('data-order-status', 'PENDING_PAYMENT');
    await expect(page.locator('[data-testid="payment-form"][data-ready="true"]')).toBeVisible();
  });

  test('payment simulation and confirmation', async () => {
    const form = page.locator('[data-testid="payment-form"][data-ready="true"]');
    await form.getByRole('radio', { name: /^Demo Card/ }).check();
    // Outcome controls exist only when the API enables them (off in production mode).
    const approve = form.getByRole('radio', { name: /^Approve/ });
    if (await approve.count()) await approve.check();
    await form.getByTestId('pay-button').click();
    await expect(page).toHaveURL(/\/booking\/confirmation\/ord_[A-Za-z0-9_-]{22}$/, { timeout: 30_000 });
    await expect(page.getByRole('heading', { name: 'Booking confirmed' })).toBeVisible();
    reference = (await page.getByTestId('booking-reference').textContent())!.trim();
    expect(reference).toMatch(/^VEL-[A-HJKMNP-Z2-9]{6}$/);
    orderUrl = page.url();
  });

  test('booking management: look up by reference', async () => {
    test.skip(!reference, 'no confirmed booking from the previous step');
    await page.goto('/manage-booking');
    await page.getByRole('textbox', { name: 'Booking reference' }).fill(reference);
    await shown(page, 'lookup-submit').click();
    await expect(page).toHaveURL(/\/manage-booking\/ord_[A-Za-z0-9_-]{22}/, { timeout: 30_000 });
    await expect(page.getByText(reference).first()).toBeVisible();
    const r = await page.reload();
    expect(r!.status()).toBe(200);
  });

  test('confirmation page survives a hard refresh', async () => {
    test.skip(!orderUrl, 'no confirmed booking from the previous step');
    const r = await page.goto(orderUrl);
    expect(r!.status()).toBe(200);
    await expect(page.getByRole('heading', { name: 'Booking confirmed' })).toBeVisible();
  });

  test('no browser errors during the whole journey', async () => {
    expect(problems.errors).toEqual([]);
    expect(problems.rsc.length).toBeGreaterThan(0);
  });
});

// ---------------------------------------------------------------------------------------------
test.describe('Server Actions', () => {
  async function capture(page: Page) {
    await page.goto('/manage-booking');
    const action = page.waitForRequest((r) => r.method() === 'POST' && Boolean(r.headers()['next-action']));
    await page.getByRole('textbox', { name: 'Booking reference' }).fill('VEL-ZZZZZZ');
    await shown(page, 'lookup-submit').click();
    return action;
  }

  test('unknown reference: action runs and shows the app error', async ({ page }) => {
    const problems = watch(page);
    await capture(page);
    await expect(shown(page, 'action-error')).toContainText('couldn’t open a booking');
    expect(problems.errors).toEqual([]);
  });

  test('cross-site action request is refused', async ({ page }) => {
    const captured = await capture(page);
    const headers = { ...captured.headers() };
    delete headers['content-length'];
    const body = captured.postDataBuffer()!;
    const same = await page.request.post(captured.url(), { headers, data: body });
    expect(same.status()).toBe(200);
    const cross = await page.request.post(captured.url(), {
      headers: { ...headers, origin: 'https://evil.example' },
      data: body,
    });
    expect(cross.status()).toBeGreaterThanOrEqual(400);
    expect(await cross.text()).not.toContain('couldn’t open a booking');
  });
});

// ---------------------------------------------------------------------------------------------
test.describe('account flows', () => {
  test('register a demo account, sign out state, sign in', async ({ page }) => {
    const problems = watch(page);
    const email = `smoke-${Date.now()}@example.com`;
    const password = 'smoke-demo-passphrase';
    await page.goto('/account/register');
    const form = page.locator('[data-testid="register-form"][data-ready="true"]');
    await form.getByLabel('Given names').fill('Amihan');
    await form.getByLabel('Family name').fill('Smoketest');
    await form.getByLabel('Email', { exact: true }).fill(email);
    await form.getByLabel('Password', { exact: true }).fill(password);
    await form.getByLabel('Confirm password').fill(password);
    await form.getByTestId('register-demo').check();
    await form.getByTestId('register-submit').click();
    await expect(page).toHaveURL(/\/account$/, { timeout: 30_000 });
    await page.context().clearCookies();
    await page.goto('/account/sign-in');
    const signIn = page.locator('[data-testid="sign-in-form"][data-ready="true"]');
    await signIn.getByLabel('Email', { exact: true }).fill(email);
    await signIn.getByLabel('Password').fill(password);
    await signIn.getByTestId('sign-in-submit').click();
    // Velasia sends a signed-in user to their trips.
    await expect(page).toHaveURL(/\/account\/trips(\?.*)?$/, { timeout: 30_000 });
    // Signed in (the sign-in route can stay mounted but hidden: React Activity).
    await expect(page.getByRole('heading', { level: 1, name: 'My Trips' })).toBeVisible();
    await expect(page.getByText('Hello, Amihan')).toBeVisible();
    await expect(page.getByRole('button', { name: 'Sign out' })).toBeVisible();
    await expect(shown(page, 'sign-in-form')).toHaveCount(0);
    expect(problems.errors).toEqual([]);
  });
});
