# Instructions

- Following Playwright test failed.
- Explain why, be concise, respect Playwright best practices.
- Provide a snippet of code with the fix, if possible.

# Test info

- Name: destinations.spec.ts >> destinations >> lists, searches, and filters destinations
- Location: tests/e2e/destinations.spec.ts:5:3

# Error details

```
Error: expect(received).toEqual(expected) // deep equality

- Expected  -  1
+ Received  + 14

- Array []
+ Array [
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
    - generic [ref=e43]:
      - paragraph [ref=e44]: Explore
      - heading "Destinations across Asia" [level=1] [ref=e45]
      - paragraph [ref=e46]: Twelve places we love, from Philippine islands to the great cities of East Asia. Find your next trip, then search flights in a tap.
    - generic [ref=e48]:
      - generic [ref=e49]:
        - generic [ref=e50]:
          - generic [ref=e51]: Search destinations
          - searchbox "Search destinations" [ref=e52]
        - group "Filter by region" [ref=e53]:
          - button "All destinations" [pressed] [ref=e54]
          - button "Philippines" [ref=e55]
          - button "Southeast Asia" [ref=e56]
          - button "East Asia" [ref=e57]
      - status [ref=e58]: Showing all 12 destinations
      - list [ref=e59]:
        - listitem [ref=e60]:
          - link "Manila skyline of white high-rise towers rising behind a marina of moored yachts on calm Manila Bay at dusk, with a waterfront lamp and chain railing in the foreground Philippines Manila Old walls, new skylines, and the best sunsets on the bay. From ₱2,390* (illustrative fare)" [ref=e61] [cursor=pointer]:
            - /url: /destinations/manila
            - img "Manila skyline of white high-rise towers rising behind a marina of moored yachts on calm Manila Bay at dusk, with a waterfront lamp and chain railing in the foreground" [ref=e62]
            - generic [ref=e65]:
              - paragraph [ref=e66]: Philippines
              - heading "Manila" [level=3] [ref=e67]
              - paragraph [ref=e68]: Old walls, new skylines, and the best sunsets on the bay.
              - paragraph [ref=e69]:
                - text: From
                - generic [ref=e70]: ₱2,390*
                - generic [ref=e71]: (illustrative fare)
        - listitem [ref=e76]:
          - 'link "Kawasan Falls in Cebu: twin waterfalls pouring into a turquoise jungle pool with a bamboo raft floating in the foreground Philippines Cebu Island-hop from a city that never forgot the sea. From ₱2,390* (illustrative fare)" [ref=e77] [cursor=pointer]':
            - /url: /destinations/cebu
            - 'img "Kawasan Falls in Cebu: twin waterfalls pouring into a turquoise jungle pool with a bamboo raft floating in the foreground" [ref=e78]'
            - generic [ref=e81]:
              - paragraph [ref=e82]: Philippines
              - heading "Cebu" [level=3] [ref=e83]
              - paragraph [ref=e84]: Island-hop from a city that never forgot the sea.
              - paragraph [ref=e85]:
                - text: From
                - generic [ref=e86]: ₱2,390*
                - generic [ref=e87]: (illustrative fare)
        - listitem [ref=e92]:
          - link "Two coconut palms silhouetted above tropical forest on Samal Island, overlooking the calm Davao Gulf and a long jetty under a blue dusk sky Philippines Davao Durian, mountain air, and the gateway to Mindanao. From ₱2,916* (illustrative fare)" [ref=e93] [cursor=pointer]:
            - /url: /destinations/davao
            - img "Two coconut palms silhouetted above tropical forest on Samal Island, overlooking the calm Davao Gulf and a long jetty under a blue dusk sky" [ref=e94]
            - generic [ref=e97]:
              - paragraph [ref=e98]: Philippines
              - heading "Davao" [level=3] [ref=e99]
              - paragraph [ref=e100]: Durian, mountain air, and the gateway to Mindanao.
              - paragraph [ref=e101]:
                - text: From
                - generic [ref=e102]: ₱2,916*
                - generic [ref=e103]: (illustrative fare)
        - listitem [ref=e108]:
          - 'link "Singapore’s Marina Bay at blue hour: the illuminated Helix Bridge leads towards the Marina Bay Sands towers and the lotus-shaped ArtScience Museum, reflected in the water Southeast Asia Singapore A garden city with a hawker stall on every corner. From ₱5,680* (illustrative fare)" [ref=e109] [cursor=pointer]':
            - /url: /destinations/singapore
            - 'img "Singapore’s Marina Bay at blue hour: the illuminated Helix Bridge leads towards the Marina Bay Sands towers and the lotus-shaped ArtScience Museum, reflected in the water" [ref=e110]'
            - generic [ref=e113]:
              - paragraph [ref=e114]: Southeast Asia
              - heading "Singapore" [level=3] [ref=e115]
              - paragraph [ref=e116]: A garden city with a hawker stall on every corner.
              - paragraph [ref=e117]:
                - text: From
                - generic [ref=e118]: ₱5,680*
                - generic [ref=e119]: (illustrative fare)
        - listitem [ref=e124]:
          - link "Tokyo skyline at golden hour, with the red-and-white Tokyo Tower rising among dense high-rises in Minato City Japan Tokyo Neon nights, quiet shrines, and perfect ramen in between. From ₱7,160* (illustrative fare)" [ref=e125] [cursor=pointer]:
            - /url: /destinations/tokyo
            - img "Tokyo skyline at golden hour, with the red-and-white Tokyo Tower rising among dense high-rises in Minato City" [ref=e126]
            - generic [ref=e129]:
              - paragraph [ref=e130]: Japan
              - heading "Tokyo" [level=3] [ref=e131]
              - paragraph [ref=e132]: Neon nights, quiet shrines, and perfect ramen in between.
              - paragraph [ref=e133]:
                - text: From
                - generic [ref=e134]: ₱7,160*
                - generic [ref=e135]: (illustrative fare)
        - listitem [ref=e140]:
          - link "Gyeonghoeru, a large two-storey wooden pavilion on stone pillars at Gyeongbokgung Palace in Seoul, reflected in its pond under a clear autumn sky South Korea Seoul Palaces by day, barbecue and karaoke by night. From ₱6,740* (illustrative fare)" [ref=e141] [cursor=pointer]:
            - /url: /destinations/seoul
            - img "Gyeonghoeru, a large two-storey wooden pavilion on stone pillars at Gyeongbokgung Palace in Seoul, reflected in its pond under a clear autumn sky" [ref=e142]
            - generic [ref=e145]:
              - paragraph [ref=e146]: South Korea
              - heading "Seoul" [level=3] [ref=e147]
              - paragraph [ref=e148]: Palaces by day, barbecue and karaoke by night.
              - paragraph [ref=e149]:
                - text: From
                - generic [ref=e150]: ₱6,740*
                - generic [ref=e151]: (illustrative fare)
        - listitem [ref=e156]:
          - link "Wat Arun’s tall central prang and smaller spires rising above the Chao Phraya River in Bangkok, with long-tail boats on the water and flags along the riverbank Thailand Bangkok Golden temples, river boats, and street food after midnight. From ₱5,470* (illustrative fare)" [ref=e157] [cursor=pointer]:
            - /url: /destinations/bangkok
            - img "Wat Arun’s tall central prang and smaller spires rising above the Chao Phraya River in Bangkok, with long-tail boats on the water and flags along the riverbank" [ref=e158]
            - generic [ref=e161]:
              - paragraph [ref=e162]: Thailand
              - heading "Bangkok" [level=3] [ref=e163]
              - paragraph [ref=e164]: Golden temples, river boats, and street food after midnight.
              - paragraph [ref=e165]:
                - text: From
                - generic [ref=e166]: ₱5,470*
                - generic [ref=e167]: (illustrative fare)
        - listitem [ref=e172]:
          - link "Night panorama from Victoria Peak over Hong Kong’s illuminated skyscrapers and Victoria Harbour, with Kowloon across the water Hong Kong SAR Hong Kong A harbour skyline, hiking trails, and dim sum all day. From ₱4,310* (illustrative fare)" [ref=e173] [cursor=pointer]:
            - /url: /destinations/hong-kong
            - img "Night panorama from Victoria Peak over Hong Kong’s illuminated skyscrapers and Victoria Harbour, with Kowloon across the water" [ref=e174]
            - generic [ref=e177]:
              - paragraph [ref=e178]: Hong Kong SAR
              - heading "Hong Kong" [level=3] [ref=e179]
              - paragraph [ref=e180]: A harbour skyline, hiking trails, and dim sum all day.
              - paragraph [ref=e181]:
                - text: From
                - generic [ref=e182]: ₱4,310*
                - generic [ref=e183]: (illustrative fare)
        - listitem [ref=e188]:
          - link "Kuala Lumpur skyline under a blue sky, with the Petronas Twin Towers and KL Tower rising above trees and modern high-rises Malaysia Kuala Lumpur Twin towers, limestone caves, and a breakfast for every culture. From ₱5,810* (illustrative fare)" [ref=e189] [cursor=pointer]:
            - /url: /destinations/kuala-lumpur
            - img "Kuala Lumpur skyline under a blue sky, with the Petronas Twin Towers and KL Tower rising above trees and modern high-rises" [ref=e190]
            - generic [ref=e193]:
              - paragraph [ref=e194]: Malaysia
              - heading "Kuala Lumpur" [level=3] [ref=e195]
              - paragraph [ref=e196]: Twin towers, limestone caves, and a breakfast for every culture.
              - paragraph [ref=e197]:
                - text: From
                - generic [ref=e198]: ₱5,810*
                - generic [ref=e199]: (illustrative fare)
        - listitem [ref=e204]:
          - link "Taipei 101 towering over the Xinyi district skyline on a clear day, seen over green hills with mountains on the horizon Taiwan Taipei Night markets, hot springs, and mountains on the metro line. From ₱4,340* (illustrative fare)" [ref=e205] [cursor=pointer]:
            - /url: /destinations/taipei
            - img "Taipei 101 towering over the Xinyi district skyline on a clear day, seen over green hills with mountains on the horizon" [ref=e206]
            - generic [ref=e209]:
              - paragraph [ref=e210]: Taiwan
              - heading "Taipei" [level=3] [ref=e211]
              - paragraph [ref=e212]: Night markets, hot springs, and mountains on the metro line.
              - paragraph [ref=e213]:
                - text: From
                - generic [ref=e214]: ₱4,340*
                - generic [ref=e215]: (illustrative fare)
        - listitem [ref=e220]:
          - link "Osaka Castle’s white Inui-yagura turret on massive stone walls, reflected in the still outer moat, with autumn trees and Osaka Business Park skyscrapers beyond Japan Osaka Japan’s kitchen, with Kyoto and Nara a short train ride away. From ₱6,770* (illustrative fare)" [ref=e221] [cursor=pointer]:
            - /url: /destinations/osaka
            - img "Osaka Castle’s white Inui-yagura turret on massive stone walls, reflected in the still outer moat, with autumn trees and Osaka Business Park skyscrapers beyond" [ref=e222]
            - generic [ref=e225]:
              - paragraph [ref=e226]: Japan
              - heading "Osaka" [level=3] [ref=e227]
              - paragraph [ref=e228]: Japan’s kitchen, with Kyoto and Nara a short train ride away.
              - paragraph [ref=e229]:
                - text: From
                - generic [ref=e230]: ₱6,770*
                - generic [ref=e231]: (illustrative fare)
        - listitem [ref=e236]:
          - 'link "Ho Chi Minh City at night: Landmark 81 lit in pink and rows of illuminated riverside towers reflected in the Saigon River Vietnam Ho Chi Minh City Scooter-filled boulevards and the world’s best breakfast noodles. From ₱4,830* (illustrative fare)" [ref=e237] [cursor=pointer]':
            - /url: /destinations/ho-chi-minh-city
            - 'img "Ho Chi Minh City at night: Landmark 81 lit in pink and rows of illuminated riverside towers reflected in the Saigon River" [ref=e238]'
            - generic [ref=e241]:
              - paragraph [ref=e242]: Vietnam
              - heading "Ho Chi Minh City" [level=3] [ref=e243]
              - paragraph [ref=e244]: Scooter-filled boulevards and the world’s best breakfast noodles.
              - paragraph [ref=e245]:
                - text: From
                - generic [ref=e246]: ₱4,830*
                - generic [ref=e247]: (illustrative fare)
  - contentinfo [ref=e252]:
    - generic [ref=e254]:
      - generic [ref=e255]:
        - link "Velasia Airways home" [ref=e256] [cursor=pointer]:
          - /url: /
        - paragraph [ref=e264]: Beyond every horizon.
        - paragraph [ref=e265]: A premium full-service airline for the Philippines, Southeast Asia, and beyond, flying from our hubs in Manila and Cebu — in a fictional network built to explore modern airline retailing.
      - generic [ref=e266]:
        - navigation "Book" [ref=e267]:
          - heading "Book" [level=2] [ref=e268]
          - list [ref=e269]:
            - listitem [ref=e270]:
              - link "Search flights" [ref=e271] [cursor=pointer]:
                - /url: /flights
            - listitem [ref=e272]:
              - link "Destinations" [ref=e273] [cursor=pointer]:
                - /url: /destinations
            - listitem [ref=e274]:
              - link "Manage booking" [ref=e275] [cursor=pointer]:
                - /url: /manage-booking
            - listitem [ref=e276]:
              - link "My Trips" [ref=e277] [cursor=pointer]:
                - /url: /account/trips
        - navigation "Help" [ref=e278]:
          - heading "Help" [level=2] [ref=e279]
          - list [ref=e280]:
            - listitem [ref=e281]:
              - link "Help centre" [ref=e282] [cursor=pointer]:
                - /url: /help
            - listitem [ref=e283]:
              - link "Baggage" [ref=e284] [cursor=pointer]:
                - /url: /help#baggage
            - listitem [ref=e285]:
              - link "Travel documents" [ref=e286] [cursor=pointer]:
                - /url: /help#documents
            - listitem [ref=e287]:
              - link "Contact us" [ref=e288] [cursor=pointer]:
                - /url: /help#contact
        - navigation "Velasia" [ref=e289]:
          - heading "Velasia" [level=2] [ref=e290]
          - list [ref=e291]:
            - listitem [ref=e292]:
              - link "About this demo" [ref=e293] [cursor=pointer]:
                - /url: /about
            - listitem [ref=e294]:
              - link "Photo credits" [ref=e295] [cursor=pointer]:
                - /url: /credits
    - generic [ref=e297]:
      - paragraph [ref=e298]: "© 2026 Velasia Airways. A fictional airline: no real reservations, tickets, payments, or financial transactions. All fares shown are illustrative."
      - paragraph [ref=e299]: Velasia Airways is a fictional airline created for demonstration. No real flights, bookings, or payments are offered.
  - alert [ref=e300]
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