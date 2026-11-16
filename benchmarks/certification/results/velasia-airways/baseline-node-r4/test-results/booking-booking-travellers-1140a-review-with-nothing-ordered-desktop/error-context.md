# Instructions

- Following Playwright test failed.
- Explain why, be concise, respect Playwright best practices.
- Provide a snippet of code with the fix, if possible.

# Test info

- Name: booking.spec.ts >> booking: travellers, seats, extras, review >> books two travellers from search to a validated review, with nothing ordered
- Location: tests/e2e/booking.spec.ts:39:3

# Error details

```
Error: expect(received).toEqual(expected) // deep equality

- Expected  - 1
+ Received  + 3

- Array []
+ Array [
+   "document-title: html",
+ ]
```

# Page snapshot

```yaml
- generic [ref=e1]:
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
        - paragraph [ref=e44]: Book your trip · Step 3 of 4
        - heading "Travel extras" [level=1] [ref=e45]
        - paragraph [ref=e46]: Everything here is optional. Your fare’s included baggage and services are already counted.
      - generic [ref=e48]:
        - navigation "Booking progress" [ref=e49]:
          - list [ref=e50]:
            - listitem [ref=e51]:
              - link "Travellers, done" [ref=e52] [cursor=pointer]:
                - /url: /booking/travellers
                - generic [aria-hidden] [ref=e56]: Travellers
            - listitem [ref=e59]:
              - link "Seats, done" [ref=e60] [cursor=pointer]:
                - /url: /booking/seats
                - generic [aria-hidden] [ref=e64]: Seats
            - listitem [ref=e67]:
              - generic [ref=e68]:
                - generic [aria-hidden] [ref=e69]: "3"
                - generic [aria-hidden] [ref=e70]: Extras
                - generic [ref=e71]: Extras, current step
            - listitem [ref=e73]:
              - generic [ref=e74]:
                - generic [aria-hidden] [ref=e79]: Review
                - generic [ref=e80]: Review, not available yet
        - generic [ref=e81]:
          - generic [ref=e82]:
            - generic [ref=e83]:
              - region "Checked baggage" [ref=e84]:
                - generic [ref=e90]:
                  - heading "Checked baggage" [level=2] [ref=e91]
                  - paragraph [ref=e92]: Extra weight covers every flight in that direction, including connections, and is charged once.
                - generic [ref=e94]:
                  - heading "Departing · MNL → SIN · Velasia Light" [level=3] [ref=e95]
                  - group "Amihan Dela Cruz" [ref=e96]:
                    - paragraph [ref=e98]: "Included with your fare: no checked bag"
                    - generic [ref=e99]:
                      - generic [ref=e100] [cursor=pointer]:
                        - radio "No extra baggage" [ref=e101]
                        - generic [ref=e102]: No extra baggage
                      - generic [ref=e103] [cursor=pointer]:
                        - radio "Extra 10 kg +₱1,100" [ref=e104]
                        - generic [ref=e105]: Extra 10 kg
                        - generic [ref=e106]: +₱1,100
                      - generic [ref=e107] [cursor=pointer]:
                        - radio "Extra 15 kg +₱1,450" [checked] [ref=e108]
                        - generic [ref=e109]: Extra 15 kg
                        - generic [ref=e110]: +₱1,450
                      - generic [ref=e111] [cursor=pointer]:
                        - radio "Extra 20 kg +₱1,800" [ref=e112]
                        - generic [ref=e113]: Extra 20 kg
                        - generic [ref=e114]: +₱1,800
                    - paragraph [ref=e115]:
                      - text: "Total checked allowance: 15 kg"
                      - generic [ref=e116]: (0 kg included + 15 kg extra)
                    - generic [ref=e117]: Saved
                  - group "Bayani Reyes" [ref=e121]:
                    - paragraph [ref=e123]: "Included with your fare: no checked bag"
                    - generic [ref=e124]:
                      - generic [ref=e125] [cursor=pointer]:
                        - radio "No extra baggage" [checked] [ref=e126]
                        - generic [ref=e127]: No extra baggage
                      - generic [ref=e128] [cursor=pointer]:
                        - radio "Extra 10 kg +₱1,100" [ref=e129]
                        - generic [ref=e130]: Extra 10 kg
                        - generic [ref=e131]: +₱1,100
                      - generic [ref=e132] [cursor=pointer]:
                        - radio "Extra 15 kg +₱1,450" [ref=e133]
                        - generic [ref=e134]: Extra 15 kg
                        - generic [ref=e135]: +₱1,450
                      - generic [ref=e136] [cursor=pointer]:
                        - radio "Extra 20 kg +₱1,800" [ref=e137]
                        - generic [ref=e138]: Extra 20 kg
                        - generic [ref=e139]: +₱1,800
                    - paragraph [ref=e140]: "Total checked allowance: none"
              - region "Meals" [ref=e142]:
                - generic [ref=e150]:
                  - heading "Meals" [level=2] [ref=e151]
                  - paragraph [ref=e152]: Every fare includes a snack and drink. On longer flights you can pre-order a meal.
                - generic [ref=e154]:
                  - generic [ref=e155]:
                    - heading "VEL 501 · MNL → SIN" [level=3] [ref=e156]
                    - paragraph [ref=e157]: A snack and drink are included. You can also pre-order a meal.
                  - group "Amihan Dela Cruz" [ref=e158]:
                    - generic [ref=e160]:
                      - generic [ref=e161] [cursor=pointer]:
                        - radio "No meal pre-order (snack and drink included)" [checked] [ref=e162]
                        - generic [ref=e163]: No meal pre-order (snack and drink included)
                      - generic [ref=e164] [cursor=pointer]:
                        - radio "Hot meal A hot main course with rice or noodles, a side, and a drink. +₱350" [ref=e165]
                        - generic [ref=e166]:
                          - generic [ref=e167]: Hot meal
                          - generic [ref=e168]: A hot main course with rice or noodles, a side, and a drink.
                        - generic [ref=e169]: +₱350
                      - generic [ref=e170] [cursor=pointer]:
                        - radio "Vegetarian meal A hot vegetarian main course (may contain eggs and dairy), a side, and a drink. +₱350" [ref=e171]
                        - generic [ref=e172]:
                          - generic [ref=e173]: Vegetarian meal
                          - generic [ref=e174]: A hot vegetarian main course (may contain eggs and dairy), a side, and a drink.
                        - generic [ref=e175]: +₱350
                      - generic [ref=e176] [cursor=pointer]:
                        - radio "Vegan meal A hot plant-based main course, a side, and a drink. +₱350" [ref=e177]
                        - generic [ref=e178]:
                          - generic [ref=e179]: Vegan meal
                          - generic [ref=e180]: A hot plant-based main course, a side, and a drink.
                        - generic [ref=e181]: +₱350
                      - generic [ref=e182] [cursor=pointer]:
                        - radio "Premium meal A three-course chef’s selection with a dessert and a choice of drinks. +₱750" [ref=e183]
                        - generic [ref=e184]:
                          - generic [ref=e185]: Premium meal
                          - generic [ref=e186]: A three-course chef’s selection with a dessert and a choice of drinks.
                        - generic [ref=e187]: +₱750
                  - group "Bayani Reyes" [ref=e189]:
                    - generic [ref=e191]:
                      - generic [ref=e192] [cursor=pointer]:
                        - radio "No meal pre-order (snack and drink included)" [ref=e193]
                        - generic [ref=e194]: No meal pre-order (snack and drink included)
                      - generic [ref=e195] [cursor=pointer]:
                        - radio "Hot meal A hot main course with rice or noodles, a side, and a drink. +₱350" [ref=e196]
                        - generic [ref=e197]:
                          - generic [ref=e198]: Hot meal
                          - generic [ref=e199]: A hot main course with rice or noodles, a side, and a drink.
                        - generic [ref=e200]: +₱350
                      - generic [ref=e201] [cursor=pointer]:
                        - radio "Vegetarian meal A hot vegetarian main course (may contain eggs and dairy), a side, and a drink. +₱350" [checked] [ref=e202]
                        - generic [ref=e203]:
                          - generic [ref=e204]: Vegetarian meal
                          - generic [ref=e205]: A hot vegetarian main course (may contain eggs and dairy), a side, and a drink.
                        - generic [ref=e206]: +₱350
                      - generic [ref=e207] [cursor=pointer]:
                        - radio "Vegan meal A hot plant-based main course, a side, and a drink. +₱350" [ref=e208]
                        - generic [ref=e209]:
                          - generic [ref=e210]: Vegan meal
                          - generic [ref=e211]: A hot plant-based main course, a side, and a drink.
                        - generic [ref=e212]: +₱350
                      - generic [ref=e213] [cursor=pointer]:
                        - radio "Premium meal A three-course chef’s selection with a dessert and a choice of drinks. +₱750" [ref=e214]
                        - generic [ref=e215]:
                          - generic [ref=e216]: Premium meal
                          - generic [ref=e217]: A three-course chef’s selection with a dessert and a choice of drinks.
                        - generic [ref=e218]: +₱750
                    - generic [ref=e219]: Saved
              - region "Priority services" [ref=e223]:
                - generic [ref=e229]:
                  - heading "Priority services" [level=2] [ref=e230]
                  - paragraph [ref=e231]: Shorter queues at the airport. Fares that include them are never charged again.
                - generic [ref=e233]:
                  - heading "Departing · MNL → SIN" [level=3] [ref=e234]
                  - group "Amihan Dela Cruz" [ref=e235]:
                    - generic [ref=e237]:
                      - generic [ref=e238] [cursor=pointer]:
                        - checkbox "Priority check-in Use the priority check-in counters where available. +₱300" [ref=e239]
                        - generic [ref=e240]:
                          - generic [ref=e241]: Priority check-in
                          - generic [ref=e242]: Use the priority check-in counters where available.
                        - generic [ref=e243]: +₱300
                      - generic [ref=e244] [cursor=pointer]:
                        - checkbox "Priority boarding Board early and settle in before the main cabin. +₱350" [active] [ref=e245]
                        - generic [ref=e246]:
                          - generic [ref=e247]: Priority boarding
                          - generic [ref=e248]: Board early and settle in before the main cabin.
                        - generic [ref=e249]: +₱350
                    - generic [ref=e250]: Saved
                  - group "Bayani Reyes" [ref=e254]:
                    - generic [ref=e256]:
                      - generic [ref=e257] [cursor=pointer]:
                        - checkbox "Priority check-in Use the priority check-in counters where available. +₱300" [ref=e258]
                        - generic [ref=e259]:
                          - generic [ref=e260]: Priority check-in
                          - generic [ref=e261]: Use the priority check-in counters where available.
                        - generic [ref=e262]: +₱300
                      - generic [ref=e263] [cursor=pointer]:
                        - checkbox "Priority boarding Board early and settle in before the main cabin. +₱350" [ref=e264]
                        - generic [ref=e265]:
                          - generic [ref=e266]: Priority boarding
                          - generic [ref=e267]: Board early and settle in before the main cabin.
                        - generic [ref=e268]: +₱350
            - generic [ref=e270]:
              - link "Seats" [ref=e271] [cursor=pointer]:
                - /url: /booking/seats
              - generic [ref=e274]:
                - paragraph [ref=e275]: Next, we’ll check every price and choice again before your review.
                - button "Review your booking" [ref=e277]
          - complementary "MNL → SIN" [ref=e280]:
            - paragraph [ref=e281]: Your trip
            - heading "MNL → SIN" [level=2] [ref=e282]
            - generic [ref=e283]:
              - list [ref=e284]:
                - listitem [ref=e285]:
                  - generic [ref=e286]:
                    - generic [ref=e287]: Departing · MNL → SIN
                    - generic [ref=e288]: Tue, Dec 15, 2026 · 08:15
                    - generic [ref=e289]: VEL 501 · Velasia Light
                  - generic [ref=e290]: ₱15,080
              - generic [ref=e291]:
                - generic [ref=e292]:
                  - term [ref=e293]: Flights, taxes and fees
                  - definition [ref=e294]: ₱15,080
                - generic [ref=e295]:
                  - term [ref=e296]: Seats
                  - definition [ref=e297]: ₱1,300
                - generic [ref=e298]:
                  - term [ref=e299]: Extra baggage
                  - definition [ref=e300]: ₱1,450
                - generic [ref=e301]:
                  - term [ref=e302]: Meals
                  - definition [ref=e303]: ₱350
                - generic [ref=e304]:
                  - term [ref=e305]: Priority services
                  - definition [ref=e306]: None added
              - generic [ref=e307]:
                - paragraph [ref=e308]: Pre-checkout total, 2 adults
                - paragraph [ref=e309]: ₱18,180
                - paragraph [ref=e310]: Includes simulated taxes and fees. Included fare benefits are never charged.
              - paragraph [ref=e311]: "Not a booking: nothing is reserved or charged, and seat choices are provisional until checkout."
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
  - alert [ref=e363]: Travel extras
```

