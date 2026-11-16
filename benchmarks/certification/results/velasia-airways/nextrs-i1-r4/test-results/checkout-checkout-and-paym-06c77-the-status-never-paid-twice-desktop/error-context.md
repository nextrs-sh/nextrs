# Instructions

- Following Playwright test failed.
- Explain why, be concise, respect Playwright best practices.
- Provide a snippet of code with the fix, if possible.

# Test info

- Name: checkout.spec.ts >> checkout and payment >> a timed-out payment is recovered by checking the status, never paid twice
- Location: tests/e2e/checkout.spec.ts:138:3

# Error details

```
Error: expect(page).toHaveURL(expected) failed

Expected pattern: /\/booking\/checkout$/
Received string:  "http://127.0.0.1:3200/booking/review"
Timeout: 20000ms

Call log:
  - Expect "toHaveURL" with timeout 20000ms
    43 × locator resolved to <html lang="en" class="mOU60a_variable qqtqVG_variable">…</html>
       - unexpected value "http://127.0.0.1:3200/booking/review"

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
  - paragraph: Book your trip · Step 4 of 4
  - heading "Review your booking" [level=1]
  - paragraph: Check every detail. We confirm prices, seats, and extras again before checkout.
  - navigation "Booking progress":
    - list:
      - listitem:
        - link "Travellers, done":
          - /url: /booking/travellers
      - listitem:
        - link "Seats":
          - /url: /booking/seats
      - listitem:
        - link "Extras, done":
          - /url: /booking/extras
      - listitem: Review, current step
  - alert "A few things need your attention":
    - heading "A few things need your attention" [level=2]
    - list:
      - listitem:
        - paragraph: Seat 12C on VEL 501 is no longer available for Adult 1. Please choose another seat.
        - link "Choose seats":
          - /url: /booking/seats?flight=NV501-20261215
    - button "Check prices and availability"
  - region "Flights":
    - heading "Flights" [level=2]
    - link "Change flights":
      - /url: /flights?trip=one-way&from=MNL&to=SIN&depart=2026-12-15&adults=1&cabin=economy&out=NV501-20261215
    - article:
      - heading "Departing · Tue, Dec 15, 2026" [level=3]
      - text: Velasia Classic
      - paragraph: 08:15
      - paragraph: MNL , Manila
      - paragraph: Total journey 3 hours 45 minutes
      - paragraph: Direct
      - paragraph: 12:00
      - paragraph: SIN , Singapore
      - list:
        - listitem: VEL 501 · MNL 08:15 → SIN 12:00 · 3h 45m · Airbus A321neo · Economy
      - paragraph: "Fare includes: cabin bag 1 × 7 kg · checked bag 1 × 20 kg · seat: included · snack and drink"
  - region "Travellers":
    - heading "Travellers" [level=2]
    - link "Edit":
      - /url: /booking/travellers
    - list:
      - listitem:
        - paragraph: Amihan Dela Cruz
        - paragraph: Adult 1 · Adult · born Thu, Apr 12, 1990 · Philippines
    - paragraph: "Contact: amihan.delacruz@example.com · +639175550123"
  - region "Seats":
    - heading "Seats" [level=2]
    - link "Change seats":
      - /url: /booking/seats
    - table "Seats by traveller and flight":
      - caption: Seats by traveller and flight
      - rowgroup:
        - row "Traveller Flight Seat Price":
          - columnheader "Traveller"
          - columnheader "Flight"
          - columnheader "Seat"
          - columnheader "Price"
      - rowgroup:
        - row "Amihan Dela Cruz VEL 501 · MNL → SIN Assigned at check-in No charge":
          - cell "Amihan Dela Cruz"
          - cell "VEL 501 · MNL → SIN"
          - cell "Assigned at check-in"
          - cell "No charge"
    - paragraph: "Seat choices are provisional: they aren’t reserved until checkout, where we confirm them again."
  - region "Baggage and extras":
    - heading "Baggage and extras" [level=2]
    - link "Change extras":
      - /url: /booking/extras
    - list:
      - listitem: "Departing: 1 × 20 kg checked baggage included per traveller."
    - paragraph: No extras added.
  - region "Price summary":
    - heading "Price summary" [level=2]
    - term: Base fares
    - definition: ₱6,140
    - term: Fare benefits
    - definition: ₱1,190
    - term: Taxes and mandatory fees
    - definition: ₱1,400
    - term: Seats
    - definition: ₱0
    - term: Extra baggage
    - definition: ₱0
    - term: Meals
    - definition: ₱0
    - term: Priority services
    - definition: ₱0
    - term: Pre-checkout total PHP, all travellers. Not yet booked or paid.
    - definition: ₱8,730
    - paragraph: Included fare benefits are listed at no charge and never added to the total.
  - link "Extras":
    - /url: /booking/extras
  - paragraph: Checkout opens once your booking has been checked and nothing needs attention.
  - complementary "MNL → SIN":
    - paragraph: Your trip
    - heading "MNL → SIN" [level=2]
    - list:
      - listitem: Departing · MNL → SIN Tue, Dec 15, 2026 · 08:15 VEL 501 · Velasia Classic ₱8,730
    - term: Flights, taxes and fees
    - definition: ₱8,730
    - term: Seats
    - definition: None added
    - term: Extra baggage
    - definition: None added
    - term: Meals
    - definition: None added
    - term: Priority services
    - definition: None added
    - paragraph: Pre-checkout total, 1 adult
    - paragraph: ₱8,730
    - paragraph: Includes simulated taxes and fees. Included fare benefits are never charged.
    - paragraph: "Not a booking: nothing is reserved or charged, and seat choices are provisional until checkout."
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
- alert: Review your booking | Velasia Airways
```

