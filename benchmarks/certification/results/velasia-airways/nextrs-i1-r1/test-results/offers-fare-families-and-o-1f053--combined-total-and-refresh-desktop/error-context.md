# Instructions

- Following Playwright test failed.
- Explain why, be concise, respect Playwright best practices.
- Provide a snippet of code with the fix, if possible.

# Test info

- Name: offers.spec.ts >> fare families and offers >> round trip with different families, a combined total, and refresh
- Location: tests/e2e/offers.spec.ts:140:3

# Error details

```
Error: expect(locator).toContainText(expected) failed

Locator: getByTestId('selected-outbound')
Expected substring: "Velasia Classic"
Timeout: 10000ms
Error: element(s) not found

Call log:
  - Expect "toContainText" getByTestId('selected-outbound') with timeout 10000ms
  - waiting for getByTestId('selected-outbound')

```

```yaml
- main:
  - heading "Velasia Airways is temporarily unavailable" [level=1]
  - paragraph: Please try again in a moment.
  - button "Try again"
```

# Test source

```ts
  52  |     const errors = trackPageErrors(page);
  53  |     const requests: string[] = [];
  54  |     page.on('request', (request) => requests.push(request.url()));
  55  | 
  56  |     // 1–2. Search Manila to Singapore and open an itinerary's fares.
  57  |     await page.goto('/');
  58  |     await chooseAirport(page, 'From', 'Manila', /Manila/);
  59  |     await chooseAirport(page, 'To', 'Singapore', /Singapore/);
  60  |     await page.getByText('One-way', { exact: true }).click();
  61  |     await page.getByLabel('Depart', { exact: true }).fill(manilaDate(14));
  62  |     await page.getByRole('button', { name: 'Search flights' }).click();
  63  |     const first = cards(page).first();
  64  |     await expect(first).toContainText('VEL 501');
  65  |     const fromPrice = await amount(
  66  |       first.locator('p:has-text("per adult") >> xpath=preceding-sibling::p[1]'),
  67  |     );
  68  |     await first.getByRole('link', { name: /View fares/ }).click();
  69  |     await expect(page).toHaveURL(/fares=NV501-20261215/);
  70  | 
  71  |     // 3–4. Compare Light, Classic, and Flex.
  72  |     await expect(page.getByRole('heading', { name: 'Choose your fare' })).toBeVisible();
  73  |     await expect(page.getByTestId('fare-itinerary')).toContainText('Airbus A321neo');
  74  |     const [light, classic, flex] = await Promise.all(
  75  |       ['LIGHT', 'CLASSIC', 'FLEX'].map((family) =>
  76  |         amount(fareCard(page, family).getByTestId('fare-total')),
  77  |       ),
  78  |     );
  79  |     expect(light).toBe(fromPrice); // the results' "from" price is the Light offer
  80  |     expect(light).toBeLessThan(classic!);
  81  |     expect(classic).toBeLessThan(flex!);
  82  |     await expect(fareCard(page, 'LIGHT')).toContainText('Not included');
  83  |     await expect(fareCard(page, 'CLASSIC')).toContainText('1 × 20 kg');
  84  |     await expect(fareCard(page, 'FLEX')).toContainText('No change fee');
  85  |     await expect(fareCard(page, 'BUSINESS')).toContainText('Business');
  86  | 
  87  |     await page.getByRole('button', { name: 'Compare all features' }).click();
  88  |     const matrix = page.getByRole('table');
  89  |     await expect(matrix).toBeVisible();
  90  |     await expect(matrix.getByRole('row', { name: /Checked baggage/ })).toContainText(
  91  |       'Not included',
  92  |     );
  93  |     await expect(matrix.getByRole('row', { name: /Refunds/ })).toContainText('₱3,000 fee');
  94  |     await expect(page.getByRole('button', { name: 'Hide comparison' })).toHaveAttribute(
  95  |       'aria-expanded',
  96  |       'true',
  97  |     );
  98  | 
  99  |     // 5–6. Select Classic and check the price breakdown.
  100 |     await selectFamily(page, 'Classic');
  101 |     await expect(page).toHaveURL(/out=NV501-20261215/);
  102 |     await expect(page.getByTestId('family-outbound')).toHaveText('Velasia Classic');
  103 |     await expect(page.getByTestId('status-outbound')).toHaveText('Selected');
  104 |     expect(await amount(page.getByTestId('trip-total'))).toBe(classic);
  105 |     const panel = page.locator('#trip-summary-panel');
  106 |     const parts = await Promise.all(
  107 |       ['Base fares', 'Fare benefits', 'Taxes and fees'].map((label) =>
  108 |         amount(panel.locator(`dt:text-is("${label}") + dd`)),
  109 |       ),
  110 |     );
  111 |     expect(parts.reduce((sum, part) => sum + part, 0)).toBe(classic);
  112 |     await page.getByTestId('selection-outbound').getByText('Price breakdown').click();
  113 |     await expect(page.getByTestId('selection-outbound')).toContainText('Classic fare benefits');
  114 |     await expect(page.getByTestId('offer-validity')).toContainText('Prices held until');
  115 | 
  116 |     // 7–8. Change the fare family to Flex and see the new authoritative total.
  117 |     await page.getByRole('link', { name: 'Change fare for the departing flight' }).click();
  118 |     await expect(fareCard(page, 'CLASSIC')).toContainText('Your choice');
  119 |     await expect(fareCard(page, 'FLEX').getByTestId('fare-difference')).toContainText(
  120 |       `compared with Velasia Classic`,
  121 |     );
  122 |     await selectFamily(page, 'Flex');
  123 |     await expect(page.getByTestId('family-outbound')).toHaveText('Velasia Flex');
  124 |     expect(await amount(page.getByTestId('trip-total'))).toBe(flex);
  125 |     await expect(page.getByTestId('changed-outbound')).toContainText(
  126 |       'Changed from Velasia Classic',
  127 |     );
  128 | 
  129 |     // 20. Nothing was booked or paid for.
  130 |     await page.getByRole('button', { name: 'Continue to traveller details' }).click();
  131 |     // M4: the next step collects traveller details; the trip total carries over unchanged.
  132 |     await expect(page.getByTestId('traveller-form')).toBeVisible();
  133 |     expect(await amount(page.getByTestId('booking-total').filter({ visible: true }))).toBe(flex);
  134 |     expect(requests.filter((url) => /order|payment|checkout/i.test(url))).toEqual([]);
  135 | 
  136 |     // 19. No hydration or runtime errors along the way.
  137 |     expect(errors).toEqual([]);
  138 |   });
  139 | 
  140 |   test('round trip with different families, a combined total, and refresh', async ({ page }) => {
  141 |     const errors = trackPageErrors(page);
  142 |     await page.goto(ROUND_SIN);
  143 |     await expect(page.getByRole('heading', { name: 'Choose your departing flight' })).toBeVisible();
  144 |     await cards(page)
  145 |       .first()
  146 |       .getByRole('link', { name: /View fares/ })
  147 |       .click();
  148 |     await expect(page.getByRole('heading', { name: 'Choose your departing fare' })).toBeVisible();
  149 |     const outClassic = await amount(fareCard(page, 'CLASSIC').getByTestId('fare-total'));
  150 |     await selectFamily(page, 'Classic');
  151 | 
> 152 |     await expect(page.getByTestId('selected-outbound')).toContainText('Velasia Classic');
      |                                                         ^ Error: expect(locator).toContainText(expected) failed
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
  246 |     await expect(page.getByTestId('trip-summary')).toBeVisible();
  247 | 
  248 |     await simulateScenario(page, 'sold-out');
  249 |     await page.getByRole('button', { name: 'Check latest price' }).click();
  250 |     await expect(page.getByTestId('unavailable-outbound')).toContainText(
  251 |       'can no longer be sold for your party',
  252 |     );
```