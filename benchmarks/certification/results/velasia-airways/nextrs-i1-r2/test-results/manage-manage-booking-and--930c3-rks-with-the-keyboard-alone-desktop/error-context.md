# Instructions

- Following Playwright test failed.
- Explain why, be concise, respect Playwright best practices.
- Provide a snippet of code with the fix, if possible.

# Test info

- Name: manage.spec.ts >> manage booking and accounts >> Manage Booking works with the keyboard alone
- Location: tests/e2e/manage.spec.ts:266:3

# Error details

```
Error: expect(locator).toBeVisible() failed

Locator: locator('[data-testid="seat-selector"][data-ready="true"]')
Expected: visible
Timeout: 10000ms
Error: element(s) not found

Call log:
  - Expect "toBeVisible" locator('[data-testid="seat-selector"][data-ready="true"]') with timeout 10000ms
  - waiting for locator('[data-testid="seat-selector"][data-ready="true"]')

```

```yaml
- link "Skip to content":
  - /url: "#main"
- paragraph:
  - strong: Demo site.
  - text: Velasia Airways is a fictional airline — no real flights, bookings, or payments.
- banner:
  - link "Velasia Airways home":
    - /url: /
  - navigation "Primary":
    - list:
      - listitem:
        - link "Book flights":
          - /url: /flights
      - listitem:
        - link "Explore destinations":
          - /url: /destinations
      - listitem:
        - link "Manage booking":
          - /url: /manage-booking
      - listitem:
        - link "Help":
          - /url: /help
  - paragraph: English PHP
  - link "Account":
    - /url: /account
- main:
  - alert:
    - heading "We couldn’t load this step" [level=1]
    - paragraph: Something went wrong while loading your booking. Your saved choices are kept in your session, and nothing has been booked or charged.
    - button "Try again"
    - link "New search":
      - /url: /flights
- contentinfo:
  - link "Velasia Airways home":
    - /url: /
  - paragraph: Beyond every horizon.
  - paragraph: A premium full-service airline for the Philippines, Southeast Asia, and beyond, flying from our hubs in Manila and Cebu — in a fictional network built to explore modern airline retailing.
  - navigation "Book":
    - heading "Book" [level=2]
    - list:
      - listitem:
        - link "Search flights":
          - /url: /flights
      - listitem:
        - link "Destinations":
          - /url: /destinations
      - listitem:
        - link "Manage booking":
          - /url: /manage-booking
      - listitem:
        - link "My Trips":
          - /url: /account/trips
  - navigation "Help":
    - heading "Help" [level=2]
    - list:
      - listitem:
        - link "Help centre":
          - /url: /help
      - listitem:
        - link "Baggage":
          - /url: /help#baggage
      - listitem:
        - link "Travel documents":
          - /url: /help#documents
      - listitem:
        - link "Contact us":
          - /url: /help#contact
  - navigation "Velasia":
    - heading "Velasia" [level=2]
    - list:
      - listitem:
        - link "About this demo":
          - /url: /about
      - listitem:
        - link "Photo credits":
          - /url: /credits
  - paragraph: "© 2026 Velasia Airways. A fictional airline: no real reservations, tickets, payments, or financial transactions. All fares shown are illustrative."
  - paragraph: Velasia Airways is a fictional airline created for demonstration. No real flights, bookings, or payments are offered.
- alert: We couldn’t load this step
```

# Test source

