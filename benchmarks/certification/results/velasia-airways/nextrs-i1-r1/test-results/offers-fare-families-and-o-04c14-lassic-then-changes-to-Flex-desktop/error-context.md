# Instructions

- Following Playwright test failed.
- Explain why, be concise, respect Playwright best practices.
- Provide a snippet of code with the fix, if possible.

# Test info

- Name: offers.spec.ts >> fare families and offers >> compares fares, selects Classic, then changes to Flex
- Location: tests/e2e/offers.spec.ts:51:3

# Error details

```
Error: expect(received).toEqual(expected) // deep equality

- Expected  -  1
+ Received  + 26

- Array []
+ Array [
+   "http 404: http://127.0.0.1:3200/_next/image?url=%2Fimages%2Fbrand%2Fhero.jpg&w=1600&q=80",
+   "console: Failed to load resource: the server responded with a status of 404 (Not Found)",
+   "http 404: http://127.0.0.1:3200/_next/image?url=%2Fimages%2Fdestinations%2Fosaka.jpg&w=828&q=70",
+   "console: Failed to load resource: the server responded with a status of 404 (Not Found)",
+   "http 404: http://127.0.0.1:3200/_next/image?url=%2Fimages%2Fdestinations%2Fcebu.jpg&w=1080&q=70",
+   "console: Failed to load resource: the server responded with a status of 404 (Not Found)",
+   "http 404: http://127.0.0.1:3200/_next/image?url=%2Fimages%2Fdestinations%2Fsingapore.jpg&w=1080&q=70",
+   "console: Failed to load resource: the server responded with a status of 404 (Not Found)",
+   "http 404: http://127.0.0.1:3200/_next/image?url=%2Fimages%2Fdestinations%2Ftokyo.jpg&w=828&q=70",
+   "console: Failed to load resource: the server responded with a status of 404 (Not Found)",
+   "http 404: http://127.0.0.1:3200/_next/image?url=%2Fimages%2Fdestinations%2Fseoul.jpg&w=828&q=70",
+   "console: Failed to load resource: the server responded with a status of 404 (Not Found)",
+   "http 404: http://127.0.0.1:3200/_next/image?url=%2Fimages%2Fdestinations%2Fbangkok.jpg&w=828&q=70",
+   "console: Failed to load resource: the server responded with a status of 404 (Not Found)",
+   "http 404: http://127.0.0.1:3200/_next/image?url=%2Fimages%2Fdestinations%2Fcebu.jpg&w=384&q=70",
+   "console: Failed to load resource: the server responded with a status of 404 (Not Found)",
+   "http 404: http://127.0.0.1:3200/_next/image?url=%2Fimages%2Fbrand%2Fwindow.jpg&w=828&q=70",
+   "console: Failed to load resource: the server responded with a status of 404 (Not Found)",
+   "http 404: http://127.0.0.1:3200/_next/image?url=%2Fimages%2Fdestinations%2Fhong-kong.jpg&w=384&q=70",
+   "console: Failed to load resource: the server responded with a status of 404 (Not Found)",
+   "http 404: http://127.0.0.1:3200/_next/image?url=%2Fimages%2Fdestinations%2Ftaipei.jpg&w=384&q=70",
+   "console: Failed to load resource: the server responded with a status of 404 (Not Found)",
+   "http 404: http://127.0.0.1:3200/_next/image?url=%2Fimages%2Fdestinations%2Fsingapore.jpg&w=384&q=70",
+   "console: Failed to load resource: the server responded with a status of 404 (Not Found)",
+ ]
```

# Page snapshot

