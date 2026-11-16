# Instructions

- Following Playwright test failed.
- Explain why, be concise, respect Playwright best practices.
- Provide a snippet of code with the fix, if possible.

# Test info

- Name: offers.spec.ts >> offer lifecycle >> marks a sold-out flight and keeps the other leg
- Location: tests/e2e/offers.spec.ts:242:3

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
- alert: Book flights | Velasia Airways
```

# Test source

```ts
  146 |       .getByRole('link', { name: /View fares/ })
  147 |       .click();
  148 |     await expect(page.getByRole('heading', { name: 'Choose your departing fare' })).toBeVisible();
  149 |     const outClassic = await amount(fareCard(page, 'CLASSIC').getByTestId('fare-total'));
  150 |     await selectFamily(page, 'Classic');
  151 | 
  152 |     await expect(page.getByTestId('selected-outbound')).toContainText('Velasia Classic');
  153 |     await expect(page.getByRole('heading', { name: 'Choose your return flight' })).toBeVisible();
  154 |     await cards(page)
  155 |       .first()
  156 |       .getByRole('link', { name: /View fares/ })
  157 |       .click();
  158 |     await expect(page.getByRole('heading', { name: 'Choose your return fare' })).toBeVisible();
  159 |     const backFlex = await amount(fareCard(page, 'FLEX').getByTestId('fare-total'));
  160 |     await selectFamily(page, 'Flex');
  161 | 
  162 |     await expect(page.getByTestId('family-outbound')).toHaveText('Velasia Classic');
  163 |     await expect(page.getByTestId('family-inbound')).toHaveText('Velasia Flex');
  164 |     expect(await amount(page.getByTestId('leg-total-outbound'))).toBe(outClassic);
  165 |     expect(await amount(page.getByTestId('leg-total-inbound'))).toBe(backFlex);
  166 |     expect(await amount(page.getByTestId('trip-total'))).toBe(outClassic + backFlex);
  167 | 
  168 |     // Editing the return fare keeps the departing selection.
  169 |     await page.getByRole('link', { name: 'Change fare for the return flight' }).click();
  170 |     await selectFamily(page, 'Light');
  171 |     await expect(page.getByTestId('family-outbound')).toHaveText('Velasia Classic');
  172 |     await expect(page.getByTestId('family-inbound')).toHaveText('Velasia Light');
  173 |     const total = await amount(page.getByTestId('trip-total'));
  174 | 
  175 |     // 12. Refresh restores the session-backed selection.
  176 |     await page.reload();
  177 |     await expect(page.getByTestId('family-inbound')).toHaveText('Velasia Light');
  178 |     expect(await amount(page.getByTestId('trip-total'))).toBe(total);
  179 |     expect(errors).toEqual([]);
  180 |   });
  181 | 
  182 |   test('explains families that are not offered on a flight', async ({ page }) => {
  183 |     await page.goto(ECONOMY_ONLY);
  184 |     // VEL 103 is an Airbus A320neo, with no Business cabin.
  185 |     await cards(page)
  186 |       .filter({ hasText: 'VEL 103' })
  187 |       .getByRole('link', { name: /View fares/ })
  188 |       .click();
  189 |     await expect(fareCard(page, 'BUSINESS').getByTestId('fare-unavailable')).toContainText(
  190 |       'A320neo has no Business cabin',
  191 |     );
  192 |     await expect(fareCard(page, 'BUSINESS').getByRole('button')).toHaveCount(0);
  193 |     await expect(
  194 |       fareCard(page, 'LIGHT').getByRole('button', { name: /Select Light/ }),
  195 |     ).toBeVisible();
  196 |   });
  197 | });
  198 | 
  199 | test.describe('offer lifecycle', () => {
  200 |   test('recovers from an expired offer at the same price', async ({ page }) => {
  201 |     await oneWayWith(page, 'Classic');
  202 |     const total = await amount(page.getByTestId('trip-total'));
  203 | 
  204 |     // 13. Sixteen minutes later (simulated), the 15-minute price hold has ended.
  205 |     await simulateScenario(page, 'offer-expired');
  206 |     await page.reload();
  207 |     await expect(page.getByTestId('status-outbound')).toHaveText('Price hold ended');
  208 |     await expect(page.getByTestId('expired-outbound')).toBeVisible();
  209 | 
  210 |     // 14. Continuing asks the API for a fresh offer and says so, instead of going ahead silently.
  211 |     await page.getByRole('button', { name: 'Continue to traveller details' }).click();
  212 |     await expect(page.getByTestId('refreshed-outbound')).toContainText('The price is unchanged');
  213 |     await expect(page.getByTestId('status-outbound')).toHaveText('Price confirmed');
  214 |     expect(await amount(page.getByTestId('trip-total'))).toBe(total);
  215 |     await expect(page.getByTestId('trip-action-status')).toContainText('Something changed');
  216 | 
  217 |     await page.getByRole('button', { name: 'Continue to traveller details' }).click();
  218 |     await expect(page).toHaveURL(/\/booking\/travellers$/);
  219 |   });
  220 | 
  221 |   test('requires explicit acceptance of a changed price', async ({ page }) => {
  222 |     await oneWayWith(page, 'Classic');
  223 |     const before = await amount(page.getByTestId('trip-total'));
  224 | 
  225 |     await simulateScenario(page, 'price-increase');
  226 |     await page.getByRole('button', { name: 'Check latest price' }).click();
  227 |     const notice = page.getByTestId('price-changed-outbound');
  228 |     await expect(notice).toBeVisible();
  229 |     await expect(notice).toHaveAttribute('role', 'alert');
  230 |     const after = await amount(page.getByTestId('new-price-outbound'));
  231 |     expect(after).toBeGreaterThan(before);
  232 |     // Not applied yet: the total is still the accepted price, and continuing is blocked.
  233 |     expect(await amount(page.getByTestId('trip-total'))).toBe(before);
  234 |     await expect(page.getByTestId('continue-blocked')).toContainText('Accept the new price');
  235 | 
  236 |     await notice.getByRole('button', { name: /^Accept/ }).click();
  237 |     await expect(page.getByTestId('status-outbound')).toHaveText('Price confirmed');
  238 |     expect(await amount(page.getByTestId('trip-total'))).toBe(after);
  239 |     await expect(page.getByTestId('changed-outbound')).toContainText('You accepted a new price');
  240 |   });
  241 | 
  242 |   test('marks a sold-out flight and keeps the other leg', async ({ page }) => {
  243 |     await page.goto(ROUND_SIN);
  244 |     await chooseFare(page, cards(page).first(), 'Light');
  245 |     await chooseFare(page, cards(page).first(), 'Light');
> 246 |     await expect(page.getByTestId('trip-summary')).toBeVisible();
      |                                                    ^ Error: expect(locator).toBeVisible() failed
  247 | 
  248 |     await simulateScenario(page, 'sold-out');
  249 |     await page.getByRole('button', { name: 'Check latest price' }).click();
  250 |     await expect(page.getByTestId('unavailable-outbound')).toContainText(
  251 |       'can no longer be sold for your party',
  252 |     );
  253 |     await expect(page.getByTestId('family-inbound')).toHaveText('Velasia Light');
  254 |     await expect(page.getByTestId('continue-blocked')).toBeVisible();
  255 |     await simulateScenario(page, null);
  256 |     await page
  257 |       .getByTestId('unavailable-outbound')
  258 |       .getByRole('link', { name: 'Choose another flight' })
  259 |       .click();
  260 |     await expect(page.getByRole('heading', { name: 'Choose your departing flight' })).toBeVisible();
  261 |   });
  262 | 
  263 |   test('offers a way back when the shopping session has expired', async ({ page }) => {
  264 |     await oneWayWith(page, 'Light');
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
```