# Instructions

- Following Playwright test failed.
- Explain why, be concise, respect Playwright best practices.
- Provide a snippet of code with the fix, if possible.

# Test info

- Name: flights.spec.ts >> flight results >> searches Manila to Singapore and shows real results
- Location: tests/e2e/flights.spec.ts:40:3

# Error details

```
Error: expect(received).toEqual(expected) // deep equality

- Expected  -  1
+ Received  + 26

- Array []
+ Array [
+   "http 404: http://127.0.0.1:3200/_next/image?url=%2Fimages%2Fbrand%2Fhero.jpg&w=1600&q=80",
+   "console: Failed to load resource: the server responded with a status of 404 (Not Found)",
+   "http 404: http://127.0.0.1:3200/_next/image?url=%2Fimages%2Fdestinations%2Fcebu.jpg&w=1080&q=70",
+   "console: Failed to load resource: the server responded with a status of 404 (Not Found)",
+   "http 404: http://127.0.0.1:3200/_next/image?url=%2Fimages%2Fdestinations%2Fsingapore.jpg&w=1080&q=70",
+   "console: Failed to load resource: the server responded with a status of 404 (Not Found)",
+   "http 404: http://127.0.0.1:3200/_next/image?url=%2Fimages%2Fdestinations%2Ftokyo.jpg&w=828&q=70",
+   "console: Failed to load resource: the server responded with a status of 404 (Not Found)",
+   "http 404: http://127.0.0.1:3200/_next/image?url=%2Fimages%2Fdestinations%2Fbangkok.jpg&w=828&q=70",
+   "console: Failed to load resource: the server responded with a status of 404 (Not Found)",
+   "http 404: http://127.0.0.1:3200/_next/image?url=%2Fimages%2Fdestinations%2Fseoul.jpg&w=828&q=70",
+   "console: Failed to load resource: the server responded with a status of 404 (Not Found)",
+   "http 404: http://127.0.0.1:3200/_next/image?url=%2Fimages%2Fdestinations%2Fosaka.jpg&w=828&q=70",
+   "console: Failed to load resource: the server responded with a status of 404 (Not Found)",
+   "http 404: http://127.0.0.1:3200/_next/image?url=%2Fimages%2Fdestinations%2Fhong-kong.jpg&w=384&q=70",
+   "console: Failed to load resource: the server responded with a status of 404 (Not Found)",
+   "http 404: http://127.0.0.1:3200/_next/image?url=%2Fimages%2Fdestinations%2Fcebu.jpg&w=384&q=70",
+   "console: Failed to load resource: the server responded with a status of 404 (Not Found)",
+   "http 404: http://127.0.0.1:3200/_next/image?url=%2Fimages%2Fdestinations%2Fsingapore.jpg&w=384&q=70",
+   "console: Failed to load resource: the server responded with a status of 404 (Not Found)",
+   "http 404: http://127.0.0.1:3200/_next/image?url=%2Fimages%2Fdestinations%2Ftaipei.jpg&w=384&q=70",
+   "console: Failed to load resource: the server responded with a status of 404 (Not Found)",
+   "http 404: http://127.0.0.1:3200/_next/image?url=%2Fimages%2Fbrand%2Fwindow.jpg&w=828&q=70",
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
    - generic [ref=e43]:
      - paragraph [ref=e44]: Book flights
      - heading "Flights from Manila to Singapore" [level=1] [ref=e45]
      - region "Your search" [ref=e47]:
        - heading "Your search" [level=2] [ref=e48]
        - generic [ref=e49]:
          - paragraph [ref=e50]:
            - generic [ref=e51]:
              - generic [ref=e52]: MNL
              - generic [ref=e53]: Manila
              - generic [ref=e54]: Ninoy Aquino International Airport
            - img "to" [ref=e55]
            - generic [ref=e57]:
              - generic [ref=e58]: SIN
              - generic [ref=e59]: Singapore
              - generic [ref=e60]: Singapore Changi Airport
          - button "Edit search" [ref=e62]
          - list "Search details" [ref=e66]:
            - listitem [ref=e67]: One-way
            - listitem [ref=e71]: Tue, Dec 15, 2026
            - listitem [ref=e75]: 1 Adult
            - listitem [ref=e82]: Economy
    - generic [ref=e87]:
      - generic [ref=e88]:
        - list "Booking steps" [ref=e89]:
          - listitem [ref=e90]:
            - generic [ref=e91]: "1"
            - generic [ref=e92]: Choose flight
          - listitem [ref=e94]:
            - generic [ref=e95]: "2"
            - generic [ref=e96]: Trip summary
        - paragraph [ref=e97]: "Then: travellers · seats · extras · review"
      - generic [ref=e98]:
        - heading "Choose your flight" [level=2] [ref=e99]
        - paragraph [ref=e100]: Manila to Singapore · Tue, Dec 15, 2026
      - navigation "Nearby dates" [ref=e101]:
        - list [ref=e102]:
          - listitem [ref=e103]:
            - link "Sat Dec 12 from ₱8,030 per adult" [ref=e104] [cursor=pointer]:
              - /url: /flights?trip=one-way&from=MNL&to=SIN&depart=2026-12-12&adults=1&cabin=economy
              - generic [ref=e105]: Sat Dec 12
              - generic [ref=e106]:
                - generic [ref=e107]: from
                - text: ₱8,030
                - generic [ref=e108]: per adult
          - listitem [ref=e109]:
            - link "Sun Dec 13 from ₱8,560 per adult" [ref=e110] [cursor=pointer]:
              - /url: /flights?trip=one-way&from=MNL&to=SIN&depart=2026-12-13&adults=1&cabin=economy
              - generic [ref=e111]: Sun Dec 13
              - generic [ref=e112]:
                - generic [ref=e113]: from
                - text: ₱8,560
                - generic [ref=e114]: per adult
          - listitem [ref=e115]:
            - link "Mon Dec 14 from ₱7,860 per adult" [ref=e116] [cursor=pointer]:
              - /url: /flights?trip=one-way&from=MNL&to=SIN&depart=2026-12-14&adults=1&cabin=economy
              - generic [ref=e117]: Mon Dec 14
              - generic [ref=e118]:
                - generic [ref=e119]: from
                - text: ₱7,860
                - generic [ref=e120]: per adult
          - listitem [ref=e121]:
            - generic [ref=e122]:
              - generic [ref=e123]: Tue Dec 15
              - generic [ref=e124]:
                - generic [ref=e125]: from
                - text: ₱7,540
                - generic [ref=e126]: per adult
          - listitem [ref=e127]:
            - link "Wed Dec 16 from ₱6,740 per adult" [ref=e128] [cursor=pointer]:
              - /url: /flights?trip=one-way&from=MNL&to=SIN&depart=2026-12-16&adults=1&cabin=economy
              - generic [ref=e129]: Wed Dec 16
              - generic [ref=e130]:
                - generic [ref=e131]: from
                - text: ₱6,740
                - generic [ref=e132]: per adult
          - listitem [ref=e133]:
            - link "Thu Dec 17 from ₱6,700 per adult , lowest nearby fare" [ref=e134] [cursor=pointer]:
              - /url: /flights?trip=one-way&from=MNL&to=SIN&depart=2026-12-17&adults=1&cabin=economy
              - generic [ref=e135]: Thu Dec 17
              - generic [ref=e136]:
                - generic [ref=e137]: from
                - text: ₱6,700
                - generic [ref=e138]: per adult
              - generic [ref=e139]: ", lowest nearby fare"
          - listitem [ref=e140]:
            - link "Fri Dec 18 from ₱9,070 per adult" [ref=e141] [cursor=pointer]:
              - /url: /flights?trip=one-way&from=MNL&to=SIN&depart=2026-12-18&adults=1&cabin=economy
              - generic [ref=e142]: Fri Dec 18
              - generic [ref=e143]:
                - generic [ref=e144]: from
                - text: ₱9,070
                - generic [ref=e145]: per adult
      - generic [ref=e146]:
        - complementary "Filters" [ref=e147]:
          - generic [ref=e148]:
            - heading "Filter flights" [level=3] [ref=e149]
            - generic [ref=e150]:
              - group "Stops" [ref=e151]:
                - generic [ref=e153]:
                  - generic [ref=e154] [cursor=pointer]:
                    - radio "Any number of stops 2 flights" [checked] [ref=e155]
                    - generic [ref=e156]: Any number of stops
                    - generic [ref=e157]:
                      - text: "2"
                      - generic [ref=e158]: flights
                  - generic [ref=e159] [cursor=pointer]:
                    - radio "Direct flights only 2 flights" [ref=e160]
                    - generic [ref=e161]: Direct flights only
                    - generic [ref=e162]:
                      - text: "2"
                      - generic [ref=e163]: flights
              - group "Departure time" [ref=e164]:
                - generic [ref=e166]:
                  - generic [ref=e167]:
                    - checkbox "Early morning 00:00–05:59 0 flights" [disabled] [ref=e168]
                    - generic [ref=e169]:
                      - text: Early morning
                      - generic [ref=e170]: 00:00–05:59
                    - generic [ref=e171]:
                      - text: "0"
                      - generic [ref=e172]: flights
                  - generic [ref=e173] [cursor=pointer]:
                    - checkbox "Morning 06:00–11:59 1 flights" [ref=e174]
                    - generic [ref=e175]:
                      - text: Morning
                      - generic [ref=e176]: 06:00–11:59
                    - generic [ref=e177]:
                      - text: "1"
                      - generic [ref=e178]: flights
                  - generic [ref=e179] [cursor=pointer]:
                    - checkbox "Afternoon 12:00–17:59 1 flights" [ref=e180]
                    - generic [ref=e181]:
                      - text: Afternoon
                      - generic [ref=e182]: 12:00–17:59
                    - generic [ref=e183]:
                      - text: "1"
                      - generic [ref=e184]: flights
                  - generic [ref=e185]:
                    - checkbox "Evening 18:00–23:59 0 flights" [disabled] [ref=e186]
                    - generic [ref=e187]:
                      - text: Evening
                      - generic [ref=e188]: 18:00–23:59
                    - generic [ref=e189]:
                      - text: "0"
                      - generic [ref=e190]: flights
              - generic [ref=e191]:
                - text: Maximum price
                - paragraph [aria-hidden] [ref=e192]: Any price per adult
                - slider "Maximum price" [ref=e193]: "7700"
                - generic [aria-hidden] [ref=e194]:
                  - generic [ref=e195]: ₱7,500
                  - generic [ref=e196]: ₱7,700
              - group "Aircraft" [ref=e197]:
                - generic [ref=e199]:
                  - generic [ref=e200] [cursor=pointer]:
                    - checkbox "Airbus A321neo 1 flights" [ref=e201]
                    - generic [ref=e202]: Airbus A321neo
                    - generic [ref=e203]:
                      - text: "1"
                      - generic [ref=e204]: flights
                  - generic [ref=e205] [cursor=pointer]:
                    - checkbox "Airbus A330-900 1 flights" [ref=e206]
                    - generic [ref=e207]: Airbus A330-900
                    - generic [ref=e208]:
                      - text: "1"
                      - generic [ref=e209]: flights
        - generic [ref=e210]:
          - generic [ref=e211]:
            - paragraph [ref=e212]: 2 flights
            - generic [ref=e213]:
              - generic [ref=e214]: Sort by
              - combobox "Sort by" [ref=e216] [cursor=pointer]:
                - option "Earliest departure" [selected]
                - option "Latest departure"
                - option "Lowest price"
                - option "Shortest duration"
          - generic [ref=e217]:
            - status [ref=e218]
            - list "Flights" [ref=e219]:
              - listitem [ref=e220]:
                - article [ref=e221]:
                  - heading "Manila to Singapore, departs 08:15, arrives 12:00" [level=3] [ref=e222]
                  - generic [ref=e223]:
                    - generic [ref=e224]:
                      - generic [ref=e225]:
                        - generic [ref=e226]:
                          - generic [ref=e231]: Velasia
                          - generic [ref=e232]: VEL 501
                        - paragraph [ref=e233]: Airbus A321neo
                      - generic [ref=e235]:
                        - generic [ref=e236]:
                          - paragraph [ref=e237]: 08:15
                          - paragraph [ref=e238]:
                            - text: MNL
                            - generic [ref=e239]: ", Manila"
                        - generic [ref=e240]:
                          - paragraph [ref=e241]:
                            - text: 3h 45m
                            - generic [ref=e242]: Total journey 3 hours 45 minutes
                          - paragraph [ref=e248]: Direct
                        - generic [ref=e249]:
                          - paragraph [ref=e250]: 12:00
                          - paragraph [ref=e251]:
                            - text: SIN
                            - generic [ref=e252]: ", Singapore"
                      - paragraph [ref=e253]:
                        - generic [ref=e254]: Economy
                    - generic [ref=e255]:
                      - generic [ref=e256]:
                        - paragraph [ref=e257]: From
                        - paragraph [ref=e258]: ₱7,540
                        - paragraph [ref=e259]: per adult · indicative
                      - generic [ref=e260]:
                        - link "View fares Manila 08:15 flight" [ref=e261] [cursor=pointer]:
                          - /url: /flights?trip=one-way&from=MNL&to=SIN&depart=2026-12-15&adults=1&cabin=economy&fares=NV501-20261215
                          - text: View fares
                          - generic [ref=e262]: Manila 08:15 flight
                        - button "Details for Manila to Singapore, departing 08:15" [ref=e265]:
                          - text: Details
                          - generic [ref=e266]: for Manila to Singapore, departing 08:15
              - listitem [ref=e267]:
                - article [ref=e268]:
                  - heading "Manila to Singapore, departs 14:40, arrives 18:25" [level=3] [ref=e269]
                  - generic [ref=e270]:
                    - generic [ref=e271]:
                      - generic [ref=e272]:
                        - generic [ref=e273]:
                          - generic [ref=e278]: Velasia
                          - generic [ref=e279]: VEL 503
                        - paragraph [ref=e280]: Airbus A330-900
                      - generic [ref=e282]:
                        - generic [ref=e283]:
                          - paragraph [ref=e284]: 14:40
                          - paragraph [ref=e285]:
                            - text: MNL
                            - generic [ref=e286]: ", Manila"
                        - generic [ref=e287]:
                          - paragraph [ref=e288]:
                            - text: 3h 45m
                            - generic [ref=e289]: Total journey 3 hours 45 minutes
                          - paragraph [ref=e295]: Direct
                        - generic [ref=e296]:
                          - paragraph [ref=e297]: 18:25
                          - paragraph [ref=e298]:
                            - text: SIN
                            - generic [ref=e299]: ", Singapore"
                      - paragraph [ref=e300]:
                        - generic [ref=e301]: Economy
                    - generic [ref=e302]:
                      - generic [ref=e303]:
                        - paragraph [ref=e304]: From
                        - paragraph [ref=e305]: ₱7,690
                        - paragraph [ref=e306]: per adult · indicative
                      - generic [ref=e307]:
                        - link "View fares Manila 14:40 flight" [ref=e308] [cursor=pointer]:
                          - /url: /flights?trip=one-way&from=MNL&to=SIN&depart=2026-12-15&adults=1&cabin=economy&fares=NV503-20261215
                          - text: View fares
                          - generic [ref=e309]: Manila 14:40 flight
                        - button "Details for Manila to Singapore, departing 14:40" [ref=e312]:
                          - text: Details
                          - generic [ref=e313]: for Manila to Singapore, departing 14:40
            - paragraph [ref=e314]: Prices are indicative totals including simulated taxes and charges, in PHP, as of 2026-12-01. They are not offers and are not guaranteed. All times are local.
  - contentinfo [ref=e315]:
    - generic [ref=e317]:
      - generic [ref=e318]:
        - link "Velasia Airways home" [ref=e319] [cursor=pointer]:
          - /url: /
        - paragraph [ref=e327]: Beyond every horizon.
        - paragraph [ref=e328]: A premium full-service airline for the Philippines, Southeast Asia, and beyond, flying from our hubs in Manila and Cebu — in a fictional network built to explore modern airline retailing.
      - generic [ref=e329]:
        - navigation "Book" [ref=e330]:
          - heading "Book" [level=2] [ref=e331]
          - list [ref=e332]:
            - listitem [ref=e333]:
              - link "Search flights" [ref=e334] [cursor=pointer]:
                - /url: /flights
            - listitem [ref=e335]:
              - link "Destinations" [ref=e336] [cursor=pointer]:
                - /url: /destinations
            - listitem [ref=e337]:
              - link "Manage booking" [ref=e338] [cursor=pointer]:
                - /url: /manage-booking
            - listitem [ref=e339]:
              - link "My Trips" [ref=e340] [cursor=pointer]:
                - /url: /account/trips
        - navigation "Help" [ref=e341]:
          - heading "Help" [level=2] [ref=e342]
          - list [ref=e343]:
            - listitem [ref=e344]:
              - link "Help centre" [ref=e345] [cursor=pointer]:
                - /url: /help
            - listitem [ref=e346]:
              - link "Baggage" [ref=e347] [cursor=pointer]:
                - /url: /help#baggage
            - listitem [ref=e348]:
              - link "Travel documents" [ref=e349] [cursor=pointer]:
                - /url: /help#documents
            - listitem [ref=e350]:
              - link "Contact us" [ref=e351] [cursor=pointer]:
                - /url: /help#contact
        - navigation "Velasia" [ref=e352]:
          - heading "Velasia" [level=2] [ref=e353]
          - list [ref=e354]:
            - listitem [ref=e355]:
              - link "About this demo" [ref=e356] [cursor=pointer]:
                - /url: /about
            - listitem [ref=e357]:
              - link "Photo credits" [ref=e358] [cursor=pointer]:
                - /url: /credits
    - generic [ref=e360]:
      - paragraph [ref=e361]: "© 2026 Velasia Airways. A fictional airline: no real reservations, tickets, payments, or financial transactions. All fares shown are illustrative."
      - paragraph [ref=e362]: Velasia Airways is a fictional airline created for demonstration. No real flights, bookings, or payments are offered.
  - alert [ref=e363]: Book flights | Velasia Airways
```

