/*
 * Cross-user isolation under concurrency (Phase 3B.6), NextRS-owned.
 *
 * N synthetic customers, each in their own browser context, at the same
 * time: register a demo account, book a one-adult trip with a traveller
 * name unique to them, pay (simulated approval), and land on the
 * confirmation. Then every customer probes every other customer's booking
 * (confirmation, Manage Booking, by id), and their own account pages are
 * rendered concurrently, many times: no page may ever contain another
 * customer's traveller name, booking reference or email.
 *
 * Velasia's own helpers are reused read-only from the clone (VELASIA_SRC).
 */
import { join } from 'node:path';
import { test, expect, type Browser, type BrowserContext, type Page } from '@playwright/test';

const src = process.env.VELASIA_SRC!;
// eslint-disable-next-line @typescript-eslint/no-require-imports
const flow = require(join(src, 'tests/e2e/booking-flow'));
// eslint-disable-next-line @typescript-eslint/no-require-imports
const checkout = require(join(src, 'tests/e2e/checkout-flow'));

const USERS = Number(process.env.ISOLATION_USERS ?? 6);
const ROUNDS = Number(process.env.ISOLATION_ROUNDS ?? 5);
const TODAY = '2026-12-01';
const PASSWORD = 'Correct-Horse-9-Battery';
const run = Date.now().toString(36);

type Customer = {
  index: number;
  context: BrowserContext;
  page: Page;
  email: string;
  given: string;
  family: string;
  orderId?: string;
  reference?: string;
};

// Letters only: names must be valid for the booking form.
const LETTERS = 'ABCDEFGHIJKLMNOPQRSTUVWXYZ';
const tag = (i: number) => `${LETTERS[i % 26]}${LETTERS[Math.floor(i / 26) % 26]}`;

async function customer(browser: Browser, index: number): Promise<Customer> {
  const context = await browser.newContext({ timezoneId: 'Asia/Manila', locale: 'en-PH' });
  const page = await context.newPage();
  await page.clock.setFixedTime(new Date(`${TODAY}T10:00:00+08:00`));
  const t = tag(index);
  return {
    index,
    context,
    page,
    email: `iso-${run}-${index}@example.com`,
    given: `Isolde${t}`,
    family: `Probe${t}${run.replace(/\d/g, '').slice(0, 4)}`,
  };
}

async function register(c: Customer) {
  const { page } = c;
  await page.goto('/account/register');
  const form = page.locator('[data-testid="register-form"][data-ready="true"]');
  await form.getByLabel('Given names').fill(c.given);
  await form.getByLabel('Family name').fill(c.family);
  await form.getByLabel('Email', { exact: true }).fill(c.email);
  await form.getByLabel('Password', { exact: true }).fill(PASSWORD);
  await form.getByLabel('Confirm password').fill(PASSWORD);
  await form.getByTestId('register-demo').check();
  await form.getByTestId('register-submit').click();
  await expect(page).toHaveURL(/\/account$/, { timeout: 30_000 });
}

async function book(c: Customer) {
  const { page } = c;
  await flow.startBooking(page, flow.ONE_ADULT, 'Classic');
  const card = page.getByTestId('traveller-ADT1');
  await card.getByLabel('Given names').fill(c.given);
  await card.getByLabel('Family name').fill(c.family);
  await card.getByLabel('Date of birth').fill('1991-05-17');
  await card.getByLabel('Nationality').selectOption('PH');
  await page.getByLabel('Email').fill(c.email);
  await page.getByLabel('Mobile phone').fill('+63 917 555 0199');
  await flow.saveTravellers(page);
  // Distinct seats per customer (checkout holds them for real).
  await flow.chooseSeat(page, flow.seat(page, 'available', 2 * c.index));
  await page.getByTestId('continue-seats').click();
  await expect(page).toHaveURL(/\/booking\/extras$/);
  await flow.extrasReady(page);
  await page.getByTestId('continue-extras').click();
  await expect(page).toHaveURL(/\/booking\/review$/);
  await checkout.toCheckout(page);
  await checkout.pay(page, 'Demo Card', 'Approve');
  c.orderId = await checkout.expectConfirmed(page);
  c.reference = (await page.getByTestId('booking-reference').textContent())!.trim();
  await expect(page.locator('body')).toContainText(c.family);
}

