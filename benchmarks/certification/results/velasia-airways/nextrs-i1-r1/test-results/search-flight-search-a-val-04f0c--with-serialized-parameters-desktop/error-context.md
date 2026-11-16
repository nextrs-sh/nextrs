# Instructions

- Following Playwright test failed.
- Explain why, be concise, respect Playwright best practices.
- Provide a snippet of code with the fix, if possible.

# Test info

- Name: search.spec.ts >> flight search >> a valid search navigates to /flights with serialized parameters
- Location: tests/e2e/search.spec.ts:58:3

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
+   "http 404: http://127.0.0.1:3200/_next/image?url=%2Fimages%2Fdestinations%2Ftokyo.jpg&w=828&q=70",
+   "console: Failed to load resource: the server responded with a status of 404 (Not Found)",
+   "http 404: http://127.0.0.1:3200/_next/image?url=%2Fimages%2Fdestinations%2Fsingapore.jpg&w=1080&q=70",
+   "console: Failed to load resource: the server responded with a status of 404 (Not Found)",
+   "http 404: http://127.0.0.1:3200/_next/image?url=%2Fimages%2Fdestinations%2Fseoul.jpg&w=828&q=70",
+   "console: Failed to load resource: the server responded with a status of 404 (Not Found)",
+   "http 404: http://127.0.0.1:3200/_next/image?url=%2Fimages%2Fdestinations%2Fbangkok.jpg&w=828&q=70",
+   "console: Failed to load resource: the server responded with a status of 404 (Not Found)",
+   "http 404: http://127.0.0.1:3200/_next/image?url=%2Fimages%2Fdestinations%2Fosaka.jpg&w=828&q=70",
+   "console: Failed to load resource: the server responded with a status of 404 (Not Found)",
+   "http 404: http://127.0.0.1:3200/_next/image?url=%2Fimages%2Fdestinations%2Fcebu.jpg&w=384&q=70",
+   "console: Failed to load resource: the server responded with a status of 404 (Not Found)",
+   "http 404: http://127.0.0.1:3200/_next/image?url=%2Fimages%2Fdestinations%2Fhong-kong.jpg&w=384&q=70",
+   "console: Failed to load resource: the server responded with a status of 404 (Not Found)",
+   "http 404: http://127.0.0.1:3200/_next/image?url=%2Fimages%2Fbrand%2Fwindow.jpg&w=828&q=70",
+   "console: Failed to load resource: the server responded with a status of 404 (Not Found)",
+   "http 404: http://127.0.0.1:3200/_next/image?url=%2Fimages%2Fdestinations%2Ftaipei.jpg&w=384&q=70",
+   "console: Failed to load resource: the server responded with a status of 404 (Not Found)",
+   "http 404: http://127.0.0.1:3200/_next/image?url=%2Fimages%2Fdestinations%2Fsingapore.jpg&w=384&q=70",
+   "console: Failed to load resource: the server responded with a status of 404 (Not Found)",
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
    - generic [ref=e43]:
      - paragraph [ref=e44]: Book flights
      - heading "Flights from Manila to Seoul" [level=1] [ref=e45]
      - region "Your search" [ref=e47]:
        - heading "Your search" [level=2] [ref=e48]
        - generic [ref=e49]:
          - paragraph [ref=e50]:
            - generic [ref=e51]:
              - generic [ref=e52]: MNL
              - generic [ref=e53]: Manila
              - generic [ref=e54]: Ninoy Aquino International Airport
            - img "round trip to" [ref=e55]
            - generic [ref=e58]:
              - generic [ref=e59]: ICN
              - generic [ref=e60]: Seoul
              - generic [ref=e61]: Incheon International Airport
          - generic [ref=e62]:
            - button "Close" [expanded] [active] [ref=e63]
            - generic [ref=e67]:
              - heading "Change your search" [level=2] [ref=e68]
              - search "Search flights" [ref=e69]:
                - generic [ref=e70]:
                  - group "Trip type" [ref=e71]:
                    - generic [ref=e73]:
                      - generic [ref=e74] [cursor=pointer]:
                        - radio "Round-trip" [checked] [ref=e75]
                        - generic [ref=e76]: Round-trip
                      - generic [ref=e77] [cursor=pointer]:
                        - radio "One-way" [ref=e78]
                        - generic [ref=e79]: One-way
                  - 'button "Passengers: 2 Adults, 1 Infant" [ref=e81]': 2 Adults, 1 Infant
                  - generic [ref=e89]:
                    - generic [ref=e90]: Cabin
                    - combobox "Cabin Cabin" [ref=e91] [cursor=pointer]:
                      - option "Economy"
                      - option "Premium Economy" [selected]
                      - option "Business"
                - generic [ref=e92]:
                  - generic [ref=e93]:
                    - generic [ref=e94]:
                      - generic [ref=e100]:
                        - generic [ref=e101]: From
                        - combobox "From From" [ref=e102]: Manila (MNL)
                      - paragraph [ref=e103]
                    - button "Swap departure and arrival airports" [ref=e104]
                    - generic [ref=e108]:
                      - generic [ref=e114]:
                        - generic [ref=e115]: To
                        - combobox "To To" [ref=e116]: Seoul (ICN)
                      - paragraph [ref=e117]
                  - generic [ref=e123]:
                    - generic [ref=e124]: Depart
                    - textbox "Depart Depart" [ref=e125]: 2026-12-22
                  - generic [ref=e131]:
                    - generic [ref=e132]: Return
                    - textbox "Return Return" [ref=e133]: 2026-12-29
                  - button "Search flights" [ref=e134]
                - status [ref=e138]
          - list "Search details" [ref=e139]:
            - listitem [ref=e140]: Round-trip
            - listitem [ref=e145]: Tue, Dec 22, 2026 – Tue, Dec 29, 2026
            - listitem [ref=e149]: 2 Adults, 1 Infant
            - listitem [ref=e156]: Premium Economy
    - generic [ref=e161]:
      - generic [ref=e162]:
        - list "Booking steps" [ref=e163]:
          - listitem [ref=e164]:
            - generic [ref=e165]: "1"
            - generic [ref=e166]: Departing flight
          - listitem [ref=e168]:
            - generic [ref=e169]: "2"
            - generic [ref=e170]: Return flight
          - listitem [ref=e172]:
            - generic [ref=e173]: "3"
            - generic [ref=e174]: Trip summary
        - paragraph [ref=e175]: "Then: travellers · seats · extras · review"
      - generic [ref=e176]:
        - heading "Choose your departing flight" [level=2] [ref=e177]
        - paragraph [ref=e178]: Manila to Seoul · Tue, Dec 22, 2026
      - navigation "Nearby dates" [ref=e179]:
        - list [ref=e180]:
          - listitem [ref=e181]:
            - link "Sat Dec 19 from ₱14,380 per adult , lowest nearby fare" [ref=e182] [cursor=pointer]:
              - /url: /flights?trip=round-trip&from=MNL&to=ICN&depart=2026-12-19&return=2026-12-29&adults=2&infants=1&cabin=premium-economy
              - generic [ref=e183]: Sat Dec 19
              - generic [ref=e184]:
                - generic [ref=e185]: from
                - text: ₱14,380
                - generic [ref=e186]: per adult
              - generic [ref=e187]: ", lowest nearby fare"
          - listitem [ref=e188]:
            - link "Sun Dec 20 from ₱15,400 per adult" [ref=e189] [cursor=pointer]:
              - /url: /flights?trip=round-trip&from=MNL&to=ICN&depart=2026-12-20&return=2026-12-29&adults=2&infants=1&cabin=premium-economy
              - generic [ref=e190]: Sun Dec 20
              - generic [ref=e191]:
                - generic [ref=e192]: from
                - text: ₱15,400
                - generic [ref=e193]: per adult
          - listitem [ref=e194]:
            - link "Mon Dec 21 from ₱15,140 per adult" [ref=e195] [cursor=pointer]:
              - /url: /flights?trip=round-trip&from=MNL&to=ICN&depart=2026-12-21&return=2026-12-29&adults=2&infants=1&cabin=premium-economy
              - generic [ref=e196]: Mon Dec 21
              - generic [ref=e197]:
                - generic [ref=e198]: from
                - text: ₱15,140
                - generic [ref=e199]: per adult
          - listitem [ref=e200]:
            - generic [ref=e201]:
              - generic [ref=e202]: Tue Dec 22
              - generic [ref=e203]:
                - generic [ref=e204]: from
                - text: ₱14,470
                - generic [ref=e205]: per adult
          - listitem [ref=e206]:
            - link "Wed Dec 23 from ₱14,470 per adult" [ref=e207] [cursor=pointer]:
              - /url: /flights?trip=round-trip&from=MNL&to=ICN&depart=2026-12-23&return=2026-12-29&adults=2&infants=1&cabin=premium-economy
              - generic [ref=e208]: Wed Dec 23
              - generic [ref=e209]:
                - generic [ref=e210]: from
                - text: ₱14,470
                - generic [ref=e211]: per adult
          - listitem [ref=e212]:
            - link "Thu Dec 24 from ₱14,380 per adult , lowest nearby fare" [ref=e213] [cursor=pointer]:
              - /url: /flights?trip=round-trip&from=MNL&to=ICN&depart=2026-12-24&return=2026-12-29&adults=2&infants=1&cabin=premium-economy
              - generic [ref=e214]: Thu Dec 24
              - generic [ref=e215]:
                - generic [ref=e216]: from
                - text: ₱14,380
                - generic [ref=e217]: per adult
              - generic [ref=e218]: ", lowest nearby fare"
          - listitem [ref=e219]:
            - link "Fri Dec 25 from ₱15,400 per adult" [ref=e220] [cursor=pointer]:
              - /url: /flights?trip=round-trip&from=MNL&to=ICN&depart=2026-12-25&return=2026-12-29&adults=2&infants=1&cabin=premium-economy
              - generic [ref=e221]: Fri Dec 25
              - generic [ref=e222]:
                - generic [ref=e223]: from
                - text: ₱15,400
                - generic [ref=e224]: per adult
      - generic [ref=e225]:
        - complementary "Filters" [ref=e226]:
          - generic [ref=e227]:
            - heading "Filter flights" [level=3] [ref=e228]
            - generic [ref=e229]:
              - group "Stops" [ref=e230]:
                - generic [ref=e232]:
                  - generic [ref=e233] [cursor=pointer]:
                    - radio "Any number of stops 3 flights" [checked] [ref=e234]
                    - generic [ref=e235]: Any number of stops
                    - generic [ref=e236]:
                      - text: "3"
                      - generic [ref=e237]: flights
                  - generic [ref=e238] [cursor=pointer]:
                    - radio "Direct flights only 2 flights" [ref=e239]
                    - generic [ref=e240]: Direct flights only
                    - generic [ref=e241]:
                      - text: "2"
                      - generic [ref=e242]: flights
              - group "Departure time" [ref=e243]:
                - generic [ref=e245]:
                  - generic [ref=e246]:
                    - checkbox "Early morning 00:00–05:59 0 flights" [disabled] [ref=e247]
                    - generic [ref=e248]:
                      - text: Early morning
                      - generic [ref=e249]: 00:00–05:59
                    - generic [ref=e250]:
                      - text: "0"
                      - generic [ref=e251]: flights
                  - generic [ref=e252] [cursor=pointer]:
                    - checkbox "Morning 06:00–11:59 1 flights" [ref=e253]
                    - generic [ref=e254]:
                      - text: Morning
                      - generic [ref=e255]: 06:00–11:59
                    - generic [ref=e256]:
                      - text: "1"
                      - generic [ref=e257]: flights
                  - generic [ref=e258] [cursor=pointer]:
                    - checkbox "Afternoon 12:00–17:59 1 flights" [ref=e259]
                    - generic [ref=e260]:
                      - text: Afternoon
                      - generic [ref=e261]: 12:00–17:59
                    - generic [ref=e262]:
                      - text: "1"
                      - generic [ref=e263]: flights
                  - generic [ref=e264] [cursor=pointer]:
                    - checkbox "Evening 18:00–23:59 1 flights" [ref=e265]
                    - generic [ref=e266]:
                      - text: Evening
                      - generic [ref=e267]: 18:00–23:59
                    - generic [ref=e268]:
                      - text: "1"
                      - generic [ref=e269]: flights
              - generic [ref=e270]:
                - text: Maximum price
                - paragraph [aria-hidden] [ref=e271]: Any price per adult
                - slider "Maximum price" [ref=e272]: "21400"
                - generic [aria-hidden] [ref=e273]:
                  - generic [ref=e274]: ₱14,400
                  - generic [ref=e275]: ₱21,400
              - group "Aircraft" [ref=e276]:
                - generic [ref=e278]:
                  - generic [ref=e279] [cursor=pointer]:
                    - checkbox "Airbus A320neo 1 flights" [ref=e280]
                    - generic [ref=e281]: Airbus A320neo
                    - generic [ref=e282]:
                      - text: "1"
                      - generic [ref=e283]: flights
                  - generic [ref=e284] [cursor=pointer]:
                    - checkbox "Airbus A321neo 2 flights" [ref=e285]
                    - generic [ref=e286]: Airbus A321neo
                    - generic [ref=e287]:
                      - text: "2"
                      - generic [ref=e288]: flights
                  - generic [ref=e289] [cursor=pointer]:
                    - checkbox "Airbus A330-900 1 flights" [ref=e290]
                    - generic [ref=e291]: Airbus A330-900
                    - generic [ref=e292]:
                      - text: "1"
                      - generic [ref=e293]: flights
        - generic [ref=e294]:
          - generic [ref=e295]:
            - paragraph [ref=e296]: 3 flights
            - generic [ref=e297]:
              - generic [ref=e298]: Sort by
              - combobox "Sort by" [ref=e300] [cursor=pointer]:
                - option "Earliest departure" [selected]
                - option "Latest departure"
                - option "Lowest price"
                - option "Shortest duration"
          - generic [ref=e301]:
            - status [ref=e302]
            - list "Flights" [ref=e303]:
              - listitem [ref=e304]:
                - article [ref=e305]:
                  - heading "Manila to Seoul, departs 07:50, arrives 12:40" [level=3] [ref=e306]
                  - generic [ref=e307]:
                    - generic [ref=e308]:
                      - generic [ref=e309]:
                        - generic [ref=e310]:
                          - generic [ref=e315]: Velasia
                          - generic [ref=e316]: VEL 581
                        - paragraph [ref=e317]: Airbus A330-900
                      - generic [ref=e319]:
                        - generic [ref=e320]:
                          - paragraph [ref=e321]: 07:50
                          - paragraph [ref=e322]:
                            - text: MNL
                            - generic [ref=e323]: ", Manila"
                        - generic [ref=e324]:
                          - paragraph [ref=e325]:
                            - text: 3h 50m
                            - generic [ref=e326]: Total journey 3 hours 50 minutes
                          - paragraph [ref=e332]: Direct
                        - generic [ref=e333]:
                          - paragraph [ref=e334]: 12:40
                          - paragraph [ref=e335]:
                            - text: ICN
                            - generic [ref=e336]: ", Seoul"
                      - paragraph [ref=e337]:
                        - generic [ref=e338]: Premium Economy
                        - generic [ref=e339]: 9 seats left
                    - generic [ref=e340]:
                      - generic [ref=e341]:
                        - paragraph [ref=e342]: From
                        - paragraph [ref=e343]: ₱14,470
                        - paragraph [ref=e344]: per adult · indicative
                        - paragraph [ref=e345]:
                          - text: ₱30,220
                          - generic [ref=e346]: total for 2 adults, 1 infant
                      - generic [ref=e347]:
                        - link "View fares Manila 07:50 flight" [ref=e348] [cursor=pointer]:
                          - /url: /flights?trip=round-trip&from=MNL&to=ICN&depart=2026-12-22&return=2026-12-29&adults=2&infants=1&cabin=premium-economy&fares=NV581-20261222
                          - text: View fares
                          - generic [ref=e349]: Manila 07:50 flight
                        - button "Details for Manila to Seoul, departing 07:50" [ref=e352]:
                          - text: Details
                          - generic [ref=e353]: for Manila to Seoul, departing 07:50
              - listitem [ref=e354]:
                - article [ref=e355]:
                  - heading "Manila to Seoul, departs 17:45, arrives 04:35" [level=3] [ref=e356]
                  - generic [ref=e357]:
                    - generic [ref=e358]:
                      - generic [ref=e359]:
                        - generic [ref=e360]:
                          - generic [ref=e365]: Velasia
                          - generic [ref=e366]: VEL 107 · VEL 721
                        - paragraph [ref=e367]: Airbus A320neo + Airbus A321neo
                      - generic [ref=e369]:
                        - generic [ref=e370]:
                          - paragraph [ref=e371]: 17:45
                          - paragraph [ref=e372]:
                            - text: MNL
                            - generic [ref=e373]: ", Manila"
                        - generic [ref=e374]:
                          - paragraph [ref=e375]:
                            - text: 9h 50m
                            - generic [ref=e376]: Total journey 9 hours 50 minutes
                          - paragraph [ref=e384]: 1 stop · CEB
                        - generic [ref=e385]:
                          - paragraph [ref=e386]:
                            - text: 04:35
                            - superscript [ref=e387]:
                              - text: "+1"
                              - generic [ref=e388]: ", arrives Wed, Dec 23, 2026"
                          - paragraph [ref=e389]:
                            - text: ICN
                            - generic [ref=e390]: ", Seoul"
                      - paragraph [ref=e391]:
                        - generic [ref=e392]: Premium Economy
                        - generic [ref=e393]: 4 seats left
                    - generic [ref=e394]:
                      - generic [ref=e395]:
                        - paragraph [ref=e396]: From
                        - paragraph [ref=e397]: ₱21,380
                        - paragraph [ref=e398]: per adult · indicative
                        - paragraph [ref=e399]:
                          - text: ₱44,640
                          - generic [ref=e400]: total for 2 adults, 1 infant
                      - generic [ref=e401]:
                        - link "View fares Manila 17:45 flight" [ref=e402] [cursor=pointer]:
                          - /url: /flights?trip=round-trip&from=MNL&to=ICN&depart=2026-12-22&return=2026-12-29&adults=2&infants=1&cabin=premium-economy&fares=NV107-20261222_NV721-20261222
                          - text: View fares
                          - generic [ref=e403]: Manila 17:45 flight
                        - button "Details for Manila to Seoul, departing 17:45" [ref=e406]:
                          - text: Details
                          - generic [ref=e407]: for Manila to Seoul, departing 17:45
              - listitem [ref=e408]:
                - article [ref=e409]:
                  - heading "Manila to Seoul, departs 22:30, arrives 03:20" [level=3] [ref=e410]
                  - generic [ref=e411]:
                    - generic [ref=e412]:
                      - generic [ref=e413]:
                        - generic [ref=e414]:
                          - generic [ref=e419]: Velasia
                          - generic [ref=e420]: VEL 583
                        - paragraph [ref=e421]: Airbus A321neo
                      - generic [ref=e423]:
                        - generic [ref=e424]:
                          - paragraph [ref=e425]: 22:30
                          - paragraph [ref=e426]:
                            - text: MNL
                            - generic [ref=e427]: ", Manila"
                        - generic [ref=e428]:
                          - paragraph [ref=e429]:
                            - text: 3h 50m
                            - generic [ref=e430]: Total journey 3 hours 50 minutes
                          - paragraph [ref=e436]: Direct
                        - generic [ref=e437]:
                          - paragraph [ref=e438]:
                            - text: 03:20
                            - superscript [ref=e439]:
                              - text: "+1"
                              - generic [ref=e440]: ", arrives Wed, Dec 23, 2026"
                          - paragraph [ref=e441]:
                            - text: ICN
                            - generic [ref=e442]: ", Seoul"
                      - paragraph [ref=e443]:
                        - generic [ref=e444]: Premium Economy
                        - generic [ref=e445]: 7 seats left
                    - generic [ref=e446]:
                      - generic [ref=e447]:
                        - paragraph [ref=e448]: From
                        - paragraph [ref=e449]: ₱15,190
                        - paragraph [ref=e450]: per adult · indicative
                        - paragraph [ref=e451]:
                          - text: ₱31,730
                          - generic [ref=e452]: total for 2 adults, 1 infant
                      - generic [ref=e453]:
                        - link "View fares Manila 22:30 flight" [ref=e454] [cursor=pointer]:
                          - /url: /flights?trip=round-trip&from=MNL&to=ICN&depart=2026-12-22&return=2026-12-29&adults=2&infants=1&cabin=premium-economy&fares=NV583-20261222
                          - text: View fares
                          - generic [ref=e455]: Manila 22:30 flight
                        - button "Details for Manila to Seoul, departing 22:30" [ref=e458]:
                          - text: Details
                          - generic [ref=e459]: for Manila to Seoul, departing 22:30
            - paragraph [ref=e460]: Prices are indicative totals including simulated taxes and charges, in PHP, as of 2026-12-01. They are not offers and are not guaranteed. All times are local.
  - contentinfo [ref=e461]:
    - generic [ref=e463]:
      - generic [ref=e464]:
        - link "Velasia Airways home" [ref=e465] [cursor=pointer]:
          - /url: /
        - paragraph [ref=e473]: Beyond every horizon.
        - paragraph [ref=e474]: A premium full-service airline for the Philippines, Southeast Asia, and beyond, flying from our hubs in Manila and Cebu — in a fictional network built to explore modern airline retailing.
      - generic [ref=e475]:
        - navigation "Book" [ref=e476]:
          - heading "Book" [level=2] [ref=e477]
          - list [ref=e478]:
            - listitem [ref=e479]:
              - link "Search flights" [ref=e480] [cursor=pointer]:
                - /url: /flights
            - listitem [ref=e481]:
              - link "Destinations" [ref=e482] [cursor=pointer]:
                - /url: /destinations
            - listitem [ref=e483]:
              - link "Manage booking" [ref=e484] [cursor=pointer]:
                - /url: /manage-booking
            - listitem [ref=e485]:
              - link "My Trips" [ref=e486] [cursor=pointer]:
                - /url: /account/trips
        - navigation "Help" [ref=e487]:
          - heading "Help" [level=2] [ref=e488]
          - list [ref=e489]:
            - listitem [ref=e490]:
              - link "Help centre" [ref=e491] [cursor=pointer]:
                - /url: /help
            - listitem [ref=e492]:
              - link "Baggage" [ref=e493] [cursor=pointer]:
                - /url: /help#baggage
            - listitem [ref=e494]:
              - link "Travel documents" [ref=e495] [cursor=pointer]:
                - /url: /help#documents
            - listitem [ref=e496]:
              - link "Contact us" [ref=e497] [cursor=pointer]:
                - /url: /help#contact
        - navigation "Velasia" [ref=e498]:
          - heading "Velasia" [level=2] [ref=e499]
          - list [ref=e500]:
            - listitem [ref=e501]:
              - link "About this demo" [ref=e502] [cursor=pointer]:
                - /url: /about
            - listitem [ref=e503]:
              - link "Photo credits" [ref=e504] [cursor=pointer]:
                - /url: /credits
    - generic [ref=e506]:
      - paragraph [ref=e507]: "© 2026 Velasia Airways. A fictional airline: no real reservations, tickets, payments, or financial transactions. All fares shown are illustrative."
      - paragraph [ref=e508]: Velasia Airways is a fictional airline created for demonstration. No real flights, bookings, or payments are offered.
  - alert [ref=e509]: Book flights | Velasia Airways
