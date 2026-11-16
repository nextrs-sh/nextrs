/*
 * Phase 3B.7 preparation (NextRS-owned): creates the state the Velasia
 * workloads read, through the real user interface, and writes it to
 * PERF_STATE (JSON):
 *
 *  - `booking`: cookies of a browser at the traveller step (shopping session);
 *  - `account`: cookies of a signed-in demo account that has one confirmed
 *    booking (`orderId`), for My Trips and Manage Booking;
 *  - `action`: one real Server Action request (selecting a fare family, which
 *    re-selects the same offer: idempotent), captured as sent, for replay.
 *
 * Run once per benchmark session against any runtime serving the build; the
 * API (and its TalaDB file) must stay up for the whole benchmark.
 */
import { writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { test, expect, type Page } from '@playwright/test';

const src = process.env.VELASIA_SRC!;
// eslint-disable-next-line @typescript-eslint/no-require-imports
const flow = require(join(src, 'tests/e2e/booking-flow'));
// eslint-disable-next-line @typescript-eslint/no-require-imports
const checkout = require(join(src, 'tests/e2e/checkout-flow'));
// eslint-disable-next-line @typescript-eslint/no-require-imports
const helpers = require(join(src, 'tests/e2e/helpers'));

const TODAY = '2026-12-01';
const cookieHeader = async (page: Page) =>
  (await page.context().cookies()).map((c) => `${c.name}=${c.value}`).join('; ');

test('prepare benchmark state', async ({ browser }) => {
  test.setTimeout(300_000);
  const state: Record<string, unknown> = {};

  // A browser at the traveller step; capture the fare-selection action on the way.
  {
    const context = await browser.newContext({ timezoneId: 'Asia/Manila', locale: 'en-PH' });
    const page = await context.newPage();
    await page.clock.setFixedTime(new Date(`${TODAY}T10:00:00+08:00`));
    let captured: { path: string; headers: Record<string, string>; body: string } | null = null;
    page.on('request', (request) => {
      const headers = request.headers();
      if (request.method() === 'POST' && headers['next-action'] && !captured) {
        const url = new URL(request.url());
        captured = { path: url.pathname + url.search, headers, body: request.postData() ?? '' };
      }
    });
    await page.goto(`${flow.ONE_ADULT}&fares=${flow.FLIGHT}`);
    await expect(page.getByTestId('fare-cards')).toBeVisible();
    await helpers.selectFamily(page, 'Classic');
    await expect(page.getByTestId('trip-summary')).toBeVisible();
    expect(captured, 'a Server Action request was captured').not.toBeNull();
    const cookies = await cookieHeader(page);
    // Replay as the browser sent it, with this browser's cookies.
    const keep = ['next-action', 'next-router-state-tree', 'content-type', 'accept', 'origin', 'next-url'];
    state.action = {
      path: captured!.path,
      headers: Object.fromEntries(
        Object.entries(captured!.headers).filter(([name]) => keep.includes(name)),
      ),
      cookies,
      body: captured!.body,
    };
    await page.getByRole('button', { name: 'Continue to traveller details' }).click();
    await expect(page).toHaveURL(/\/booking\/travellers$/);
    state.booking = { cookies: await cookieHeader(page) };
    await context.close();
  }

  // A signed-in account with one confirmed booking.
  {
    const context = await browser.newContext({ timezoneId: 'Asia/Manila', locale: 'en-PH' });
    const page = await context.newPage();
    await page.clock.setFixedTime(new Date(`${TODAY}T10:00:00+08:00`));
    const email = `perf-${Date.now().toString(36)}@example.com`;
    await page.goto('/account/register');
    const form = page.locator('[data-testid="register-form"][data-ready="true"]');
    await form.getByLabel('Given names').fill('Perf');
    await form.getByLabel('Family name').fill('Bench');
    await form.getByLabel('Email', { exact: true }).fill(email);
    await form.getByLabel('Password', { exact: true }).fill('Correct-Horse-9-Battery');
    await form.getByLabel('Confirm password').fill('Correct-Horse-9-Battery');
    await form.getByTestId('register-demo').check();
    await form.getByTestId('register-submit').click();
    await expect(page).toHaveURL(/\/account$/, { timeout: 30_000 });
    await flow.toReview(page, 'Classic');
    await checkout.toCheckout(page);
    await checkout.pay(page, 'Demo Card', 'Approve');
    const orderId = await checkout.expectConfirmed(page);
    const reference = (await page.getByTestId('booking-reference').textContent())!.trim();
    await page.goto('/account/trips');
    await expect(page.locator('body')).toContainText(reference);
    state.account = { cookies: await cookieHeader(page), orderId, reference, email };
    await context.close();
  }

  writeFileSync(process.env.PERF_STATE!, JSON.stringify(state, null, 2));
});