/** Text a page shows, plus its HTML (RSC payloads, hidden markup). */
async function everything(page: Page) {
  return `${await page.locator('body').innerText()}\n${await page.content()}`;
}

// Negative control: treat each customer's own data as another's. The test
// must then fail on the first page it checks (proves the assertions fire).
const NEGATIVE_CONTROL = process.env.ISOLATION_NEGATIVE_CONTROL === '1';

function others(all: Customer[], me: Customer) {
  return NEGATIVE_CONTROL ? all : all.filter((o) => o !== me);
}

function assertNoneOf(text: string, me: Customer, all: Customer[], where: string) {
  for (const o of others(all, me)) {
    for (const secret of [o.family, o.email, o.reference!, o.orderId!]) {
      expect(text.includes(secret), `${where} for customer ${me.index} shows customer ${o.index}'s ${secret}`).toBe(false);
    }
  }
}

test('concurrent customers never see each other’s data', async ({ browser }) => {
  const all = await Promise.all(Array.from({ length: USERS }, (_, i) => customer(browser, i)));
  try {
    // 1. Everyone registers and books at the same time.
    await Promise.all(all.map((c) => register(c)));
    await Promise.all(all.map((c) => book(c)));
    const references = new Set(all.map((c) => c.reference));
    expect(references.size, 'booking references are unique').toBe(USERS);
    expect(new Set(all.map((c) => c.orderId)).size, 'order ids are unique').toBe(USERS);

    // 2. Each customer's own pages show their own booking and nobody else's,
    //    rendered concurrently, several rounds.
    for (let round = 0; round < ROUNDS; round++) {
      await Promise.all(
        all.map(async (c) => {
          const page = c.page;
          for (const path of ['/account', '/account/trips', `/manage-booking/${c.orderId}`, `/booking/confirmation/${c.orderId}`]) {
            const response = await page.goto(path);
            expect(response?.status(), `${path} for customer ${c.index}`).toBeLessThan(400);
            const text = await everything(page);
            if (path !== '/account') expect(text, `${path} shows customer ${c.index}'s own booking`).toContain(c.reference!);
            assertNoneOf(text, c, all, path);
          }
        }),
      );
    }

    // 3. Everyone tries everyone else's booking by id: never shown.
    await Promise.all(
      all.map(async (c) => {
        for (const o of others(all, c)) {
          for (const path of [`/manage-booking/${o.orderId}`, `/booking/confirmation/${o.orderId}`]) {
            await c.page.goto(path);
            const text = await everything(c.page);
            for (const secret of [o.family, o.email, o.reference!]) {
              expect(text.includes(secret), `customer ${c.index} opened ${path} and saw ${secret}`).toBe(false);
            }
          }
        }
      }),
    );

    // 4. A browser with no session (no cookies) sees no booking at all.
    const stranger = await customer(browser, USERS);
    try {
      for (const o of all) {
        for (const path of [`/manage-booking/${o.orderId}`, `/booking/confirmation/${o.orderId}`]) {
          await stranger.page.goto(path);
          const text = await everything(stranger.page);
          for (const secret of [o.family, o.email, o.reference!]) {
            expect(text.includes(secret), `a stranger opened ${path} and saw ${secret}`).toBe(false);
          }
        }
      }
      await stranger.page.goto('/account/trips');
      await expect(stranger.page).toHaveURL(/\/account\/sign-in/);
    } finally {
      await stranger.context.close();
    }
  } finally {
    await Promise.all(all.map((c) => c.context.close()));
  }
});
