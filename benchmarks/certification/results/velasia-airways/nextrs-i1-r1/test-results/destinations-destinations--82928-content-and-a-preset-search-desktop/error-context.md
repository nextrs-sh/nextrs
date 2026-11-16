# Instructions

- Following Playwright test failed.
- Explain why, be concise, respect Playwright best practices.
- Provide a snippet of code with the fix, if possible.

# Test info

- Name: destinations.spec.ts >> destinations >> a destination page shows guide content and a preset search
- Location: tests/e2e/destinations.spec.ts:36:3

# Error details

```
Error: expect(received).toEqual(expected) // deep equality

- Expected  -  1
+ Received  + 18

- Array []
+ Array [
+   "http://127.0.0.1:3200/_next/image?url=%2Fimages%2Fdestinations%2Ftokyo.jpg&w=1600&q=80",
+   "http://127.0.0.1:3200/_next/image?url=%2Fimages%2Fdestinations%2Fseoul.jpg&w=828&q=70",
+   "http://127.0.0.1:3200/_next/image?url=%2Fimages%2Fdestinations%2Fhong-kong.jpg&w=828&q=70",
+   "http://127.0.0.1:3200/_next/image?url=%2Fimages%2Fdestinations%2Ftaipei.jpg&w=828&q=70",
+   "http://127.0.0.1:3200/_next/image?url=%2Fimages%2Fdestinations%2Fmanila.jpg&w=828&q=70",
+   "http://127.0.0.1:3200/_next/image?url=%2Fimages%2Fdestinations%2Fcebu.jpg&w=828&q=70",
+   "http://127.0.0.1:3200/_next/image?url=%2Fimages%2Fdestinations%2Fdavao.jpg&w=828&q=70",
+   "http://127.0.0.1:3200/_next/image?url=%2Fimages%2Fdestinations%2Fsingapore.jpg&w=828&q=70",
+   "http://127.0.0.1:3200/_next/image?url=%2Fimages%2Fdestinations%2Ftokyo.jpg&w=828&q=70",
+   "http://127.0.0.1:3200/_next/image?url=%2Fimages%2Fdestinations%2Fseoul.jpg&w=828&q=70",
+   "http://127.0.0.1:3200/_next/image?url=%2Fimages%2Fdestinations%2Fbangkok.jpg&w=828&q=70",
+   "http://127.0.0.1:3200/_next/image?url=%2Fimages%2Fdestinations%2Fhong-kong.jpg&w=828&q=70",
+   "http://127.0.0.1:3200/_next/image?url=%2Fimages%2Fdestinations%2Fkuala-lumpur.jpg&w=828&q=70",
+   "http://127.0.0.1:3200/_next/image?url=%2Fimages%2Fdestinations%2Ftaipei.jpg&w=828&q=70",
+   "http://127.0.0.1:3200/_next/image?url=%2Fimages%2Fdestinations%2Fosaka.jpg&w=828&q=70",
+   "http://127.0.0.1:3200/_next/image?url=%2Fimages%2Fdestinations%2Fho-chi-minh-city.jpg&w=828&q=70",
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
    - article [ref=e41]:
      - generic [ref=e42]:
        - img "Tokyo skyline at golden hour, with the red-and-white Tokyo Tower rising among dense high-rises in Minato City" [ref=e43]
        - generic [ref=e45]:
          - navigation "Breadcrumb" [ref=e46]:
            - list [ref=e47]:
              - listitem [ref=e48]:
                - link "Destinations" [ref=e49] [cursor=pointer]:
                  - /url: /destinations
              - listitem [aria-hidden] [ref=e50]
              - listitem [ref=e53]: Tokyo
          - paragraph [ref=e54]: Japan · East Asia
          - heading "Tokyo" [level=1] [ref=e55]
          - paragraph [ref=e56]: Neon nights, quiet shrines, and perfect ramen in between.
          - list "Trip styles" [ref=e57]:
            - listitem [ref=e58]:
              - generic [ref=e59]: City
            - listitem [ref=e60]:
              - generic [ref=e61]: Food
            - listitem [ref=e62]:
              - generic [ref=e63]: Culture
      - generic [ref=e64]:
        - generic [ref=e65]:
          - paragraph [ref=e66]: The world’s largest city is also one of its most orderly — a place where centuries-old temples sit beside the bright lights of Shibuya.
          - generic [ref=e67]:
            - paragraph [ref=e68]: Tokyo is best understood one neighbourhood at a time. Wander the incense-scented lanes around Senso-ji in Asakusa, browse vintage stores in Shimokitazawa, then lose track of time beneath the lights of Shinjuku.
            - paragraph [ref=e69]: Spring cherry blossoms and autumn foliage draw the crowds, but winter brings crisp, clear days with views of Mount Fuji from the city’s observation decks.
          - heading "Highlights" [level=2] [ref=e70]
          - list [ref=e71]:
            - listitem [ref=e72]:
              - generic [ref=e73]: "01"
              - heading "Asakusa and Senso-ji" [level=3] [ref=e74]
              - paragraph [ref=e75]: Tokyo’s oldest temple, approached through the bustling Nakamise shopping street.
            - listitem [ref=e76]:
              - generic [ref=e77]: "02"
              - heading "Shibuya Crossing" [level=3] [ref=e78]
              - paragraph [ref=e79]: Watch thousands cross at once from above, then dive into the side streets for izakaya.
            - listitem [ref=e80]:
              - generic [ref=e81]: "03"
              - heading "Day trip to Hakone" [level=3] [ref=e82]
              - paragraph [ref=e83]: Hot springs, a lake cruise, and — on a clear day — Mount Fuji.
        - complementary "Tokyo travel facts" [ref=e84]:
          - generic [ref=e85]:
            - heading "Good to know" [level=2] [ref=e86]
            - generic [ref=e87]:
              - generic [ref=e88]:
                - term [ref=e89]: Local time
                - definition [ref=e93]: UTC+9 (Asia/Tokyo)
              - generic [ref=e94]:
                - term [ref=e95]: Currency
                - definition [ref=e101]: Japanese yen (JPY)
              - generic [ref=e102]:
                - term [ref=e103]: Language
                - definition [ref=e108]: Japanese
              - generic [ref=e109]:
                - term [ref=e110]: Best time to visit
                - definition [ref=e117]: March to May and October to November
            - paragraph [ref=e119]:
              - generic [ref=e120]: One-way from
              - generic [ref=e121]: ₱7,160*
              - generic [ref=e122]:
                - generic [ref=e123]: "Note:"
                - text: Illustrative fare · not a live offer
          - generic [ref=e124]:
            - heading "Getting there" [level=2] [ref=e125]
            - paragraph [ref=e126]: Planned routes in Velasia’s fictional network.
            - list [ref=e127]:
              - listitem [ref=e128]:
                - generic [ref=e132]:
                  - paragraph [ref=e133]:
                    - text: Manila (MNL) →
                    - generic [ref=e134]: to
                    - text: Tokyo (NRT)
                  - paragraph [ref=e135]: About 4h 20m · 15× weekly
              - listitem [ref=e136]:
                - generic [ref=e140]:
                  - paragraph [ref=e141]:
                    - text: Manila (MNL) →
                    - generic [ref=e142]: to
                    - text: Tokyo (HND)
                  - paragraph [ref=e143]: About 4h 15m · Daily
              - listitem [ref=e144]:
                - generic [ref=e148]:
                  - paragraph [ref=e149]:
                    - text: Cebu (CEB) →
                    - generic [ref=e150]: to
                    - text: Tokyo (NRT)
                  - paragraph [ref=e151]: About 4h 40m · 4× weekly
      - region "Find flights to Tokyo" [ref=e152]:
        - generic [ref=e153]:
          - generic [ref=e155]:
            - paragraph [ref=e156]: Plan your trip
            - heading "Find flights to Tokyo" [level=2] [ref=e157]
            - paragraph [ref=e158]: Choose your dates and who’s travelling. We’ll remember your selections while you browse.
          - search "Search flights" [ref=e160]:
            - generic [ref=e161]:
              - group "Trip type" [ref=e162]:
                - generic [ref=e164]:
                  - generic [ref=e165] [cursor=pointer]:
                    - radio "Round-trip" [checked] [ref=e166]
                    - generic [ref=e167]: Round-trip
                  - generic [ref=e168] [cursor=pointer]:
                    - radio "One-way" [ref=e169]
                    - generic [ref=e170]: One-way
              - 'button "Passengers: 1 Adult" [ref=e172]': 1 Adult
              - generic [ref=e180]:
                - generic [ref=e181]: Cabin
                - combobox "Cabin" [ref=e182] [cursor=pointer]:
                  - option "Economy" [selected]
                  - option "Premium Economy"
                  - option "Business"
            - generic [ref=e183]:
              - generic [ref=e184]:
                - generic [ref=e185]:
                  - generic [ref=e191]:
                    - generic [ref=e192]: From
                    - combobox "From" [ref=e193]
                  - paragraph [ref=e194]
                - button "Swap departure and arrival airports" [ref=e195]
                - generic [ref=e199]:
                  - generic [ref=e205]:
                    - generic [ref=e206]: To
                    - combobox "To" [ref=e207]: Tokyo (NRT)
                  - paragraph [ref=e208]
              - generic [ref=e214]:
                - generic [ref=e215]: Depart
                - textbox "Depart" [ref=e216]
              - generic [ref=e222]:
                - generic [ref=e223]: Return
                - textbox "Return" [ref=e224]
              - button "Search flights" [ref=e225]
            - status [ref=e229]
      - region "You might also like" [ref=e230]:
        - generic [ref=e231]:
          - heading "You might also like" [level=2] [ref=e234]
          - list [ref=e235]:
            - listitem [ref=e236]:
              - link "Gyeonghoeru, a large two-storey wooden pavilion on stone pillars at Gyeongbokgung Palace in Seoul, reflected in its pond under a clear autumn sky South Korea Seoul Palaces by day, barbecue and karaoke by night. From ₱6,740* (illustrative fare)" [ref=e237] [cursor=pointer]:
                - /url: /destinations/seoul
                - img "Gyeonghoeru, a large two-storey wooden pavilion on stone pillars at Gyeongbokgung Palace in Seoul, reflected in its pond under a clear autumn sky" [ref=e238]
                - generic [ref=e241]:
                  - paragraph [ref=e242]: South Korea
                  - heading "Seoul" [level=3] [ref=e243]
                  - paragraph [ref=e244]: Palaces by day, barbecue and karaoke by night.
                  - paragraph [ref=e245]:
                    - text: From
                    - generic [ref=e246]: ₱6,740*
                    - generic [ref=e247]: (illustrative fare)
            - listitem [ref=e252]:
              - link "Night panorama from Victoria Peak over Hong Kong’s illuminated skyscrapers and Victoria Harbour, with Kowloon across the water Hong Kong SAR Hong Kong A harbour skyline, hiking trails, and dim sum all day. From ₱4,310* (illustrative fare)" [ref=e253] [cursor=pointer]:
                - /url: /destinations/hong-kong
                - img "Night panorama from Victoria Peak over Hong Kong’s illuminated skyscrapers and Victoria Harbour, with Kowloon across the water" [ref=e254]
                - generic [ref=e257]:
                  - paragraph [ref=e258]: Hong Kong SAR
                  - heading "Hong Kong" [level=3] [ref=e259]
                  - paragraph [ref=e260]: A harbour skyline, hiking trails, and dim sum all day.
                  - paragraph [ref=e261]:
                    - text: From
                    - generic [ref=e262]: ₱4,310*
                    - generic [ref=e263]: (illustrative fare)
            - listitem [ref=e268]:
              - link "Taipei 101 towering over the Xinyi district skyline on a clear day, seen over green hills with mountains on the horizon Taiwan Taipei Night markets, hot springs, and mountains on the metro line. From ₱4,340* (illustrative fare)" [ref=e269] [cursor=pointer]:
                - /url: /destinations/taipei
                - img "Taipei 101 towering over the Xinyi district skyline on a clear day, seen over green hills with mountains on the horizon" [ref=e270]
                - generic [ref=e273]:
                  - paragraph [ref=e274]: Taiwan
                  - heading "Taipei" [level=3] [ref=e275]
                  - paragraph [ref=e276]: Night markets, hot springs, and mountains on the metro line.
                  - paragraph [ref=e277]:
                    - text: From
                    - generic [ref=e278]: ₱4,340*
                    - generic [ref=e279]: (illustrative fare)
  - contentinfo [ref=e284]:
    - generic [ref=e286]:
      - generic [ref=e287]:
        - link "Velasia Airways home" [ref=e288] [cursor=pointer]:
          - /url: /
        - paragraph [ref=e296]: Beyond every horizon.
        - paragraph [ref=e297]: A premium full-service airline for the Philippines, Southeast Asia, and beyond, flying from our hubs in Manila and Cebu — in a fictional network built to explore modern airline retailing.
      - generic [ref=e298]:
        - navigation "Book" [ref=e299]:
          - heading "Book" [level=2] [ref=e300]
          - list [ref=e301]:
            - listitem [ref=e302]:
              - link "Search flights" [ref=e303] [cursor=pointer]:
                - /url: /flights
            - listitem [ref=e304]:
              - link "Destinations" [ref=e305] [cursor=pointer]:
                - /url: /destinations
            - listitem [ref=e306]:
              - link "Manage booking" [ref=e307] [cursor=pointer]:
                - /url: /manage-booking
            - listitem [ref=e308]:
              - link "My Trips" [ref=e309] [cursor=pointer]:
                - /url: /account/trips
        - navigation "Help" [ref=e310]:
          - heading "Help" [level=2] [ref=e311]
          - list [ref=e312]:
            - listitem [ref=e313]:
              - link "Help centre" [ref=e314] [cursor=pointer]:
                - /url: /help
            - listitem [ref=e315]:
              - link "Baggage" [ref=e316] [cursor=pointer]:
                - /url: /help#baggage
            - listitem [ref=e317]:
              - link "Travel documents" [ref=e318] [cursor=pointer]:
                - /url: /help#documents
            - listitem [ref=e319]:
              - link "Contact us" [ref=e320] [cursor=pointer]:
                - /url: /help#contact
        - navigation "Velasia" [ref=e321]:
          - heading "Velasia" [level=2] [ref=e322]
          - list [ref=e323]:
            - listitem [ref=e324]:
              - link "About this demo" [ref=e325] [cursor=pointer]:
                - /url: /about
            - listitem [ref=e326]:
              - link "Photo credits" [ref=e327] [cursor=pointer]:
                - /url: /credits
    - generic [ref=e329]:
      - paragraph [ref=e330]: "© 2026 Velasia Airways. A fictional airline: no real reservations, tickets, payments, or financial transactions. All fares shown are illustrative."
      - paragraph [ref=e331]: Velasia Airways is a fictional airline created for demonstration. No real flights, bookings, or payments are offered.
  - alert [ref=e332]: Tokyo, Japan | Velasia Airways
```

