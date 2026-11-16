# Instructions

- Following Playwright test failed.
- Explain why, be concise, respect Playwright best practices.
- Provide a snippet of code with the fix, if possible.

# Test info

- Name: offers.spec.ts >> failures and accessibility >> compares and chooses fares with the keyboard alone
- Location: tests/e2e/offers.spec.ts:346:3

# Error details

```
Error: expect(locator).toBeVisible() failed

Locator: getByTestId('trip-summary')
Expected: visible
Timeout: 10000ms
Error: element(s) not found

Call log:
  - Expect "toBeVisible" getByTestId('trip-summary') with timeout 10000ms
  - waiting for getByTestId('trip-summary')

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
    - paragraph: Something went wrong while loading your flights. Your search is still in the address bar, and nothing has been booked or charged.
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
- alert
```

# Test source

```ts
  265 |     await simulateScenario(page, 'session-expired');
  266 |     await page.reload();
  267 |     const recovery = page.getByTestId('trip-recovery');
  268 |     await expect(recovery).toContainText('Your trip selection has expired');
  269 |     await simulateScenario(page, null);
  270 |     await recovery.getByRole('link', { name: 'Choose a fare for your departing flight' }).click();
  271 |     await expect(page.getByTestId('fare-cards')).toBeVisible();
  272 |     await selectFamily(page, 'Flex');
  273 |     await expect(page.getByTestId('family-outbound')).toHaveText('Velasia Flex');
  274 |   });
  275 | 
  276 |   test('keeps each guest’s selection private', async ({ page, browser }) => {
  277 |     await oneWayWith(page, 'Classic');
  278 |     const url = page.url();
  279 |     const other = await browser.newContext({ timezoneId: 'Asia/Manila', locale: 'en-PH' });
  280 |     const stranger = await other.newPage();
  281 |     await stranger.goto(url);
  282 |     await expect(stranger.getByTestId('trip-recovery')).toContainText('Choose your fares');
  283 |     await expect(stranger.getByText('Velasia Classic')).toHaveCount(0);
  284 |     await other.close();
  285 |   });
  286 | });
  287 | 
  288 | test.describe('failures and accessibility', () => {
  289 |   test('explains an unavailable offer service and recovers on retry', async ({ page }) => {
  290 |     await page.goto(ONE_WAY_SIN);
  291 |     await expect(cards(page).first()).toBeVisible();
  292 |     // 15–16. The API fails while loading fares: an accessible alert with a retry.
  293 |     await simulateApiFault(page, 'unavailable');
  294 |     await cards(page)
  295 |       .first()
  296 |       .getByRole('link', { name: /View fares/ })
  297 |       .click();
  298 |     const error = page.getByTestId('results-error');
  299 |     await expect(error).toHaveAttribute('role', 'alert');
  300 |     await expect(error).toContainText('We can’t load flights right now');
  301 |     expect(await axe(page)).toEqual([]);
  302 |     await simulateApiFault(page, null);
  303 |     await error.getByRole('button', { name: 'Try again' }).click();
  304 |     await expect(page.getByTestId('fare-cards')).toBeVisible();
  305 |   });
  306 | 
  307 |   test('reports a failed selection without losing the page', async ({ page }) => {
  308 |     await page.goto(`${ONE_WAY_SIN}&fares=NV501-20261215`);
  309 |     await expect(page.getByTestId('fare-cards')).toBeVisible();
  310 |     await simulateApiFault(page, 'unavailable');
  311 |     await selectFamily(page, 'Classic');
  312 |     const alert = fareCard(page, 'CLASSIC').getByTestId('fare-error');
  313 |     await expect(alert).toHaveAttribute('role', 'alert');
  314 |     await expect(alert).toContainText('nothing has changed');
  315 |     await expect(page).toHaveURL(/fares=NV501-20261215/);
  316 |   });
  317 | 
  318 |   test('times out gracefully', async ({ page }) => {
  319 |     await page.goto(ONE_WAY_SIN);
  320 |     await simulateApiFault(page, 'timeout');
  321 |     await cards(page)
  322 |       .first()
  323 |       .getByRole('link', { name: /View fares/ })
  324 |       .click();
  325 |     await expect(page.getByTestId('results-error')).toContainText('taking too long', {
  326 |       timeout: 10_000,
  327 |     });
  328 |   });
  329 | 
  330 |   test('fare pages and the trip summary have no serious axe violations', async ({ page }) => {
  331 |     await page.goto(`${ONE_WAY_SIN}&fares=NV501-20261215`);
  332 |     await expect(page.getByTestId('fare-cards')).toBeVisible();
  333 |     await page.getByRole('button', { name: 'Compare all features' }).click();
  334 |     await expect(page.getByRole('table')).toBeVisible();
  335 |     expect(await axe(page)).toEqual([]);
  336 |     await selectFamily(page, 'Classic');
  337 |     await expect(page.getByTestId('trip-summary')).toBeVisible();
  338 |     expect(await axe(page)).toEqual([]);
  339 |     await page.getByRole('button', { name: 'Continue to traveller details' }).click();
  340 |     await expect(page).toHaveURL(/\/booking\/travellers$/);
  341 |     // After a client navigation the title is applied once the new page's metadata arrives.
  342 |     await expect(page).toHaveTitle(/Traveller details/);
  343 |     expect(await axe(page)).toEqual([]);
  344 |   });
  345 | 
  346 |   test('compares and chooses fares with the keyboard alone', async ({ page }) => {
  347 |     await page.goto(`${ONE_WAY_SIN}&fares=NV501-20261215`);
  348 |     await expect(page.getByTestId('fare-cards')).toBeVisible();
  349 |     // 18. Open the comparison and choose a fare without a pointer.
  350 |     const compare = page.getByRole('button', { name: 'Compare all features' });
  351 |     await compare.focus();
  352 |     await page.keyboard.press('Enter');
  353 |     await expect(page.getByRole('table')).toBeVisible();
  354 |     await expect(page.getByRole('button', { name: 'Hide comparison' })).toHaveAttribute(
  355 |       'aria-expanded',
  356 |       'true',
  357 |     );
  358 |     const breakdown = fareCard(page, 'CLASSIC').getByText('Price breakdown');
  359 |     await breakdown.focus();
  360 |     await page.keyboard.press('Enter');
  361 |     await expect(fareCard(page, 'CLASSIC')).toContainText('Total, all travellers');
  362 |     const select = fareCard(page, 'CLASSIC').getByRole('button', { name: /Select Classic/ });
  363 |     await select.focus();
  364 |     await page.keyboard.press('Enter');
> 365 |     await expect(page.getByTestId('trip-summary')).toBeVisible();
      |                                                    ^ Error: expect(locator).toBeVisible() failed
  366 |     const proceed = page.getByRole('button', { name: 'Continue to traveller details' });
  367 |     await proceed.focus();
  368 |     await page.keyboard.press('Enter');
  369 |     await expect(page).toHaveURL(/\/booking\/travellers$/);
  370 |   });
  371 | });
  372 | 
  373 | test.describe('mobile', () => {
  374 |   test('compares and selects fares on a phone @mobile', async ({ page, isMobile }) => {
  375 |     test.skip(!isMobile, 'mobile only');
  376 |     const errors = trackPageErrors(page);
  377 |     await page.goto(ONE_WAY_SIN);
  378 |     await cards(page)
  379 |       .first()
  380 |       .getByRole('link', { name: /View fares/ })
  381 |       .click();
  382 |     await expect(page.getByTestId('fare-cards')).toBeVisible();
  383 |     await page.getByRole('button', { name: 'Compare all features' }).click();
  384 |     // Phones get one list per feature instead of a wide table.
  385 |     await expect(page.getByRole('table')).toBeHidden();
  386 |     await expect(page.getByRole('heading', { name: 'Checked baggage' })).toBeVisible();
  387 |     const overflow = () =>
  388 |       page.evaluate(() => document.documentElement.scrollWidth - window.innerWidth);
  389 |     expect(await overflow()).toBeLessThanOrEqual(0);
  390 | 
  391 |     await selectFamily(page, 'Flex');
  392 |     await expect(page.getByTestId('trip-summary')).toBeVisible();
  393 |     expect(await overflow()).toBeLessThanOrEqual(0);
  394 |     await page.getByRole('link', { name: 'Review and continue' }).click();
  395 |     await expect(
  396 |       page.getByRole('button', { name: 'Continue to traveller details' }),
  397 |     ).toBeInViewport();
  398 |     expect(errors).toEqual([]);
  399 |   });
  400 | });
  401 | 
```