```

# Test source

```ts
  1   | import { chooseAirport, manilaDate, trackPageErrors, waitForSearch, expect, test } from './helpers';
  2   | 
  3   | test.describe('flight search', () => {
  4   |   test('airport autocomplete filters and supports the keyboard', async ({ page }) => {
  5   |     await page.goto('/');
  6   |     await waitForSearch(page);
  7   |     const from = page.getByRole('combobox', { name: 'From' });
  8   |     const options = page.getByRole('listbox', { name: 'From airports' }).getByRole('option');
  9   |     await from.click();
  10  |     await expect(options.first()).toBeVisible();
  11  |     await expect(options.first()).toContainText('Cebu'); // hubs first
  12  | 
  13  |     await from.fill('osa');
  14  |     await expect(options).toHaveCount(1);
  15  |     await expect(options).toContainText('Kansai International Airport');
  16  | 
  17  |     await from.fill('tokyo');
  18  |     await expect(options).toHaveCount(2);
  19  |     await page.keyboard.press('ArrowDown');
  20  |     await page.keyboard.press('Enter');
  21  |     await expect(from).toHaveValue('Tokyo (NRT)');
  22  |     // Focus moves on to the arrival airport.
  23  |     await expect(page.getByRole('combobox', { name: 'To' })).toBeFocused();
  24  |   });
  25  | 
  26  |   test('rejects an incomplete search with accessible messages', async ({ page }) => {
  27  |     await page.goto('/');
  28  |     await waitForSearch(page);
  29  |     await page.getByRole('button', { name: 'Search flights' }).click();
  30  | 
  31  |     await expect(page.getByText('Choose where you are flying from.')).toBeVisible();
  32  |     await expect(page.getByText('Choose where you are flying to.')).toBeVisible();
  33  |     await expect(page.getByText('Choose a departure date.')).toBeVisible();
  34  |     await expect(page.getByText('Choose a return date.')).toBeVisible();
  35  |     await expect(page.getByRole('combobox', { name: 'From' })).toHaveAttribute(
  36  |       'aria-invalid',
  37  |       'true',
  38  |     );
  39  |     await expect(page.getByRole('combobox', { name: 'From' })).toBeFocused();
  40  |     // Focusing the field must not open suggestions over its error message.
  41  |     await expect(page.getByRole('listbox', { name: 'From airports' })).toBeHidden();
  42  |     await expect(page).toHaveURL(/\/$/);
  43  |   });
  44  | 
  45  |   test('rejects returning before departing', async ({ page }) => {
  46  |     await page.goto('/');
  47  |     await chooseAirport(page, 'From', 'MNL', /Manila/);
  48  |     await chooseAirport(page, 'To', 'CEB', /Cebu/);
  49  |     await page.getByLabel('Depart', { exact: true }).fill(manilaDate(20));
  50  |     await page.getByLabel('Return', { exact: true }).fill(manilaDate(10));
  51  |     await page.getByRole('button', { name: 'Search flights' }).click();
  52  |     await expect(
  53  |       page.getByText('Return date must be on or after the departure date.'),
  54  |     ).toBeVisible();
  55  |     await expect(page).toHaveURL(/\/$/);
  56  |   });
  57  | 
  58  |   test('a valid search navigates to /flights with serialized parameters', async ({ page }) => {
  59  |     const errors = trackPageErrors(page);
  60  |     const depart = manilaDate(21);
  61  |     const back = manilaDate(28);
  62  |     await page.goto('/');
  63  |     await waitForSearch(page);
  64  | 
  65  |     await chooseAirport(page, 'From', 'manila', /Manila/);
  66  |     await chooseAirport(page, 'To', 'seoul', /Seoul/);
  67  |     await page.getByLabel('Depart', { exact: true }).fill(depart);
  68  |     await page.getByLabel('Return', { exact: true }).fill(back);
  69  | 
  70  |     await page.getByRole('button', { name: /Passengers/ }).click();
  71  |     await page.getByRole('button', { name: 'Add one adult' }).click();
  72  |     await page.getByRole('button', { name: 'Add one infant' }).click();
  73  |     await page.getByRole('button', { name: 'Done' }).click();
  74  |     await page.getByLabel('Cabin', { exact: true }).selectOption('premium-economy');
  75  | 
  76  |     await page.getByRole('button', { name: 'Search flights' }).click();
  77  |     await expect(page).toHaveURL(
  78  |       `/flights?trip=round-trip&from=MNL&to=ICN&depart=${depart}&return=${back}&adults=2&infants=1&cabin=premium-economy`,
  79  |     );
  80  | 
  81  |     const summary = page.getByTestId('search-summary');
  82  |     await expect(summary).toContainText('MNL');
  83  |     await expect(summary).toContainText('Seoul');
  84  |     await expect(summary).toContainText('2 Adults, 1 Infant');
  85  |     await expect(summary).toContainText('Premium Economy');
  86  |     await expect(page.getByTestId('flight-card').first()).toBeVisible();
  87  | 
  88  |     // The editable search keeps the submitted values.
  89  |     await page.getByRole('button', { name: 'Edit search' }).click();
  90  |     await expect(page.getByRole('combobox', { name: 'From' })).toHaveValue('Manila (MNL)');
  91  |     await expect(page.getByRole('combobox', { name: 'To' })).toHaveValue('Seoul (ICN)');
  92  |     await expect(page.getByLabel('Depart', { exact: true })).toHaveValue(depart);
> 93  |     expect(errors).toEqual([]);
      |                    ^ Error: expect(received).toEqual(expected) // deep equality
  94  |   });
  95  | 
  96  |   test('one-way trips hide the return date and swap works', async ({ page }) => {
  97  |     await page.goto('/');
  98  |     await waitForSearch(page);
  99  |     await page.getByText('One-way', { exact: true }).click();
  100 |     await expect(page.getByLabel('Return', { exact: true })).toHaveCount(0);
  101 |     await expect(page.getByRole('button', { name: /Add a return flight/ })).toBeVisible();
  102 | 
  103 |     await chooseAirport(page, 'From', 'CEB', /Cebu/);
  104 |     await chooseAirport(page, 'To', 'Taipei', /Taipei/);
  105 |     await page.getByRole('button', { name: 'Swap departure and arrival airports' }).click();
  106 |     await expect(page.getByRole('combobox', { name: 'From' })).toHaveValue('Taipei (TPE)');
  107 |     await expect(page.getByRole('combobox', { name: 'To' })).toHaveValue('Cebu (CEB)');
  108 | 
  109 |     await page.getByLabel('Depart', { exact: true }).fill(manilaDate(5));
  110 |     await page.getByRole('button', { name: 'Search flights' }).click();
  111 |     await expect(page).toHaveURL(
  112 |       /\/flights\?trip=one-way&from=TPE&to=CEB&depart=\d{4}-\d{2}-\d{2}&adults=1&cabin=economy$/,
  113 |     );
  114 |   });
  115 | 
  116 |   test('selections are preserved while navigating', async ({ page }) => {
  117 |     await page.goto('/');
  118 |     await chooseAirport(page, 'From', 'Davao', /Davao/);
  119 |     await page
  120 |       .getByRole('navigation', { name: 'Primary' })
  121 |       .getByRole('link', { name: 'Explore destinations' })
  122 |       .click();
  123 |     await expect(page).toHaveURL(/\/destinations$/);
  124 |     await page.getByRole('link', { name: 'Velasia Airways home' }).first().click();
  125 |     await expect(page.getByRole('combobox', { name: 'From' })).toHaveValue('Davao (DVO)');
  126 |   });
  127 | 
  128 |   test('invalid URL parameters are validated and explained', async ({ page }) => {
  129 |     await page.goto('/flights?trip=round-trip&from=MNL&to=MNL&depart=2001-01-01&adults=0');
  130 |     await waitForSearch(page);
  131 |     const alert = page.getByRole('alert').filter({ hasText: 'We couldn’t run this search' });
  132 |     await expect(alert).toBeVisible();
  133 |     await expect(alert).toContainText('Departure and arrival airports must be different.');
  134 |     await expect(alert).toContainText('Choose a date from today onwards.');
  135 |     await expect(alert).toContainText('At least one adult must travel on every booking.');
  136 |     await expect(page.getByTestId('flight-card')).toHaveCount(0);
  137 |     await expect(page.getByRole('combobox', { name: 'From' })).toHaveValue('Manila (MNL)');
  138 |   });
  139 | 
  140 |   test('date inputs restrict the bookable window', async ({ page }) => {
  141 |     await page.goto('/');
  142 |     await waitForSearch(page);
  143 |     const depart = page.getByLabel('Depart', { exact: true });
  144 |     await expect(depart).toHaveAttribute('min', manilaDate(0));
  145 |     await expect(depart).toHaveAttribute('max', /\d{4}-\d{2}-\d{2}/);
  146 |     await depart.fill(manilaDate(30));
  147 |     await expect(page.getByLabel('Return', { exact: true })).toHaveAttribute('min', manilaDate(30));
  148 |   });
  149 | 
  150 |   test('works on a phone @mobile', async ({ page, isMobile }) => {
  151 |     test.skip(!isMobile, 'mobile only');
  152 |     await page.goto('/');
  153 |     await chooseAirport(page, 'From', 'MNL', /Manila/);
  154 |     await chooseAirport(page, 'To', 'BKK', /Bangkok/);
  155 |     await page.getByText('One-way', { exact: true }).click();
  156 |     await page.getByLabel('Depart', { exact: true }).fill(manilaDate(9));
  157 |     await page.getByRole('button', { name: 'Search flights' }).click();
  158 |     await expect(page).toHaveURL(/\/flights\?trip=one-way&from=MNL&to=BKK/);
  159 |   });
  160 | });
  161 | 
  162 | test.describe('mobile layout', () => {
  163 |   test('no page overflows horizontally @mobile', async ({ page, isMobile }) => {
  164 |     test.skip(!isMobile, 'mobile only');
  165 |     for (const path of [
  166 |       '/',
  167 |       '/destinations',
  168 |       '/destinations/ho-chi-minh-city',
  169 |       '/flights',
  170 |       '/help',
  171 |     ]) {
  172 |       await page.goto(path);
  173 |       const overflow = await page.evaluate(
  174 |         () => document.documentElement.scrollWidth - window.innerWidth,
  175 |       );
  176 |       expect(overflow, path).toBeLessThanOrEqual(0);
  177 |     }
  178 |   });
  179 | });
  180 | 
```