# Test source

```ts
  1   | import AxeBuilder from '@axe-core/playwright';
  2   | import type { Page } from '@playwright/test';
  3   | import {
  4   |   chooseAirport,
  5   |   chooseFare,
  6   |   expect,
  7   |   manilaDate,
  8   |   simulateApiFault,
  9   |   test,
  10  |   trackPageErrors,
  11  |   waitForSearch,
  12  | } from './helpers';
  13  | 
  14  | /*
  15  |  * Milestone 2: flight results from the retailing API. All dates are relative
  16  |  * to the pinned E2E "today" (2026-12-01), so results are deterministic.
  17  |  */
  18  | 
  19  | const ONE_WAY_SIN =
  20  |   '/flights?trip=one-way&from=MNL&to=SIN&depart=2026-12-15&adults=1&cabin=economy';
  21  | const CEB_SIN = '/flights?trip=one-way&from=CEB&to=SIN&depart=2026-12-16&adults=1&cabin=economy';
  22  | const ROUND_HKG =
  23  |   '/flights?trip=round-trip&from=MNL&to=HKG&depart=2026-12-10&return=2026-12-14&adults=2&cabin=economy';
  24  | 
  25  | const cards = (page: Page) => page.getByTestId('flight-card');
  26  | 
  27  | async function prices(page: Page): Promise<number[]> {
  28  |   const texts = await cards(page)
  29  |     .locator('p:has-text("per adult")')
  30  |     .evaluateAll((nodes) => nodes.map((node) => node.previousElementSibling?.textContent ?? ''));
  31  |   return texts.map((text) => Number(text.replace(/[^\d.]/g, '')));
  32  | }
  33  | 
  34  | /** Wait for a URL change made by a filter, then for the refreshed results. */
  35  | async function settled(page: Page) {
  36  |   await expect(page.locator('[data-pending]')).toHaveCount(0);
  37  | }
  38  | 
  39  | test.describe('flight results', () => {
  40  |   test('searches Manila to Singapore and shows real results', async ({ page }) => {
  41  |     const errors = trackPageErrors(page);
  42  |     await page.goto('/');
  43  |     await chooseAirport(page, 'From', 'Manila', /Manila/);
  44  |     await chooseAirport(page, 'To', 'Singapore', /Singapore/);
  45  |     await page.getByText('One-way', { exact: true }).click();
  46  |     await page.getByLabel('Depart', { exact: true }).fill(manilaDate(14));
  47  |     await page.getByRole('button', { name: 'Search flights' }).click();
  48  | 
  49  |     await expect(page).toHaveURL(/\/flights\?trip=one-way&from=MNL&to=SIN&depart=2026-12-15/);
  50  |     const summary = page.getByTestId('search-summary');
  51  |     await expect(summary).toContainText('Ninoy Aquino International Airport');
  52  |     await expect(summary).toContainText('Singapore Changi Airport');
  53  |     await expect(summary).toContainText('One-way');
  54  |     await expect(summary).toContainText('1 Adult');
  55  | 
  56  |     await expect(cards(page).first()).toBeVisible();
  57  |     const first = cards(page).first();
  58  |     await expect(first).toContainText('Velasia');
  59  |     await expect(first).toContainText('VEL 501');
  60  |     await expect(first).not.toContainText('NV 501');
  61  |     await expect(first).toContainText('08:15');
  62  |     await expect(first).toContainText('12:00');
  63  |     await expect(first).toContainText('Direct');
  64  |     await expect(first).toContainText('Airbus A321neo');
  65  |     await expect(first).toContainText(/From₱[\d,]+per adult · indicative/);
> 66  |     expect(errors).toEqual([]);
      |                    ^ Error: expect(received).toEqual(expected) // deep equality
  67  |   });
  68  | 
  69  |   test('changes the departure date from the date strip', async ({ page }) => {
  70  |     await page.goto(ONE_WAY_SIN);
  71  |     const strip = page.getByRole('navigation', { name: 'Nearby dates' });
  72  |     await expect(strip.locator('[aria-current="date"]')).toContainText('Tue Dec 15');
  73  |     await strip.getByRole('link', { name: /^Wed Dec 16 from ₱[\d,]+ per adult/ }).click();
  74  |     await expect(page).toHaveURL(/depart=2026-12-16/);
  75  |     await expect(strip.locator('[aria-current="date"]')).toContainText('Wed Dec 16');
  76  |     await expect(page.getByTestId('search-summary')).toContainText('Wed, Dec 16, 2026');
  77  |     await expect(cards(page).first()).toBeVisible();
  78  |     await expect(page.getByRole('heading', { name: 'Choose your flight' })).toBeVisible();
  79  |   });
  80  | 
  81  |   test('filters to direct flights and sorts by lowest price', async ({ page }) => {
  82  |     await page.goto(CEB_SIN);
  83  |     await expect(cards(page)).toHaveCount(4);
  84  |     await expect(cards(page).filter({ hasText: '1 stop · MNL' })).toHaveCount(3);
  85  | 
  86  |     await page.getByRole('radio', { name: /Direct flights only/ }).check();
  87  |     await expect(page).toHaveURL(/stops=0/);
  88  |     await settled(page);
  89  |     await expect(cards(page)).toHaveCount(1);
  90  |     await expect(cards(page).first()).toContainText('Direct');
  91  |     await expect(page.getByText('1 of 4 flights')).toBeVisible();
  92  | 
  93  |     await page.getByRole('radio', { name: /Any number of stops/ }).check();
  94  |     await expect(page).not.toHaveURL(/stops=/);
  95  |     await expect(cards(page)).toHaveCount(4);
  96  | 
  97  |     await page.getByLabel('Sort by').selectOption('price_asc');
  98  |     await expect(page).toHaveURL(/sort=price_asc/);
  99  |     await settled(page);
  100 |     const sorted = await prices(page);
  101 |     expect(sorted.length).toBe(4);
  102 |     expect(sorted).toEqual([...sorted].sort((a, b) => a - b));
  103 |   });
  104 | 
  105 |   test('opens flight details with itinerary and cabin availability', async ({ page }) => {
  106 |     await page.goto(CEB_SIN);
  107 |     const connecting = cards(page).filter({ hasText: '1 stop · MNL' }).first();
  108 |     await connecting.getByRole('button', { name: /Details/ }).click();
  109 |     const dialog = page.getByRole('dialog', { name: 'Flight details' });
  110 |     await expect(dialog).toBeVisible();
  111 |     await expect(dialog).toContainText('Cebu to Singapore');
  112 |     await expect(dialog).toContainText('Mactan–Cebu International Airport');
  113 |     await expect(dialog).toContainText(/connection in Manila \(MNL\)/);
  114 |     await expect(dialog).toContainText('All times are local to each airport.');
  115 |     await expect(dialog.getByText('Your cabin').first()).toBeVisible(); // loaded via /api/flights/[id]
  116 |     await expect(dialog).toContainText('Passenger service charge');
  117 |     await expect(dialog.getByRole('link', { name: /^View fares/ })).toBeVisible();
  118 |     await page.keyboard.press('Escape');
  119 |     await expect(dialog).toBeHidden();
  120 |   });
  121 | });
  122 | 
  123 | test.describe('round trips', () => {
  124 |   test('selects outbound and return flights, then reviews the total', async ({ page }) => {
  125 |     await page.goto('/');
  126 |     await chooseAirport(page, 'From', 'MNL', /Manila/);
  127 |     await chooseAirport(page, 'To', 'Hong Kong', /Hong Kong/);
  128 |     await page.getByLabel('Depart', { exact: true }).fill('2026-12-10');
  129 |     await page.getByLabel('Return', { exact: true }).fill('2026-12-14');
  130 |     await page.getByRole('button', { name: /Passengers/ }).click();
  131 |     await page.getByRole('button', { name: 'Add one adult' }).click();
  132 |     await page.getByRole('button', { name: 'Done' }).click();
  133 |     await page.getByRole('button', { name: 'Search flights' }).click();
  134 |     await expect(page).toHaveURL(ROUND_HKG);
  135 | 
  136 |     await expect(page.getByRole('heading', { name: 'Choose your departing flight' })).toBeVisible();
  137 |     const outbound = cards(page).filter({ hasText: '13:10' });
  138 |     await expect(outbound).toContainText(/total for 2 adults/);
  139 |     await chooseFare(page, outbound, 'Light');
  140 |     await expect(page).toHaveURL(/out=NV533-20261210/);
  141 | 
  142 |     await expect(page.getByTestId('selected-outbound')).toContainText('13:10');
  143 |     await expect(page.getByTestId('selected-outbound')).toContainText('Velasia Light');
  144 |     await expect(page.getByRole('heading', { name: 'Choose your return flight' })).toBeVisible();
  145 |     await expect(cards(page).first()).toContainText('HKG');
  146 |     await chooseFare(page, cards(page).filter({ hasText: '16:35' }), 'Light');
  147 |     await expect(page).toHaveURL(/out=NV533-20261210&ret=NV534-20261214/);
  148 | 
  149 |     const summary = page.getByTestId('trip-summary');
  150 |     await expect(summary.getByTestId('selection-outbound')).toContainText('Departing');
  151 |     await expect(summary.getByTestId('selection-inbound')).toContainText('Return');
  152 |     const legTotals = await Promise.all(
  153 |       ['outbound', 'inbound'].map(async (direction) =>
  154 |         Number(
  155 |           (await page.getByTestId(`leg-total-${direction}`).textContent())!.replace(/[^\d]/g, ''),
  156 |         ),
  157 |       ),
  158 |     );
  159 |     const total = Number(
  160 |       (await page.getByTestId('trip-total').textContent())!.replace(/[^\d]/g, ''),
  161 |     );
  162 |     expect(total).toBe(legTotals[0]! + legTotals[1]!);
  163 | 
  164 |     await page.getByRole('button', { name: 'Continue to traveller details' }).click();
  165 |     await expect(page).toHaveURL(/\/booking\/travellers$/);
  166 |     await expect(page.getByTestId('traveller-form')).toBeVisible();
```