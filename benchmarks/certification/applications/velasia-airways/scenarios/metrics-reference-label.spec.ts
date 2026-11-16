/*
 * Phase 3B: does a booking-reference lookup put the reference into the
 * metrics' `operation` label? Deterministic version of the condition behind
 * production.spec.ts's "metrics never contain VEL-" failures. NextRS-owned.
 */
import { join } from 'node:path';
import { writeFileSync } from 'node:fs';
import { test, expect } from '@playwright/test';

// eslint-disable-next-line @typescript-eslint/no-require-imports
const flow = require(join(process.env.VELASIA_SRC!, 'tests/e2e/booking-flow'));
// eslint-disable-next-line @typescript-eslint/no-require-imports
const checkout = require(join(process.env.VELASIA_SRC!, 'tests/e2e/checkout-flow'));

test('a reference lookup and the metrics labels', async ({ page, request }) => {
  test.setTimeout(180_000);
  await page.clock.setFixedTime(new Date('2026-12-01T10:00:00+08:00'));
  // The owner's lookup of their own booking (the API is only asked for a
  // reference this browser can open).
  await flow.toReview(page, 'Classic');
  await checkout.toCheckout(page);
  await checkout.pay(page, 'Demo Card', 'Approve');
  await checkout.expectConfirmed(page);
  const reference = (await page.getByTestId('booking-reference').textContent())!.trim();
  console.log('reference', reference);
  await page.goto('/manage-booking');
  await page.getByRole('textbox', { name: 'Booking reference' }).fill(reference);
  await flow.shown(page, 'lookup-submit').click();
  await expect(page).toHaveURL(/\/manage-booking\/ord_/, { timeout: 20_000 });
  await page.goto('/manage-booking');
  await page.getByRole('textbox', { name: 'Booking reference' }).fill('VEL-ZZZZZZ');
  await flow.shown(page, 'lookup-submit').click();
  await expect(flow.shown(page, 'action-error')).toContainText('couldn’t open a booking');
  const metrics = await (await request.get('/api/internal/metrics')).text();
  const lines = metrics.split('\n').filter((l) => l.includes('reference') && !l.startsWith('#'));
  writeFileSync(process.env.LABEL_OUT ?? 'labels.txt', lines.join('\n') + '\n');
  console.log(lines.slice(0, 5).join('\n') || '(no reference series)');
});