# Test source

```ts
  1  | import type { APIRequestContext, Page } from '@playwright/test';
  2  | import { reviewReady } from './booking-flow';
  3  | import { API_PORT } from './constants';
  4  | import { expect } from './helpers';
  5  | 
  6  | /* From the review to a confirmed booking (Milestone 5), shared by the checkout and manage specs. */
  7  | 
  8  | export const API = `http://127.0.0.1:${API_PORT}`;
  9  | export const ORDER_URL = /\/booking\/confirmation\/(ord_[A-Za-z0-9_-]{22})$/;
  10 | 
  11 | export const peso = (value: number) => `₱${value.toLocaleString('en-PH')}`;
  12 | 
  13 | /** From the review to checkout: the API re-validates and holds the seats. */
  14 | export async function toCheckout(page: Page) {
  15 |   await reviewReady(page);
  16 |   await page.getByTestId('continue-checkout').click();
  17 |   // Validation, the hold, and the checkout render: allow for a fully parallel run on a laptop.
> 18 |   await expect(page).toHaveURL(/\/booking\/checkout$/, { timeout: 20_000 });
     |                      ^ Error: expect(page).toHaveURL(expected) failed
  19 |   await expect(page.getByTestId('checkout')).toHaveAttribute(
  20 |     'data-order-status',
  21 |     'PENDING_PAYMENT',
  22 |   );
  23 |   await expect(page.locator('[data-testid="payment-form"][data-ready="true"]')).toBeVisible();
  24 | }
  25 | 
  26 | export const paymentForm = (page: Page) =>
  27 |   page.locator('[data-testid="payment-form"][data-ready="true"]');
  28 | 
  29 | export async function choosePayment(
  30 |   page: Page,
  31 |   method: 'Demo Card' | 'Demo Wallet' | 'Demo Bank Transfer',
  32 |   outcome: 'Approve' | 'Decline' | 'Time out' | 'Pending',
  33 | ) {
  34 |   const form = paymentForm(page);
  35 |   await form.getByRole('radio', { name: new RegExp(`^${method}`) }).check();
  36 |   await form.getByRole('radio', { name: new RegExp(`^${outcome}`) }).check();
  37 | }
  38 | 
  39 | export async function pay(
  40 |   page: Page,
  41 |   method: Parameters<typeof choosePayment>[1],
  42 |   outcome: Parameters<typeof choosePayment>[2],
  43 | ) {
  44 |   await choosePayment(page, method, outcome);
  45 |   await paymentForm(page).getByTestId('pay-button').click();
  46 | }
  47 | 
  48 | export async function expectConfirmed(page: Page) {
  49 |   // A payment is a Server Action, an API round trip, and a redirect render: allow for a fully
  50 |   // parallel run on a laptop.
  51 |   await expect(page).toHaveURL(ORDER_URL, { timeout: 20_000 });
  52 |   await expect(page.getByRole('heading', { name: 'Booking confirmed' })).toBeVisible();
  53 |   await expect(page.getByTestId('booking-reference')).toHaveText(/^VEL-[A-HJKMNP-Z2-9]{6}$/);
  54 |   return ORDER_URL.exec(page.url())![1]!;
  55 | }
  56 | 
  57 | /** Read the order straight from the retailing API with this browser's guest token. */
  58 | export async function apiOrder(page: Page, request: APIRequestContext, orderId: string) {
  59 |   const guest = (await page.context().cookies()).find((c) => c.name === 'velasia-guest')!.value;
  60 |   const response = await request.get(`${API}/v1/orders/${orderId}`, {
  61 |     headers: { authorization: `Bearer ${guest}` },
  62 |   });
  63 |   expect(response.status()).toBe(200);
  64 |   return (await response.json()).data;
  65 | }
  66 | 
```