# Test source

```ts
  11  |     await page.clock.setFixedTime(new Date(`${E2E_TODAY}T10:00:00+08:00`));
  12  |     await use(page);
  13  |   },
  14  | });
  15  | export { expect };
  16  | 
  17  | /** `YYYY-MM-DD` for the pinned "today" + `days`. */
  18  | export function manilaDate(days = 0): string {
  19  |   return new Date(Date.parse(`${E2E_TODAY}T00:00:00Z`) + days * 86_400_000)
  20  |     .toISOString()
  21  |     .slice(0, 10);
  22  | }
  23  | 
  24  | /** Open the fare comparison from a result card, then choose a fare family. */
  25  | export async function chooseFare(
  26  |   page: Page,
  27  |   card: Locator,
  28  |   family: 'Light' | 'Classic' | 'Flex' | 'Business',
  29  | ) {
  30  |   await card.getByRole('link', { name: /View fares/ }).click();
  31  |   await expect(page.getByTestId('fare-cards')).toBeVisible();
  32  |   await selectFamily(page, family);
  33  | }
  34  | 
  35  | /** Choose a fare family on the fare comparison that is already open. */
  36  | export async function selectFamily(page: Page, family: 'Light' | 'Classic' | 'Flex' | 'Business') {
  37  |   const card = page.locator(`[data-testid="fare-card"][data-family="${family.toUpperCase()}"]`);
  38  |   await card.getByRole('button', { name: new RegExp(`^(Select|Continue with) ${family}`) }).click();
  39  | }
  40  | 
  41  | /** Run a deterministic retailing scenario for this browser context (test-only). */
  42  | export async function simulateScenario(
  43  |   page: Page,
  44  |   scenario:
  45  |     | 'offer-expired'
  46  |     | 'price-increase'
  47  |     | 'sold-out'
  48  |     | 'session-expired'
  49  |     | 'seat-taken'
  50  |     | 'hold-expired'
  51  |     | null,
  52  | ) {
  53  |   const context = page.context();
  54  |   if (scenario === null) await context.clearCookies({ name: 'velasia-scenario' });
  55  |   else await context.addCookies([{ name: 'velasia-scenario', value: scenario, url: BASE_URL }]);
  56  | }
  57  | 
  58  | /** Simulate a retailing API failure for this browser context (test-only fault injection). */
  59  | export async function simulateApiFault(
  60  |   page: Page,
  61  |   fault: 'unavailable' | 'timeout' | 'empty' | 'malformed' | 'slow-commit' | null,
  62  | ) {
  63  |   const context = page.context();
  64  |   if (fault === null) await context.clearCookies({ name: 'velasia-fault' });
  65  |   else await context.addCookies([{ name: 'velasia-fault', value: fault, url: BASE_URL }]);
  66  | }
  67  | 
  68  | /** Collect uncaught errors, console errors (incl. hydration), and failed requests. */
  69  | export function trackPageErrors(page: Page) {
  70  |   const errors: string[] = [];
  71  |   page.on('pageerror', (error) => errors.push(`pageerror: ${error.message}`));
  72  |   page.on('console', (message) => {
  73  |     if (message.type() === 'error') errors.push(`console: ${message.text()}`);
  74  |   });
  75  |   page.on('response', (response) => {
  76  |     if (response.status() >= 400 && !response.url().includes('favicon')) {
  77  |       errors.push(`http ${response.status()}: ${response.url()}`);
  78  |     }
  79  |   });
  80  |   return errors;
  81  | }
  82  | 
  83  | /** Scroll through the page so lazy images load, wait for them, then assert none are broken. */
  84  | export async function expectNoBrokenImages(page: Page) {
  85  |   await page.evaluate(async () => {
  86  |     for (let y = 0; y < document.body.scrollHeight; y += window.innerHeight / 2) {
  87  |       window.scrollTo(0, y);
  88  |       await new Promise((resolve) => setTimeout(resolve, 60));
  89  |     }
  90  |     window.scrollTo(0, 0);
  91  |   });
  92  |   const broken = await page.evaluate(async () => {
  93  |     const images = [...document.images];
  94  |     // Lazy images scrolled past quickly may never start; force them so every image is checked.
  95  |     for (const img of images) img.loading = 'eager';
  96  |     const settled = (img: HTMLImageElement) =>
  97  |       img.complete
  98  |         ? Promise.resolve()
  99  |         : new Promise<void>((resolve) => {
  100 |             img.addEventListener('load', () => resolve(), { once: true });
  101 |             img.addEventListener('error', () => resolve(), { once: true });
  102 |           });
  103 |     await Promise.race([
  104 |       Promise.all(images.map(settled)),
  105 |       new Promise((r) => setTimeout(r, 20_000)),
  106 |     ]);
  107 |     return images
  108 |       .filter((img) => !img.complete || img.naturalWidth === 0)
  109 |       .map((img) => img.currentSrc || img.src);
  110 |   });
> 111 |   expect(broken).toEqual([]);
      |                  ^ Error: expect(received).toEqual(expected) // deep equality
  112 | }
  113 | 
  114 | export async function chooseAirport(
  115 |   page: Page,
  116 |   label: 'From' | 'To',
  117 |   query: string,
  118 |   optionName: RegExp,
  119 | ) {
  120 |   await waitForSearch(page);
  121 |   const input = page.getByRole('combobox', { name: label });
  122 |   await input.click();
  123 |   await input.fill(query);
  124 |   await page
  125 |     .getByRole('listbox', { name: `${label} airports` })
  126 |     .getByRole('option', { name: optionName })
  127 |     .click();
  128 | }
  129 | 
  130 | /** Wait until the search widget has hydrated and restored any saved selections. */
  131 | export async function waitForSearch(page: Page) {
  132 |   await expect(
  133 |     page.locator('[data-testid="flight-search"][data-ready="true"]').first(),
  134 |   ).toBeAttached();
  135 | }
  136 | 
```