# Test source

```ts
  39  |   test('books two travellers from search to a validated review, with nothing ordered', async ({
  40  |     page,
  41  |   }) => {
  42  |     test.setTimeout(90_000);
  43  |     const errors = trackPageErrors(page);
  44  |     const mutations: string[] = [];
  45  |     page.on('request', (request) => {
  46  |       if (request.method() !== 'GET') mutations.push(request.url());
  47  |     });
  48  | 
  49  |     // 1–3. Search, select an offer, and continue to traveller details.
  50  |     await page.goto(TWO_ADULTS);
  51  |     await page
  52  |       .getByTestId('flight-card')
  53  |       .first()
  54  |       .getByRole('link', { name: /View fares/ })
  55  |       .click();
  56  |     await expect(page.getByTestId('fare-cards')).toBeVisible();
  57  |     await selectFamily(page, 'Light');
  58  |     await page.getByRole('button', { name: 'Continue to traveller details' }).click();
  59  |     await expect(page.getByTestId('traveller-form')).toBeVisible();
  60  |     await expect(page).toHaveTitle(/Traveller details/);
  61  | 
  62  |     // 5. Required fields are validated inline, before anything is sent.
  63  |     await page.getByTestId('save-travellers').click();
  64  |     await expect(page.getByTestId('traveller-form-status')).toContainText('check the highlighted');
  65  |     const given = page.getByTestId('traveller-ADT1').getByLabel('Given names');
  66  |     await expect(given).toHaveAttribute('aria-invalid', 'true');
  67  |     await expect(page.getByText('Enter the given name.').first()).toBeVisible();
  68  |     await page.getByTestId('traveller-ADT1').getByLabel('Date of birth').fill('2020-01-01');
  69  |     await page.getByTestId('traveller-ADT1').getByLabel('Given names').focus();
  70  |     await expect(page.getByText(/Adults must be 12 or older/)).toBeVisible();
  71  | 
  72  |     // 4, 6. Two demo travellers, saved through a Server Action.
  73  |     await fillTravellers(page, 2);
  74  |     await saveTravellers(page);
  75  |     expect(page.url()).not.toMatch(/Amihan|Reyes|example/);
  76  | 
  77  |     // 7–11. The A321neo seat map: seats for both, a change, occupied seats refused, prices.
  78  |     const flightsTotal = await amount(shown(page, 'panel-flights'));
  79  |     await expect(page.getByTestId('seat-legend')).toContainText('Standard · ₱300');
  80  |     const occupied = seat(page, 'occupied');
  81  |     // Occupied seats are announced and reachable, but aria-disabled: open one by keyboard.
  82  |     await occupied.focus();
  83  |     await page.keyboard.press('Enter');
  84  |     await expect(page.getByTestId('seat-details')).toContainText('already taken');
  85  |     await expect(page.getByTestId('choose-seat')).toHaveCount(0);
  86  |     await expect(occupied).toHaveAttribute('aria-disabled', 'true');
  87  |     await expect(occupied).toHaveAttribute('aria-label', /occupied/);
  88  | 
  89  |     const standard = page.locator(
  90  |       '[data-testid="seat-map"] [data-look="available"]:not(:has(svg))',
  91  |     );
  92  |     const first = await chooseSeat(page, standard.nth(3));
  93  |     await expect(page.getByTestId('assigned-seat-ADT1')).toHaveText(`Seat ${first}`);
  94  |     // The next traveller is selected automatically.
  95  |     await expect(page.getByTestId('seat-traveller-ADT2')).toHaveAttribute('aria-pressed', 'true');
  96  |     const second = await chooseSeat(page, standard.nth(6));
  97  |     await expect(page.getByTestId('assigned-seat-ADT2')).toHaveText(`Seat ${second}`);
  98  |     await expect(shown(page, 'booking-total').first()).toHaveText(
  99  |       `₱${(flightsTotal + 600).toLocaleString('en-PH')}`,
  100 |     );
  101 |     // Change Adult 1 to an extra-legroom seat: its charge replaces the old one.
  102 |     await page.getByTestId('seat-traveller-ADT1').click();
  103 |     const legroom = page
  104 |       .locator('[data-testid="seat-map"] [data-look="available"]')
  105 |       .filter({ has: page.locator('.lucide-move-vertical') })
  106 |       .first();
  107 |     await legroom.click();
  108 |     await expect(page.getByTestId('seat-price')).toHaveText('₱1,000 per flight');
  109 |     await page.getByTestId('choose-seat').click();
  110 |     await expect(page.getByTestId('seat-saved')).toBeVisible();
  111 |     await expect(page.getByTestId('assigned-seat-ADT1')).not.toHaveText(`Seat ${first}`);
  112 |     await expect(shown(page, 'panel-seats').first()).toHaveText('₱1,300');
  113 |     expect(await axe(page)).toEqual([]);
  114 |     await page.getByTestId('continue-seats').click();
  115 |     await expect(page).toHaveURL(/\/booking\/extras$/);
  116 |     await extrasReady(page);
  117 | 
  118 |     // 12–15. Baggage, a meal, a priority service (then removed), and what Light includes.
  119 |     await expect(page.getByTestId('baggage-ADT1-outbound')).toContainText('no checked bag');
  120 |     await page.getByTestId('baggage-ADT1-outbound').getByText('Extra 15 kg').click();
  121 |     await expect(page.getByTestId('baggage-total-ADT1-outbound')).toContainText('15 kg');
  122 |     await expect(
  123 |       page.getByTestId('baggage-ADT1-outbound').getByTestId('extra-saved'),
  124 |     ).toBeVisible();
  125 |     await page.getByTestId(`meal-ADT2-${FLIGHT}`).getByText('Vegetarian meal').click();
  126 |     await expect(page.getByTestId(`meal-ADT2-${FLIGHT}`).getByTestId('extra-saved')).toBeVisible();
  127 |     const priority = page.getByTestId('priority-ADT1-outbound').getByRole('checkbox', {
  128 |       name: /Priority boarding/,
  129 |     });
  130 |     await priority.check();
  131 |     await expect(
  132 |       page.getByTestId('priority-ADT1-outbound').getByTestId('extra-saved'),
  133 |     ).toBeVisible();
  134 |     await expect(shown(page, 'panel-priority').first()).toHaveText('₱350');
  135 |     await priority.uncheck();
  136 |     await expect(shown(page, 'panel-priority').first()).toHaveText('None added');
  137 |     await expect(shown(page, 'panel-baggage').first()).toHaveText('₱1,450');
  138 |     await expect(shown(page, 'panel-meals').first()).toHaveText('₱350');
> 139 |     expect(await axe(page)).toEqual([]);
      |                             ^ Error: expect(received).toEqual(expected) // deep equality
  140 | 
  141 |     // 16–17. The review: the itinerary, every line, and a total that reconciles.
  142 |     await page.getByTestId('continue-extras').click();
  143 |     await expect(page).toHaveURL(/\/booking\/review$/);
  144 |     await expect(page.getByTestId('review-ready')).toBeVisible();
  145 |     await expect(page.getByTestId('review-flights')).toContainText('VEL 501');
  146 |     await expect(page.getByTestId('review-travellers')).toContainText('Amihan Dela Cruz');
  147 |     await expect(page.getByTestId('review-seats')).toContainText(second);
  148 |     const total = flightsTotal + 1300 + 1450 + 350;
  149 |     await expect(page.getByTestId('quote-total')).toHaveText(`₱${total.toLocaleString('en-PH')}`);
  150 |     await expect(shown(page, 'booking-total').first()).toHaveText(
  151 |       `₱${total.toLocaleString('en-PH')}`,
  152 |     );
  153 |     const parts = await Promise.all(
  154 |       [
  155 |         'quote-base',
  156 |         'quote-family',
  157 |         'quote-taxes',
  158 |         'quote-seats',
  159 |         'quote-baggage',
  160 |         'quote-meals',
  161 |         'quote-priority',
  162 |       ].map((id) => amount(page.getByTestId(id))),
  163 |     );
  164 |     expect(parts.reduce((sum, part) => sum + part, 0)).toBe(total);
  165 |     await expect(page.getByTestId('continue-checkout')).toBeVisible();
  166 |     expect(await axe(page)).toEqual([]);
  167 | 
  168 |     // 18. Back to seats and change: the review must be checked again.
  169 |     await page.getByRole('link', { name: 'Change seats' }).click();
  170 |     await expect(page).toHaveURL(/\/booking\/seats/);
  171 |     await page.getByTestId('remove-seat-ADT2').click();
  172 |     await expect(page.getByTestId('assigned-seat-ADT2')).toHaveText('No seat yet');
  173 |     await page.getByTestId('continue-seats').click();
  174 |     await page.getByTestId('continue-extras').click();
  175 |     await expect(page.getByTestId('review-ready')).toBeVisible();
  176 |     await expect(page.getByTestId('review-seats')).toContainText('Assigned at check-in');
  177 | 
  178 |     // 19. A refresh restores everything from the session.
  179 |     await page.reload();
  180 |     await expect(page.getByTestId('review-ready')).toBeVisible();
  181 |     await expect(page.getByTestId('quote-total')).toHaveText(
  182 |       `₱${(total - 300).toLocaleString('en-PH')}`,
  183 |     );
  184 | 
  185 |     // 27. Nothing is ordered or paid during the booking steps. (Since M5, "Continue to checkout"
  186 |     //     holds the seats and opens checkout, where payment needs an explicit action; see
  187 |     //     checkout.spec.ts.)
  188 |     expect(mutations.filter((url) => /order|payment|ticket/i.test(url))).toEqual([]);
  189 |     await page.getByTestId('continue-checkout').click();
  190 |     // A Server Action plus the checkout render: allow for a fully parallel run on a laptop.
  191 |     await expect(page).toHaveURL(/\/booking\/checkout$/, { timeout: 20_000 });
  192 |     await expect(page.getByTestId('checkout')).toHaveAttribute(
  193 |       'data-order-status',
  194 |       'PENDING_PAYMENT',
  195 |     );
  196 | 
  197 |     // 26. No hydration or runtime errors anywhere in the flow.
  198 |     expect(errors).toEqual([]);
  199 |   });
  200 | 
  201 |   test('shows fare-family benefits as included and never charges for them', async ({ page }) => {
  202 |     await startBooking(page, ONE_ADULT, 'Flex');
  203 |     await fillTravellers(page, 1);
  204 |     await saveTravellers(page);
  205 |     await expect(page.getByTestId('legend-STANDARD')).toContainText('Included with your fare');
  206 |     await expect(page.getByTestId('legend-PREFERRED')).toContainText('Included with your fare');
  207 |     await expect(page.getByTestId('legend-EXTRA_LEGROOM')).toContainText('₱1,000');
  208 |     const preferred = page
  209 |       .locator('[data-testid="seat-map"] [data-look="available"]')
  210 |       .filter({ has: page.locator('.lucide-star') })
  211 |       .first();
  212 |     await expect(preferred).toHaveAttribute('aria-label', /Preferred.*no additional charge/);
  213 |     await chooseSeat(page, preferred);
  214 |     await expect(shown(page, 'panel-seats').first()).toHaveText('None added');
  215 |     await page.getByTestId('continue-seats').click();
  216 |     await expect(page.getByTestId('priority-included-outbound')).toContainText('Velasia Flex');
  217 |     await expect(page.getByTestId('baggage-ADT1-outbound')).toContainText('1 × 30 kg (30 kg)');
  218 |   });
  219 | 
  220 |   test('refreshes an expired offer at the same price and stays ready', async ({ page }) => {
  221 |     await toReview(page);
  222 |     await reviewReady(page);
  223 |     await simulateScenario(page, 'offer-expired');
  224 |     await page.getByTestId('review-check').click();
  225 |     await expect(shown(page, 'booking-notices')).toContainText('the price is the same');
  226 |     await expect(shown(page, 'review-ready')).toBeVisible();
  227 |     await simulateScenario(page, null);
  228 |   });
  229 | 
  230 |   test('asks before accepting a higher price, then keeps seats and extras', async ({ page }) => {
  231 |     const id = await toReview(page);
  232 |     await reviewReady(page);
  233 |     const before = await amount(page.getByTestId('quote-total'));
  234 |     await simulateScenario(page, 'price-increase');
  235 |     await page.getByTestId('review-check').click();
  236 |     await expect(page.getByTestId('review-action-required')).toBeVisible();
  237 |     await expect(page.locator('[data-code="PRICE_CHANGED"]')).toBeVisible();
  238 |     await expect(page.getByTestId('continue-checkout')).toHaveCount(0);
  239 |     // The accepted total stays until the traveller accepts.
```