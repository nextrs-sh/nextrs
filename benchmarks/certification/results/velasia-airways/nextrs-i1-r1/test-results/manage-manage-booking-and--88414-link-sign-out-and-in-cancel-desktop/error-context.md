# Instructions

- Following Playwright test failed.
- Explain why, be concise, respect Playwright best practices.
- Provide a snippet of code with the fix, if possible.

# Test info

- Name: manage.spec.ts >> manage booking and accounts >> the whole post-booking journey: change, seat, bag, account, link, sign out and in, cancel
- Location: tests/e2e/manage.spec.ts:75:3

# Error details

```
Error: expect(locator).toBeVisible() failed

Locator: getByTestId('manage-updated').visible()
Expected: visible
Timeout: 10000ms
Error: element(s) not found

Call log:
  - Expect "toBeVisible" getByTestId('manage-updated').visible() with timeout 10000ms
  - waiting for getByTestId('manage-updated').visible()

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
  - heading "We hit some turbulence" [level=1]
  - paragraph: Something went wrong while loading this page. Please try again — if it keeps happening, come back a little later.
  - button "Try again"
  - link "Back to home":
    - /url: /
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
- alert: Manage your booking | Velasia Airways
```

# Test source

```ts
  1   | import type { Page } from '@playwright/test';
  2   | import { axe, nextSeatIndex, seat, shown, toReview } from './booking-flow';
  3   | import { API, apiOrder, expectConfirmed, pay, toCheckout } from './checkout-flow';
  4   | import { expect, test, trackPageErrors } from './helpers';
  5   | 
  6   | /*
  7   |  * Milestone 6: Manage Booking, servicing, and demo accounts, end to end against the production
  8   |  * build, a real retailing API, and its own TalaDB file. Every traveller and account is synthetic;
  9   |  * every payment and refund is simulated with the test-only outcome control.
  10  |  */
  11  | 
  12  | const MANAGE_URL = /\/manage-booking\/(ord_[A-Za-z0-9_-]{22})(\?.*)?$/;
  13  | const PASSWORD = 'e2e-demo-passphrase';
  14  | 
  15  | async function confirmedBooking(page: Page) {
  16  |   await toReview(page, 'Classic');
  17  |   await toCheckout(page);
  18  |   await pay(page, 'Demo Card', 'Approve');
  19  |   return expectConfirmed(page);
  20  | }
  21  | 
  22  | async function confirmServicing(
  23  |   page: Page,
  24  |   method?: 'Demo Card' | 'Demo Wallet',
  25  |   outcome = 'Approve',
  26  | ) {
  27  |   const form = page
  28  |     .locator('[data-testid="servicing-confirm"][data-ready="true"]')
  29  |     .filter({ visible: true });
  30  |   await expect(form).toBeVisible();
  31  |   if (method) {
  32  |     await form.getByRole('radio', { name: method }).check();
  33  |     await form.getByRole('radio', { name: outcome, exact: true }).first().check();
  34  |   }
  35  |   await form.getByTestId('confirm-servicing').click();
  36  | }
  37  | 
  38  | async function expectUpdated(page: Page, orderId: string, what: string) {
  39  |   await expect(page).toHaveURL(new RegExp(`/manage-booking/${orderId}\\?updated=${what}$`), {
  40  |     timeout: 20_000,
  41  |   });
> 42  |   await expect(shown(page, 'manage-updated')).toBeVisible();
      |                                               ^ Error: expect(locator).toBeVisible() failed
  43  | }
  44  | 
  45  | async function expectNotice(page: Page, orderId: string, what: string) {
  46  |   await expect(page).toHaveURL(new RegExp(`/manage-booking/${orderId}\\?updated=${what}$`), {
  47  |     timeout: 20_000,
  48  |   });
  49  |   await expect(shown(page, 'manage-notice')).toBeVisible();
  50  | }
  51  | 
  52  | async function register(page: Page, email: string) {
  53  |   await page.goto('/account/register');
  54  |   const form = page.locator('[data-testid="register-form"][data-ready="true"]');
  55  |   await form.getByLabel('Given names').fill('Amihan');
  56  |   await form.getByLabel('Family name').fill('Testcase');
  57  |   await form.getByLabel('Email', { exact: true }).fill(email);
  58  |   await form.getByLabel('Password', { exact: true }).fill(PASSWORD);
  59  |   await form.getByLabel('Confirm password').fill(PASSWORD);
  60  |   await form.getByTestId('register-demo').check();
  61  |   await form.getByTestId('register-submit').click();
  62  |   await expect(page).toHaveURL(/\/account$/, { timeout: 20_000 });
  63  | }
  64  | 
  65  | async function signIn(page: Page, email: string) {
  66  |   const form = page.locator('[data-testid="sign-in-form"][data-ready="true"]');
  67  |   await form.getByLabel('Email', { exact: true }).fill(email);
  68  |   await form.getByLabel('Password').fill(PASSWORD);
  69  |   await form.getByTestId('sign-in-submit').click();
  70  | }
  71  | 
  72  | test.describe('manage booking and accounts', () => {
  73  |   test.describe.configure({ timeout: 180_000 });
  74  | 
  75  |   test('the whole post-booking journey: change, seat, bag, account, link, sign out and in, cancel', async ({
  76  |     page,
  77  |     request,
  78  |   }) => {
  79  |     const errors = trackPageErrors(page);
  80  |     const orderId = await confirmedBooking(page);
  81  | 
  82  |     // Manage Booking finds the booking this browser made.
  83  |     await page.goto('/manage-booking');
  84  |     const card = page.locator(`[data-testid="trip-card"][data-order="${orderId}"]`);
  85  |     await expect(card).toBeVisible();
  86  |     await card.getByRole('link', { name: /Manage booking/ }).click();
  87  |     await expect(page).toHaveURL(MANAGE_URL);
  88  |     await expect(shown(page, 'manage-booking')).toHaveAttribute('data-order-status', 'CONFIRMED');
  89  |     expect(await axe(page)).toEqual([]);
  90  | 
  91  |     // 7–10. Change to the afternoon flight; the price is the API's; pay; the itinerary updates.
  92  |     await shown(page, 'action-change-outbound').click();
  93  |     await expect(shown(page, 'change-alternatives')).toBeVisible();
  94  |     const alternative = page.locator(
  95  |       '[data-testid="change-alternative"][data-itinerary="NV503-20261215"]',
  96  |     );
  97  |     await alternative.getByRole('link', { name: /Review/ }).click();
  98  |     const review = shown(page, 'change-review');
  99  |     await expect(review).toBeVisible();
  100 |     await expect(review.getByTestId('current-journey')).toContainText('VEL 501');
  101 |     await expect(review.getByTestId('new-journey')).toContainText('VEL 503');
  102 |     await expect(review.getByTestId('servicing-quote')).toHaveAttribute(
  103 |       'data-adjustment',
  104 |       'CHARGE',
  105 |     );
  106 |     await expect(review.getByTestId('quote-line-CHANGE_FEE')).toContainText('₱1,200');
  107 |     await confirmServicing(page, 'Demo Wallet');
  108 |     await expectUpdated(page, orderId, 'flight');
  109 |     await expect(shown(page, 'order-leg-outbound')).toContainText('VEL 503');
  110 | 
  111 |     // 11. A seat on the new flight (the old flight's seat was released).
  112 |     await shown(page, 'action-seats').click();
  113 |     await expect(page.locator('[data-testid="seat-selector"][data-ready="true"]')).toBeVisible();
  114 |     await seat(page, 'available', nextSeatIndex()).click();
  115 |     await shown(page, 'choose-seat').click();
  116 |     const seatQuote = shown(page, 'seat-change-panel').getByTestId('servicing-quote');
  117 |     await expect(seatQuote).toBeVisible();
  118 |     const seatAdjustment = await seatQuote.getAttribute('data-adjustment');
  119 |     await confirmServicing(page, seatAdjustment === 'CHARGE' ? 'Demo Card' : undefined);
  120 |     await expectUpdated(page, orderId, 'seats');
  121 | 
  122 |     // 12. Extra baggage after booking.
  123 |     await shown(page, 'action-extras').click();
  124 |     await shown(page, 'extra-BAG_10-ADT1-outbound').click();
  125 |     await expect(shown(page, 'extras-panel').getByTestId('servicing-quote')).toHaveAttribute(
  126 |       'data-adjustment',
  127 |       'CHARGE',
  128 |     );
  129 |     await confirmServicing(page, 'Demo Card');
  130 |     await expectUpdated(page, orderId, 'extras');
  131 | 
  132 |     // 13. The total and the history reflect three completed changes; the original is kept.
  133 |     await expect(
  134 |       page.locator('[data-testid="servicing-record"][data-status="COMPLETED"]'),
  135 |     ).toHaveCount(3);
  136 |     const order = await apiOrder(page, request, orderId);
  137 |     expect(order.currentVersion).toBe(4);
  138 |     expect(order.original.journeys[0].itinerary.id).toBe('NV501-20261215');
  139 |     expect(order.currentTotalMinor).toBeGreaterThan(order.totalMinor);
  140 |     expect(order.financial.paidMinor).toBe(order.currentTotalMinor);
  141 | 
  142 |     // 14–16. A demo account; the guest booking is linked only on explicit confirmation.
```