```yaml
- generic [active] [ref=e1]:
  - link "Skip to content" [ref=e2] [cursor=pointer]:
    - /url: "#main"
  - paragraph [ref=e4]:
    - generic [ref=e7]:
      - strong [ref=e8]: Demo site.
      - text: Velasia Airways is a fictional airline — no real flights, bookings, or payments.
  - banner [ref=e9]:
    - generic [ref=e10]:
      - link "Velasia Airways home" [ref=e11] [cursor=pointer]:
        - /url: /
      - navigation "Primary" [ref=e19]:
        - list [ref=e20]:
          - listitem [ref=e21]:
            - link "Book flights" [ref=e22] [cursor=pointer]:
              - /url: /flights
          - listitem [ref=e23]:
            - link "Explore destinations" [ref=e24] [cursor=pointer]:
              - /url: /destinations
          - listitem [ref=e25]:
            - link "Manage booking" [ref=e26] [cursor=pointer]:
              - /url: /manage-booking
          - listitem [ref=e27]:
            - link "Help" [ref=e28] [cursor=pointer]:
              - /url: /help
      - generic [ref=e29]:
        - paragraph [ref=e30]:
          - generic [ref=e34]: English · PHP
        - link "Account" [ref=e35] [cursor=pointer]:
          - /url: /account
  - main [ref=e40]:
    - generic [ref=e41]:
      - generic [ref=e43]:
        - paragraph [ref=e44]: Book your trip · Step 1 of 4
        - heading "Traveller details" [level=1] [ref=e45]
        - paragraph [ref=e46]: Tell us who’s travelling. Names should match each traveller’s ID.
      - generic [ref=e48]:
        - navigation "Booking progress" [ref=e49]:
          - list [ref=e50]:
            - listitem [ref=e51]:
              - generic [ref=e52]:
                - generic [aria-hidden] [ref=e53]: "1"
                - generic [aria-hidden] [ref=e54]: Travellers
                - generic [ref=e55]: Travellers, current step
            - listitem [ref=e57]:
              - generic [ref=e58]:
                - generic [aria-hidden] [ref=e63]: Seats
                - generic [ref=e64]: Seats, not available yet
            - listitem [ref=e66]:
              - generic [ref=e67]:
                - generic [aria-hidden] [ref=e72]: Extras
                - generic [ref=e73]: Extras, not available yet
            - listitem [ref=e75]:
              - generic [ref=e76]:
                - generic [aria-hidden] [ref=e81]: Review
                - generic [ref=e82]: Review, not available yet
        - generic [ref=e83]:
          - generic [ref=e84]:
            - generic [ref=e85]:
              - paragraph [ref=e86]: Velasia is a fictional airline. Please use made-up details — don’t enter anyone’s real personal information. Names should match the traveller’s ID; no passport details are needed.
              - group "Adult 1" [ref=e87]:
                - generic [ref=e94]:
                  - generic [ref=e95]: Adult 1
                  - generic [ref=e96]: Aged 12 or over
                - generic [ref=e97]:
                  - generic [ref=e98]:
                    - generic [ref=e99]: Title (optional)
                    - combobox "Title (optional)" [ref=e100]:
                      - option "No title" [selected]
                      - option "Mr"
                      - option "Ms"
                      - option "Mrs"
                      - option "Mx"
                      - option "Dr"
                  - generic [ref=e101]:
                    - generic [ref=e102]: Given names
                    - textbox "Given names" [ref=e103]
                    - paragraph [ref=e104]: As on the traveller’s ID.
                  - generic [ref=e105]:
                    - generic [ref=e106]: Middle name (optional)
                    - textbox "Middle name (optional)" [ref=e107]
                  - generic [ref=e108]:
                    - generic [ref=e109]: Family name
                    - textbox "Family name" [ref=e110]
                  - generic [ref=e111]:
                    - generic [ref=e112]: Date of birth
                    - textbox "Date of birth" [ref=e113]
                    - paragraph [ref=e114]: 12 or older on Tue, Dec 15, 2026.
                  - generic [ref=e115]:
                    - generic [ref=e116]: Nationality
                    - combobox "Nationality" [ref=e117]:
                      - option "Choose a nationality" [selected]
                      - option "Philippines"
                      - option "Brunei"
                      - option "Cambodia"
                      - option "Indonesia"
                      - option "Laos"
                      - option "Malaysia"
                      - option "Myanmar"
                      - option "Singapore"
                      - option "Thailand"
                      - option "Timor-Leste"
                      - option "Vietnam"
                      - option "Australia"
                      - option "Brazil"
                      - option "Canada"
                      - option "China"
                      - option "France"
                      - option "Germany"
                      - option "Hong Kong SAR"
                      - option "India"
                      - option "Ireland"
                      - option "Italy"
                      - option "Japan"
                      - option "Mexico"
                      - option "Netherlands"
                      - option "New Zealand"
                      - option "Norway"
                      - option "Qatar"
                      - option "Saudi Arabia"
                      - option "South Korea"
                      - option "Spain"
                      - option "Sweden"
                      - option "Switzerland"
                      - option "Taiwan"
                      - option "United Arab Emirates"
                      - option "United Kingdom"
                      - option "United States"
                  - generic [ref=e118]:
                    - generic [ref=e119]: Special assistance (optional)
                    - combobox "Special assistance (optional)" [ref=e120]:
                      - option "No assistance needed" [selected]
                      - option "Wheelchair to and from the aircraft door"
                      - option "Support for blind or low-vision travellers"
                      - option "Support for deaf or hard-of-hearing travellers"
                      - option "Meet and assist through the airport"
                    - paragraph [ref=e121]: We’ll note the request for the airport team. No medical details needed.
              - group "Contact details" [ref=e122]:
                - generic [ref=e129]:
                  - generic [ref=e130]: Contact details
                  - generic [ref=e131]: Where we’d send trip updates. This demo never sends any messages.
                - generic [ref=e132]:
                  - generic [ref=e133]:
                    - generic [ref=e134]: Email
                    - textbox "Email" [ref=e135]
                  - generic [ref=e136]:
                    - generic [ref=e137]: Mobile phone
                    - textbox "Mobile phone" [ref=e138]
                    - paragraph [ref=e139]: With the country code, for example +63 917 555 0123.
              - generic [ref=e140]:
                - paragraph [ref=e141]: Saved only in your booking session, for a limited time.
                - button "Save and choose seats" [ref=e142]
            - generic [ref=e145]:
              - link "Back to trip summary" [ref=e146] [cursor=pointer]:
                - /url: /flights?trip=one-way&from=MNL&to=SIN&depart=2026-12-15&adults=1&cabin=economy&out=NV501-20261215
              - paragraph [ref=e149]:
                - text: Need to change who’s travelling?
                - link "Start a new search" [ref=e150] [cursor=pointer]:
                  - /url: /flights
                - text: — prices depend on the number of travellers.
          - complementary "MNL → SIN" [ref=e151]:
            - paragraph [ref=e152]: Your trip
            - heading "MNL → SIN" [level=2] [ref=e153]
            - generic [ref=e154]:
              - list [ref=e155]:
                - listitem [ref=e156]:
                  - generic [ref=e157]:
                    - generic [ref=e158]: Departing · MNL → SIN
                    - generic [ref=e159]: Tue, Dec 15, 2026 · 08:15
                    - generic [ref=e160]: VEL 501 · Velasia Flex
                  - generic [ref=e161]: ₱10,380
              - generic [ref=e162]:
                - generic [ref=e163]:
                  - term [ref=e164]: Flights, taxes and fees
                  - definition [ref=e165]: ₱10,380
                - generic [ref=e166]:
                  - term [ref=e167]: Seats
                  - definition [ref=e168]: None added
                - generic [ref=e169]:
                  - term [ref=e170]: Extra baggage
                  - definition [ref=e171]: None added
                - generic [ref=e172]:
                  - term [ref=e173]: Meals
                  - definition [ref=e174]: None added
                - generic [ref=e175]:
                  - term [ref=e176]: Priority services
                  - definition [ref=e177]: None added
              - generic [ref=e178]:
                - paragraph [ref=e179]: Pre-checkout total, 1 adult
                - paragraph [ref=e180]: ₱10,380
                - paragraph [ref=e181]: Includes simulated taxes and fees. Included fare benefits are never charged.
              - paragraph [ref=e182]: "Not a booking: nothing is reserved or charged, and seat choices are provisional until checkout."
  - contentinfo [ref=e186]:
    - generic [ref=e188]:
      - generic [ref=e189]:
        - link "Velasia Airways home" [ref=e190] [cursor=pointer]:
          - /url: /
        - paragraph [ref=e198]: Beyond every horizon.
        - paragraph [ref=e199]: A premium full-service airline for the Philippines, Southeast Asia, and beyond, flying from our hubs in Manila and Cebu — in a fictional network built to explore modern airline retailing.
      - generic [ref=e200]:
        - navigation "Book" [ref=e201]:
          - heading "Book" [level=2] [ref=e202]
          - list [ref=e203]:
            - listitem [ref=e204]:
              - link "Search flights" [ref=e205] [cursor=pointer]:
                - /url: /flights
            - listitem [ref=e206]:
              - link "Destinations" [ref=e207] [cursor=pointer]:
                - /url: /destinations
            - listitem [ref=e208]:
              - link "Manage booking" [ref=e209] [cursor=pointer]:
                - /url: /manage-booking
            - listitem [ref=e210]:
              - link "My Trips" [ref=e211] [cursor=pointer]:
                - /url: /account/trips
        - navigation "Help" [ref=e212]:
          - heading "Help" [level=2] [ref=e213]
          - list [ref=e214]:
            - listitem [ref=e215]:
              - link "Help centre" [ref=e216] [cursor=pointer]:
                - /url: /help
            - listitem [ref=e217]:
              - link "Baggage" [ref=e218] [cursor=pointer]:
                - /url: /help#baggage
            - listitem [ref=e219]:
              - link "Travel documents" [ref=e220] [cursor=pointer]:
                - /url: /help#documents
            - listitem [ref=e221]:
              - link "Contact us" [ref=e222] [cursor=pointer]:
                - /url: /help#contact
        - navigation "Velasia" [ref=e223]:
          - heading "Velasia" [level=2] [ref=e224]
          - list [ref=e225]:
            - listitem [ref=e226]:
              - link "About this demo" [ref=e227] [cursor=pointer]:
                - /url: /about
            - listitem [ref=e228]:
              - link "Photo credits" [ref=e229] [cursor=pointer]:
                - /url: /credits
    - generic [ref=e231]:
      - paragraph [ref=e232]: "© 2026 Velasia Airways. A fictional airline: no real reservations, tickets, payments, or financial transactions. All fares shown are illustrative."
      - paragraph [ref=e233]: Velasia Airways is a fictional airline created for demonstration. No real flights, bookings, or payments are offered.
  - alert [ref=e234]: Traveller details | Velasia Airways
```

# Test source

```ts
  37  |     .analyze();
  38  |   return violations
  39  |     .filter((v) => v.impact === 'critical' || v.impact === 'serious')
  40  |     .map((v) => `${v.id}: ${v.nodes.map((node) => node.target.join(' ')).join(' | ')}`);
  41  | }
  42  | 
  43  | /** Choose a one-way Manila → Singapore fare and land on the trip summary. */
  44  | async function oneWayWith(page: Page, family: 'Light' | 'Classic' | 'Flex' | 'Business') {
  45  |   await page.goto(ONE_WAY_SIN);
  46  |   await chooseFare(page, cards(page).first(), family);
  47  |   await expect(page.getByTestId('trip-summary')).toBeVisible();
  48  | }
  49  | 
  50  | test.describe('fare families and offers', () => {
  51  |   test('compares fares, selects Classic, then changes to Flex', async ({ page }) => {
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
> 137 |     expect(errors).toEqual([]);
      |                    ^ Error: expect(received).toEqual(expected) // deep equality
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
```