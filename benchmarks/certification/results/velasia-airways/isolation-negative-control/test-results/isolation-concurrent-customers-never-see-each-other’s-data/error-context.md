# Instructions

- Following Playwright test failed.
- Explain why, be concise, respect Playwright best practices.
- Provide a snippet of code with the fix, if possible.

# Test info

- Name: isolation.spec.ts >> concurrent customers never see each other’s data
- Location: ../../../Documents/OpenSource/nextrs/benchmarks/certification/applications/velasia-airways/scenarios/isolation.spec.ts:119:1

# Error details

```
Error: /account/trips for customer 0 shows customer 0's ProbeAAmwjz

expect(received).toBe(expected) // Object.is equality

Expected: false
Received: true
```

# Test source

```ts
  14  | import { join } from 'node:path';
  15  | import { test, expect, type Browser, type BrowserContext, type Page } from '@playwright/test';
  16  | 
  17  | const src = process.env.VELASIA_SRC!;
  18  | // eslint-disable-next-line @typescript-eslint/no-require-imports
  19  | const flow = require(join(src, 'tests/e2e/booking-flow'));
  20  | // eslint-disable-next-line @typescript-eslint/no-require-imports
  21  | const checkout = require(join(src, 'tests/e2e/checkout-flow'));
  22  | 
  23  | const USERS = Number(process.env.ISOLATION_USERS ?? 6);
  24  | const ROUNDS = Number(process.env.ISOLATION_ROUNDS ?? 5);
  25  | const TODAY = '2026-12-01';
  26  | const PASSWORD = 'Correct-Horse-9-Battery';
  27  | const run = Date.now().toString(36);
  28  | 
  29  | type Customer = {
  30  |   index: number;
  31  |   context: BrowserContext;
  32  |   page: Page;
  33  |   email: string;
  34  |   given: string;
  35  |   family: string;
  36  |   orderId?: string;
  37  |   reference?: string;
  38  | };
  39  | 
  40  | // Letters only: names must be valid for the booking form.
  41  | const LETTERS = 'ABCDEFGHIJKLMNOPQRSTUVWXYZ';
  42  | const tag = (i: number) => `${LETTERS[i % 26]}${LETTERS[Math.floor(i / 26) % 26]}`;
  43  | 
  44  | async function customer(browser: Browser, index: number): Promise<Customer> {
  45  |   const context = await browser.newContext({ timezoneId: 'Asia/Manila', locale: 'en-PH' });
  46  |   const page = await context.newPage();
  47  |   await page.clock.setFixedTime(new Date(`${TODAY}T10:00:00+08:00`));
  48  |   const t = tag(index);
  49  |   return {
  50  |     index,
  51  |     context,
  52  |     page,
  53  |     email: `iso-${run}-${index}@example.com`,
  54  |     given: `Isolde${t}`,
  55  |     family: `Probe${t}${run.replace(/\d/g, '').slice(0, 4)}`,
  56  |   };
  57  | }
  58  | 
  59  | async function register(c: Customer) {
  60  |   const { page } = c;
  61  |   await page.goto('/account/register');
  62  |   const form = page.locator('[data-testid="register-form"][data-ready="true"]');
  63  |   await form.getByLabel('Given names').fill(c.given);
  64  |   await form.getByLabel('Family name').fill(c.family);
  65  |   await form.getByLabel('Email', { exact: true }).fill(c.email);
  66  |   await form.getByLabel('Password', { exact: true }).fill(PASSWORD);
  67  |   await form.getByLabel('Confirm password').fill(PASSWORD);
  68  |   await form.getByTestId('register-demo').check();
  69  |   await form.getByTestId('register-submit').click();
  70  |   await expect(page).toHaveURL(/\/account$/, { timeout: 30_000 });
  71  | }
  72  | 
  73  | async function book(c: Customer) {
  74  |   const { page } = c;
  75  |   await flow.startBooking(page, flow.ONE_ADULT, 'Classic');
  76  |   const card = page.getByTestId('traveller-ADT1');
  77  |   await card.getByLabel('Given names').fill(c.given);
  78  |   await card.getByLabel('Family name').fill(c.family);
  79  |   await card.getByLabel('Date of birth').fill('1991-05-17');
  80  |   await card.getByLabel('Nationality').selectOption('PH');
  81  |   await page.getByLabel('Email').fill(c.email);
  82  |   await page.getByLabel('Mobile phone').fill('+63 917 555 0199');
  83  |   await flow.saveTravellers(page);
  84  |   // Distinct seats per customer (checkout holds them for real).
  85  |   await flow.chooseSeat(page, flow.seat(page, 'available', 2 * c.index));
  86  |   await page.getByTestId('continue-seats').click();
  87  |   await expect(page).toHaveURL(/\/booking\/extras$/);
  88  |   await flow.extrasReady(page);
  89  |   await page.getByTestId('continue-extras').click();
  90  |   await expect(page).toHaveURL(/\/booking\/review$/);
  91  |   await checkout.toCheckout(page);
  92  |   await checkout.pay(page, 'Demo Card', 'Approve');
  93  |   c.orderId = await checkout.expectConfirmed(page);
  94  |   c.reference = (await page.getByTestId('booking-reference').textContent())!.trim();
  95  |   await expect(page.locator('body')).toContainText(c.family);
  96  | }
  97  | 
  98  | /** Text a page shows, plus its HTML (RSC payloads, hidden markup). */
  99  | async function everything(page: Page) {
  100 |   return `${await page.locator('body').innerText()}\n${await page.content()}`;
  101 | }
  102 | 
  103 | // Negative control: treat each customer's own data as another's. The test
  104 | // must then fail on the first page it checks (proves the assertions fire).
  105 | const NEGATIVE_CONTROL = process.env.ISOLATION_NEGATIVE_CONTROL === '1';
  106 | 
  107 | function others(all: Customer[], me: Customer) {
  108 |   return NEGATIVE_CONTROL ? all : all.filter((o) => o !== me);
  109 | }
  110 | 
  111 | function assertNoneOf(text: string, me: Customer, all: Customer[], where: string) {
  112 |   for (const o of others(all, me)) {
  113 |     for (const secret of [o.family, o.email, o.reference!, o.orderId!]) {
> 114 |       expect(text.includes(secret), `${where} for customer ${me.index} shows customer ${o.index}'s ${secret}`).toBe(false);
      |                                                                                                                ^ Error: /account/trips for customer 0 shows customer 0's ProbeAAmwjz
  115 |     }
  116 |   }
  117 | }
  118 | 
  119 | test('concurrent customers never see each other’s data', async ({ browser }) => {
  120 |   const all = await Promise.all(Array.from({ length: USERS }, (_, i) => customer(browser, i)));
  121 |   try {
  122 |     // 1. Everyone registers and books at the same time.
  123 |     await Promise.all(all.map((c) => register(c)));
  124 |     await Promise.all(all.map((c) => book(c)));
  125 |     const references = new Set(all.map((c) => c.reference));
  126 |     expect(references.size, 'booking references are unique').toBe(USERS);
  127 |     expect(new Set(all.map((c) => c.orderId)).size, 'order ids are unique').toBe(USERS);
  128 | 
  129 |     // 2. Each customer's own pages show their own booking and nobody else's,
  130 |     //    rendered concurrently, several rounds.
  131 |     for (let round = 0; round < ROUNDS; round++) {
  132 |       await Promise.all(
  133 |         all.map(async (c) => {
  134 |           const page = c.page;
  135 |           for (const path of ['/account', '/account/trips', `/manage-booking/${c.orderId}`, `/booking/confirmation/${c.orderId}`]) {
  136 |             const response = await page.goto(path);
  137 |             expect(response?.status(), `${path} for customer ${c.index}`).toBeLessThan(400);
  138 |             const text = await everything(page);
  139 |             if (path !== '/account') expect(text, `${path} shows customer ${c.index}'s own booking`).toContain(c.reference!);
  140 |             assertNoneOf(text, c, all, path);
  141 |           }
  142 |         }),
  143 |       );
  144 |     }
  145 | 
  146 |     // 3. Everyone tries everyone else's booking by id: never shown.
  147 |     await Promise.all(
  148 |       all.map(async (c) => {
  149 |         for (const o of others(all, c)) {
  150 |           for (const path of [`/manage-booking/${o.orderId}`, `/booking/confirmation/${o.orderId}`]) {
  151 |             await c.page.goto(path);
  152 |             const text = await everything(c.page);
  153 |             for (const secret of [o.family, o.email, o.reference!]) {
  154 |               expect(text.includes(secret), `customer ${c.index} opened ${path} and saw ${secret}`).toBe(false);
  155 |             }
  156 |           }
  157 |         }
  158 |       }),
  159 |     );
  160 | 
  161 |     // 4. A browser with no session (no cookies) sees no booking at all.
  162 |     const stranger = await customer(browser, USERS);
  163 |     try {
  164 |       for (const o of all) {
  165 |         for (const path of [`/manage-booking/${o.orderId}`, `/booking/confirmation/${o.orderId}`]) {
  166 |           await stranger.page.goto(path);
  167 |           const text = await everything(stranger.page);
  168 |           for (const secret of [o.family, o.email, o.reference!]) {
  169 |             expect(text.includes(secret), `a stranger opened ${path} and saw ${secret}`).toBe(false);
  170 |           }
  171 |         }
  172 |       }
  173 |       await stranger.page.goto('/account/trips');
  174 |       await expect(stranger.page).toHaveURL(/\/account\/sign-in/);
  175 |     } finally {
  176 |       await stranger.context.close();
  177 |     }
  178 |   } finally {
  179 |     await Promise.all(all.map((c) => c.context.close()));
  180 |   }
  181 | });
  182 | 
```