```ts
  1   | import AxeBuilder from '@axe-core/playwright';
  2   | import { test, type Locator, type Page } from '@playwright/test';
  3   | import { expect, selectFamily } from './helpers';
  4   | 
  5   | /*
  6   |  * The booking journey up to the review (Milestones 3–4), shared by the booking and checkout
  7   |  * specs. All travellers are synthetic.
  8   |  */
  9   | 
  10  | export const TWO_ADULTS =
  11  |   '/flights?trip=one-way&from=MNL&to=SIN&depart=2026-12-15&adults=2&cabin=economy';
  12  | export const ONE_ADULT =
  13  |   '/flights?trip=one-way&from=MNL&to=SIN&depart=2026-12-15&adults=1&cabin=economy';
  14  | export const FLIGHT = 'NV501-20261215';
  15  | 
  16  | /**
  17  |  * Next.js keeps recently visited routes mounted but hidden (React Activity), so a test id can
  18  |  * exist on an earlier step too: always look at the visible one.
  19  |  */
  20  | export const shown = (page: Page, testId: string) =>
  21  |   page.getByTestId(testId).filter({ visible: true });
  22  | 
  23  | export const amount = async (locator: Locator) =>
  24  |   Number((await locator.first().textContent())!.replace(/[^\d]/g, ''));
  25  | 
  26  | export async function axe(page: Page) {
  27  |   const { violations } = await new AxeBuilder({ page })
  28  |     .withTags(['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa'])
  29  |     .analyze();
  30  |   return violations
  31  |     .filter((v) => v.impact === 'critical' || v.impact === 'serious')
  32  |     .map((v) => `${v.id}: ${v.nodes.map((node) => node.target.join(' ')).join(' | ')}`);
  33  | }
  34  | 
  35  | export const TRAVELLERS = [
  36  |   { id: 'ADT1', given: 'Amihan', family: 'Dela Cruz', dob: '1990-04-12' },
  37  |   { id: 'ADT2', given: 'Bayani', family: 'Reyes', dob: '1987-09-30' },
  38  | ];
  39  | 
  40  | /** Search, open the fares of the first result, choose a family, and continue to travellers. */
  41  | export async function startBooking(
  42  |   page: Page,
  43  |   search: string,
  44  |   family: 'Light' | 'Classic' | 'Flex',
  45  | ) {
  46  |   await page.goto(`${search}&fares=${FLIGHT}`);
  47  |   await expect(page.getByTestId('fare-cards')).toBeVisible();
  48  |   await selectFamily(page, family);
  49  |   await expect(page.getByTestId('trip-summary')).toBeVisible();
  50  |   await page.getByRole('button', { name: 'Continue to traveller details' }).click();
  51  |   await expect(page).toHaveURL(/\/booking\/travellers$/);
  52  |   await expect(page.getByTestId('traveller-form')).toBeVisible();
  53  | }
  54  | 
  55  | export async function fillTravellers(page: Page, count: number) {
  56  |   for (const traveller of TRAVELLERS.slice(0, count)) {
  57  |     const card = page.getByTestId(`traveller-${traveller.id}`);
  58  |     await card.getByLabel('Given names').fill(traveller.given);
  59  |     await card.getByLabel('Family name').fill(traveller.family);
  60  |     await card.getByLabel('Date of birth').fill(traveller.dob);
  61  |     await card.getByLabel('Nationality').selectOption('PH');
  62  |   }
  63  |   await page.getByLabel('Email').fill('amihan.delacruz@example.com');
  64  |   await page.getByLabel('Mobile phone').fill('+63 917 555 0123');
  65  | }
  66  | 
  67  | export async function saveTravellers(page: Page) {
  68  |   await page.getByTestId('save-travellers').click();
  69  |   await expect(page).toHaveURL(/\/booking\/seats/);
> 70  |   await expect(page.locator('[data-testid="seat-selector"][data-ready="true"]')).toBeVisible();
      |                                                                                  ^ Error: expect(locator).toBeVisible() failed
  71  | }
  72  | 
  73  | export const seat = (page: Page, look: string, nth = 0) =>
  74  |   page.locator(`[data-testid="seat-map"] [data-look="${look}"]`).nth(nth);
  75  | 
  76  | /** Choose the seat under the cursor for the active traveller; returns the seat id. */
  77  | export async function chooseSeat(page: Page, target: Locator) {
  78  |   const id = (await target.getAttribute('data-seat'))!;
  79  |   await target.click();
  80  |   await page.getByTestId('choose-seat').click();
  81  |   await expect(page.getByTestId('seat-saved')).toBeVisible();
  82  |   return id;
  83  | }
  84  | 
  85  | export async function extrasReady(page: Page) {
  86  |   await expect(page.locator('[data-testid^="baggage-"][data-ready="true"]').first()).toBeVisible();
  87  | }
  88  | 
  89  | /** Wait until the review's client actions are interactive. */
  90  | export async function reviewReady(page: Page) {
  91  |   await expect(shown(page, 'review-ready')).toBeVisible();
  92  |   await page.waitForLoadState('networkidle');
  93  | }
  94  | 
  95  | /** Travellers saved, seats skipped, extras skipped: the review for a one-adult trip. */
  96  | let seatsChosen = 0;
  97  | /**
  98  |  * Which available seat a booking picks. Since Milestone 5, checkout really holds seats, so tests
  99  |  * running at the same time on the same flight must not pick the same one: each parallel worker
  100 |  * gets its own band of five positions in the list of free seats (40 of the flight's ~74).
  101 |  */
  102 | export function nextSeatIndex(): number {
  103 |   seatsChosen += 1;
  104 |   return (test.info().parallelIndex % 8) * 5 + (seatsChosen % 5);
  105 | }
  106 | 
  107 | export async function toReview(page: Page, family: 'Light' | 'Classic' | 'Flex' = 'Classic') {
  108 |   await startBooking(page, ONE_ADULT, family);
  109 |   await fillTravellers(page, 1);
  110 |   await saveTravellers(page);
  111 |   const id = await chooseSeat(page, seat(page, 'available', nextSeatIndex()));
  112 |   await page.getByTestId('continue-seats').click();
  113 |   await expect(page).toHaveURL(/\/booking\/extras$/);
  114 |   await extrasReady(page);
  115 |   await page.getByTestId('continue-extras').click();
  116 |   await expect(page).toHaveURL(/\/booking\/review$/);
  117 |   await expect(page.getByTestId('review-ready')).toBeVisible();
  118 |   return id;
  119 | }
  120 | 
```