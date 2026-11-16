# Instructions

- Following Playwright test failed.
- Explain why, be concise, respect Playwright best practices.
- Provide a snippet of code with the fix, if possible.

# Test info

- Name: home.spec.ts >> homepage >> loads without errors, broken images, or hydration warnings
- Location: tests/e2e/home.spec.ts:5:3

# Error details

```
Error: expect(received).toEqual(expected) // deep equality

- Expected  -  1
+ Received  + 17

- Array []
+ Array [
+   "http://127.0.0.1:3200/_next/image?url=%2Fimages%2Fbrand%2Fhero.jpg&w=1600&q=80",
+   "http://127.0.0.1:3200/_next/image?url=%2Fimages%2Fdestinations%2Fcebu.jpg&w=1080&q=70",
+   "http://127.0.0.1:3200/_next/image?url=%2Fimages%2Fdestinations%2Fsingapore.jpg&w=1080&q=70",
+   "http://127.0.0.1:3200/_next/image?url=%2Fimages%2Fdestinations%2Ftokyo.jpg&w=828&q=70",
+   "http://127.0.0.1:3200/_next/image?url=%2Fimages%2Fdestinations%2Fseoul.jpg&w=828&q=70",
+   "http://127.0.0.1:3200/_next/image?url=%2Fimages%2Fdestinations%2Fbangkok.jpg&w=828&q=70",
+   "http://127.0.0.1:3200/_next/image?url=%2Fimages%2Fdestinations%2Fosaka.jpg&w=828&q=70",
+   "http://127.0.0.1:3200/_next/image?url=%2Fimages%2Fdestinations%2Fcebu.jpg&w=384&q=70",
+   "http://127.0.0.1:3200/_next/image?url=%2Fimages%2Fdestinations%2Fhong-kong.jpg&w=384&q=70",
+   "http://127.0.0.1:3200/_next/image?url=%2Fimages%2Fdestinations%2Ftaipei.jpg&w=384&q=70",
+   "http://127.0.0.1:3200/_next/image?url=%2Fimages%2Fdestinations%2Fsingapore.jpg&w=384&q=70",
+   "http://127.0.0.1:3200/_next/image?url=%2Fimages%2Fbrand%2Fwindow.jpg&w=828&q=70",
+   "http://127.0.0.1:3200/_next/image?url=%2Fimages%2Fbrand%2Fbeach.jpg&w=640&q=70",
+   "http://127.0.0.1:3200/_next/image?url=%2Fimages%2Fbrand%2Ffood.jpg&w=640&q=70",
+   "http://127.0.0.1:3200/_next/image?url=%2Fimages%2Fbrand%2Fculture.jpg&w=640&q=70",
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
    - region "Beyond every horizon." [ref=e41]:
      - generic [ref=e42]:
        - img "Turquoise water winding between towering green-and-grey limestone cliffs in El Nido, Palawan, with a small white-sand beach and outrigger boats under a clear blue sky" [ref=e43]
        - generic [ref=e46]:
          - generic [ref=e47]: Philippines · Southeast Asia · East Asia
          - heading "Beyond every horizon." [level=1] [ref=e48]: Beyond everyhorizon.
          - paragraph [ref=e49]: Island weekends, city breaks, and long-awaited reunions — flown with warm Filipino hospitality, and fares you shape around how you travel.
      - generic [ref=e51]:
        - heading "Book a flight" [level=2] [ref=e54]
        - search "Search flights" [ref=e55]:
          - generic [ref=e56]:
            - group "Trip type" [ref=e57]:
              - generic [ref=e59]:
                - generic [ref=e60] [cursor=pointer]:
                  - radio "Round-trip" [checked] [ref=e61]
                  - generic [ref=e62]: Round-trip
                - generic [ref=e63] [cursor=pointer]:
                  - radio "One-way" [ref=e64]
                  - generic [ref=e65]: One-way
            - 'button "Passengers: 1 Adult" [ref=e67]': 1 Adult
            - generic [ref=e75]:
              - generic [ref=e76]: Cabin
              - combobox "Cabin" [ref=e77] [cursor=pointer]:
                - option "Economy" [selected]
                - option "Premium Economy"
                - option "Business"
          - generic [ref=e78]:
            - generic [ref=e79]:
              - generic [ref=e80]:
                - generic [ref=e86]:
                  - generic [ref=e87]: From
                  - combobox "From" [ref=e88]
                - paragraph [ref=e89]
              - button "Swap departure and arrival airports" [disabled] [ref=e90]
              - generic [ref=e94]:
                - generic [ref=e100]:
                  - generic [ref=e101]: To
                  - combobox "To" [ref=e102]
                - paragraph [ref=e103]
            - generic [ref=e109]:
              - generic [ref=e110]: Depart
              - textbox "Depart" [ref=e111]
            - generic [ref=e117]:
              - generic [ref=e118]: Return
              - textbox "Return" [ref=e119]
            - button "Search flights" [ref=e120]
          - status [ref=e124]
    - region "Where will you go next?" [ref=e125]:
      - generic [ref=e126]:
        - generic [ref=e127]:
          - generic [ref=e128]:
            - paragraph [ref=e129]: Featured destinations
            - heading "Where will you go next?" [level=2] [ref=e130]
            - paragraph [ref=e131]: From island hideaways a short hop from Manila to the great cities of East Asia.
          - link "Explore all destinations" [ref=e133] [cursor=pointer]:
            - /url: /destinations
        - list [ref=e137]:
          - listitem [ref=e138]:
            - 'link "Kawasan Falls in Cebu: twin waterfalls pouring into a turquoise jungle pool with a bamboo raft floating in the foreground Philippines Cebu Island-hop from a city that never forgot the sea. From ₱2,390* (illustrative fare)" [ref=e139] [cursor=pointer]':
              - /url: /destinations/cebu
              - 'img "Kawasan Falls in Cebu: twin waterfalls pouring into a turquoise jungle pool with a bamboo raft floating in the foreground" [ref=e140]'
              - generic [ref=e143]:
                - paragraph [ref=e144]: Philippines
                - heading "Cebu" [level=3] [ref=e145]
                - paragraph [ref=e146]: Island-hop from a city that never forgot the sea.
                - paragraph [ref=e147]:
                  - text: From
                  - generic [ref=e148]: ₱2,390*
                  - generic [ref=e149]: (illustrative fare)
          - listitem [ref=e154]:
            - 'link "Singapore’s Marina Bay at blue hour: the illuminated Helix Bridge leads towards the Marina Bay Sands towers and the lotus-shaped ArtScience Museum, reflected in the water Southeast Asia Singapore A garden city with a hawker stall on every corner. From ₱5,680* (illustrative fare)" [ref=e155] [cursor=pointer]':
              - /url: /destinations/singapore
              - 'img "Singapore’s Marina Bay at blue hour: the illuminated Helix Bridge leads towards the Marina Bay Sands towers and the lotus-shaped ArtScience Museum, reflected in the water" [ref=e156]'
              - generic [ref=e159]:
                - paragraph [ref=e160]: Southeast Asia
                - heading "Singapore" [level=3] [ref=e161]
                - paragraph [ref=e162]: A garden city with a hawker stall on every corner.
                - paragraph [ref=e163]:
                  - text: From
                  - generic [ref=e164]: ₱5,680*
                  - generic [ref=e165]: (illustrative fare)
          - listitem [ref=e170]:
            - link "Tokyo skyline at golden hour, with the red-and-white Tokyo Tower rising among dense high-rises in Minato City Japan Tokyo Neon nights, quiet shrines, and perfect ramen in between. From ₱7,160* (illustrative fare)" [ref=e171] [cursor=pointer]:
              - /url: /destinations/tokyo
              - img "Tokyo skyline at golden hour, with the red-and-white Tokyo Tower rising among dense high-rises in Minato City" [ref=e172]
              - generic [ref=e175]:
                - paragraph [ref=e176]: Japan
                - heading "Tokyo" [level=3] [ref=e177]
                - paragraph [ref=e178]: Neon nights, quiet shrines, and perfect ramen in between.
                - paragraph [ref=e179]:
                  - text: From
                  - generic [ref=e180]: ₱7,160*
                  - generic [ref=e181]: (illustrative fare)
          - listitem [ref=e186]:
            - link "Gyeonghoeru, a large two-storey wooden pavilion on stone pillars at Gyeongbokgung Palace in Seoul, reflected in its pond under a clear autumn sky South Korea Seoul Palaces by day, barbecue and karaoke by night. From ₱6,740* (illustrative fare)" [ref=e187] [cursor=pointer]:
              - /url: /destinations/seoul
              - img "Gyeonghoeru, a large two-storey wooden pavilion on stone pillars at Gyeongbokgung Palace in Seoul, reflected in its pond under a clear autumn sky" [ref=e188]
              - generic [ref=e191]:
                - paragraph [ref=e192]: South Korea
                - heading "Seoul" [level=3] [ref=e193]
                - paragraph [ref=e194]: Palaces by day, barbecue and karaoke by night.
                - paragraph [ref=e195]:
                  - text: From
                  - generic [ref=e196]: ₱6,740*
                  - generic [ref=e197]: (illustrative fare)
          - listitem [ref=e202]:
            - link "Wat Arun’s tall central prang and smaller spires rising above the Chao Phraya River in Bangkok, with long-tail boats on the water and flags along the riverbank Thailand Bangkok Golden temples, river boats, and street food after midnight. From ₱5,470* (illustrative fare)" [ref=e203] [cursor=pointer]:
              - /url: /destinations/bangkok
              - img "Wat Arun’s tall central prang and smaller spires rising above the Chao Phraya River in Bangkok, with long-tail boats on the water and flags along the riverbank" [ref=e204]
              - generic [ref=e207]:
                - paragraph [ref=e208]: Thailand
                - heading "Bangkok" [level=3] [ref=e209]
                - paragraph [ref=e210]: Golden temples, river boats, and street food after midnight.
                - paragraph [ref=e211]:
                  - text: From
                  - generic [ref=e212]: ₱5,470*
                  - generic [ref=e213]: (illustrative fare)
          - listitem [ref=e218]:
            - link "Osaka Castle’s white Inui-yagura turret on massive stone walls, reflected in the still outer moat, with autumn trees and Osaka Business Park skyscrapers beyond Japan Osaka Japan’s kitchen, with Kyoto and Nara a short train ride away. From ₱6,770* (illustrative fare)" [ref=e219] [cursor=pointer]:
              - /url: /destinations/osaka
              - img "Osaka Castle’s white Inui-yagura turret on massive stone walls, reflected in the still outer moat, with autumn trees and Osaka Business Park skyscrapers beyond" [ref=e220]
              - generic [ref=e223]:
                - paragraph [ref=e224]: Japan
                - heading "Osaka" [level=3] [ref=e225]
                - paragraph [ref=e226]: Japan’s kitchen, with Kyoto and Nara a short train ride away.
                - paragraph [ref=e227]:
                  - text: From
                  - generic [ref=e228]: ₱6,770*
                  - generic [ref=e229]: (illustrative fare)
    - region "Fares worth packing for" [ref=e234]:
      - generic [ref=e235]:
        - generic [ref=e237]:
          - paragraph [ref=e238]: Fare inspiration
          - heading "Fares worth packing for" [level=2] [ref=e239]
          - paragraph [ref=e240]: A taste of where a few thousand pesos could take you. Live fares arrive with flight search.
        - list [ref=e241]:
          - listitem [ref=e242]:
            - article [ref=e243]:
              - generic [ref=e244]: Cebu
              - generic [ref=e249]:
                - heading "Island weekends" [level=3] [ref=e250]
                - paragraph [ref=e251]: Swap the city for clear water. Fly to Cebu on Friday, be on a boat by Saturday morning.
                - generic [ref=e252]:
                  - paragraph [ref=e253]:
                    - generic [ref=e254]: One-way from
                    - generic [ref=e255]: ₱2,390*
                    - generic [ref=e256]:
                      - generic [ref=e257]: "Note:"
                      - text: Illustrative fare · not a live offer
                  - link "Explore Cebu" [ref=e258] [cursor=pointer]:
                    - /url: /destinations/cebu
                    - text: Explore
                    - generic [ref=e259]: Cebu
          - listitem [ref=e262]:
            - article [ref=e263]:
              - generic [ref=e264]: Hong Kong
              - generic [ref=e269]:
                - heading "Dim sum by lunchtime" [level=3] [ref=e270]
                - paragraph [ref=e271]: Leave Manila after breakfast, be at a dim sum table in Hong Kong before noon.
                - generic [ref=e272]:
                  - paragraph [ref=e273]:
                    - generic [ref=e274]: One-way from
                    - generic [ref=e275]: ₱4,310*
                    - generic [ref=e276]:
                      - generic [ref=e277]: "Note:"
                      - text: Illustrative fare · not a live offer
                  - link "Explore Hong Kong" [ref=e278] [cursor=pointer]:
                    - /url: /destinations/hong-kong
                    - text: Explore
                    - generic [ref=e279]: Hong Kong
          - listitem [ref=e282]:
            - article [ref=e283]:
              - generic [ref=e284]: Taipei
              - generic [ref=e289]:
                - heading "Snack your way through Taipei" [level=3] [ref=e290]
                - paragraph [ref=e291]: Two hours from Manila, a lifetime of night-market bites. Pack stretchy trousers.
                - generic [ref=e292]:
                  - paragraph [ref=e293]:
                    - generic [ref=e294]: One-way from
                    - generic [ref=e295]: ₱4,340*
                    - generic [ref=e296]:
                      - generic [ref=e297]: "Note:"
                      - text: Illustrative fare · not a live offer
                  - link "Explore Taipei" [ref=e298] [cursor=pointer]:
                    - /url: /destinations/taipei
                    - text: Explore
                    - generic [ref=e299]: Taipei
          - listitem [ref=e302]:
            - article [ref=e303]:
              - generic [ref=e304]: Singapore
              - generic [ref=e309]:
                - heading "A long weekend in the Lion City" [level=3] [ref=e310]
                - paragraph [ref=e311]: Three days, twelve hawker stalls, one very good chilli crab. We’ll handle the flight.
                - generic [ref=e312]:
                  - paragraph [ref=e313]:
                    - generic [ref=e314]: One-way from
                    - generic [ref=e315]: ₱5,680*
                    - generic [ref=e316]:
                      - generic [ref=e317]: "Note:"
                      - text: Illustrative fare · not a live offer
                  - link "Explore Singapore" [ref=e318] [cursor=pointer]:
                    - /url: /destinations/singapore
                    - text: Explore
                    - generic [ref=e319]: Singapore
        - paragraph [ref=e322]: "*Lowest indicative one-way Economy fare per adult, including simulated taxes and charges, from the origin noted on each destination page over the published schedule. These are not offers, are not guaranteed, and cannot be booked."
    - region "Thoughtful from check‑in to touchdown" [ref=e323]:
      - generic [ref=e324]:
        - generic [ref=e326]:
          - paragraph [ref=e327]: Flying Velasia
          - heading "Thoughtful from check‑in to touchdown" [level=2] [ref=e328]
        - list [ref=e329]:
          - listitem [ref=e330]:
            - heading "Bags, your way" [level=3] [ref=e337]
            - paragraph [ref=e338]: Fly light with a cabin bag, or add checked baggage when your plans grow.
          - listitem [ref=e339]:
            - heading "Comfort that scales" [level=3] [ref=e344]
            - paragraph [ref=e345]: Three cabins, from smart Economy to Business with extra space on longer routes.
          - listitem [ref=e346]:
            - heading "A taste of home" [level=3] [ref=e353]
            - paragraph [ref=e354]: Menus inspired by Filipino and regional favourites on international flights.
          - listitem [ref=e355]:
            - heading "Warm hospitality" [level=3] [ref=e359]
            - paragraph [ref=e360]: Crew who look after you from the first boarding call to the final bag on the belt.
    - region "Travel your way" [ref=e361]:
      - generic [ref=e362]:
        - generic [ref=e363]:
          - img "View from a plane window of a silver wing and winglet over patchwork fields, with soft sunset light breaking through the clouds" [ref=e364]
          - paragraph [ref=e366]: Pay for what matters to you. Skip what doesn’t.
        - generic [ref=e367]:
          - generic [ref=e368]: Coming with live flight search
          - heading "Travel your way" [level=2] [ref=e372]
          - paragraph [ref=e373]: Every Velasia offer will start with a seat and grow with you. Pick a fare bundle, then add bags, seats, meals, and flexibility — so a quick business trip and a family holiday each get exactly what they need.
          - list [ref=e374]:
            - listitem [ref=e375]:
              - heading "Lite" [level=3] [ref=e376]
              - paragraph [ref=e377]: Just you and a backpack
              - list [ref=e378]:
                - listitem [ref=e379]: 7 kg cabin bag
                - listitem [ref=e382]: Seat assigned at check-in
            - listitem [ref=e385]:
              - heading "Classic" [level=3] [ref=e386]
              - paragraph [ref=e387]: The everyday favourite
              - list [ref=e388]:
                - listitem [ref=e389]: 7 kg cabin bag + 20 kg checked
                - listitem [ref=e392]: Choose your seat
                - listitem [ref=e395]: Change for a fee
            - listitem [ref=e398]:
              - heading "Flex" [level=3] [ref=e399]
              - paragraph [ref=e400]: Plans that can move
              - list [ref=e401]:
                - listitem [ref=e402]: 7 kg cabin bag + 30 kg checked
                - listitem [ref=e405]: Free changes
                - listitem [ref=e408]: Priority boarding
          - paragraph [ref=e411]: Bundles shown are concepts for an upcoming release. Inclusions will be confirmed with each live offer.
    - region "Ideas for your next escape" [ref=e412]:
      - generic [ref=e413]:
        - generic [ref=e415]:
          - paragraph [ref=e416]: Travel inspiration
          - heading "Ideas for your next escape" [level=2] [ref=e417]
          - paragraph [ref=e418]: Not sure where to go? Start with how you’d like to feel when you get there.
        - list [ref=e419]:
          - listitem [ref=e420]:
            - link [ref=e421] [cursor=pointer]:
              - /url: /destinations/cebu
              - img "Coconut palms leaning over a quiet golden-sand beach and calm shallow sea on Koh Mak, Thailand, under a bright blue sky" [ref=e423]
              - paragraph [ref=e424]: Islands & beaches
              - heading "Sun, sand, and slow mornings" [level=3] [ref=e425]
              - paragraph [ref=e426]: Trade alarms for tides with a few nights by the water.
              - generic [ref=e427]: Start in Cebu
          - listitem [ref=e431]:
            - link [ref=e432] [cursor=pointer]:
              - /url: /destinations/kuala-lumpur
              - img "A plate of nasi lemak with coconut rice, sambal, fried egg, fried chicken, cucumber slices, peanuts and anchovies, beside a fresh coconut with a straw on a wooden table" [ref=e434]
              - paragraph [ref=e435]: Food journeys
              - heading "Eat your way across Asia" [level=3] [ref=e436]
              - paragraph [ref=e437]: Nasi lemak for breakfast, satay by night — Kuala Lumpur never stops serving.
              - generic [ref=e438]: Discover Kuala Lumpur
          - listitem [ref=e442]:
            - link [ref=e443] [cursor=pointer]:
              - /url: /destinations/bangkok
              - img "The central brick prang of Wat Chaiwatthanaram in Ayutthaya, Thailand, with seated Buddha statues draped in saffron cloth and smaller spires under a blue sky" [ref=e445]
              - paragraph [ref=e446]: Culture & history
              - heading "Temples and old capitals" [level=3] [ref=e447]
              - paragraph [ref=e448]: Ayutthaya’s ruins are an easy day trip from Bangkok’s riverside temples.
              - generic [ref=e449]: Discover Bangkok
    - region "Helpful booking information" [ref=e453]:
      - generic [ref=e454]:
        - generic [ref=e456]:
          - paragraph [ref=e457]: Before you fly
          - heading "Helpful booking information" [level=2] [ref=e458]
        - list [ref=e459]:
          - listitem [ref=e460]:
            - link [ref=e461] [cursor=pointer]:
              - /url: /help#baggage
              - heading "Baggage allowance" [level=3] [ref=e468]
              - paragraph [ref=e469]: Cabin and checked allowances, sizes, and what to pack where.
              - generic [ref=e470]: Baggage guide
          - listitem [ref=e473]:
            - link [ref=e474] [cursor=pointer]:
              - /url: /help#documents
              - heading "Travel documents" [level=3] [ref=e479]
              - paragraph [ref=e480]: Passports, visas, and IDs for domestic and international trips.
              - generic [ref=e481]: Document checklist
          - listitem [ref=e484]:
            - link [ref=e485] [cursor=pointer]:
              - /url: /help#check-in
              - heading "Check-in" [level=3] [ref=e488]
              - paragraph [ref=e489]: When check-in opens, and how early to arrive at the airport.
              - generic [ref=e490]: Check-in times
          - listitem [ref=e493]:
            - link [ref=e494] [cursor=pointer]:
              - /url: /manage-booking
              - heading "Manage your booking" [level=3] [ref=e506]
              - paragraph [ref=e507]: Changes, seats, and extras after you book — coming soon.
              - generic [ref=e508]: See what’s coming
  - contentinfo [ref=e511]:
    - generic [ref=e513]:
      - generic [ref=e514]:
        - link "Velasia Airways home" [ref=e515] [cursor=pointer]:
          - /url: /
        - paragraph [ref=e523]: Beyond every horizon.
        - paragraph [ref=e524]: A premium full-service airline for the Philippines, Southeast Asia, and beyond, flying from our hubs in Manila and Cebu — in a fictional network built to explore modern airline retailing.
      - generic [ref=e525]:
        - navigation "Book" [ref=e526]:
          - heading "Book" [level=2] [ref=e527]
          - list [ref=e528]:
            - listitem [ref=e529]:
              - link "Search flights" [ref=e530] [cursor=pointer]:
                - /url: /flights
            - listitem [ref=e531]:
              - link "Destinations" [ref=e532] [cursor=pointer]:
                - /url: /destinations
            - listitem [ref=e533]:
              - link "Manage booking" [ref=e534] [cursor=pointer]:
                - /url: /manage-booking
            - listitem [ref=e535]:
              - link "My Trips" [ref=e536] [cursor=pointer]:
                - /url: /account/trips
        - navigation "Help" [ref=e537]:
          - heading "Help" [level=2] [ref=e538]
          - list [ref=e539]:
            - listitem [ref=e540]:
              - link "Help centre" [ref=e541] [cursor=pointer]:
                - /url: /help
            - listitem [ref=e542]:
              - link "Baggage" [ref=e543] [cursor=pointer]:
                - /url: /help#baggage
            - listitem [ref=e544]:
              - link "Travel documents" [ref=e545] [cursor=pointer]:
                - /url: /help#documents
            - listitem [ref=e546]:
              - link "Contact us" [ref=e547] [cursor=pointer]:
                - /url: /help#contact
        - navigation "Velasia" [ref=e548]:
          - heading "Velasia" [level=2] [ref=e549]
          - list [ref=e550]:
            - listitem [ref=e551]:
              - link "About this demo" [ref=e552] [cursor=pointer]:
                - /url: /about
            - listitem [ref=e553]:
              - link "Photo credits" [ref=e554] [cursor=pointer]:
                - /url: /credits
    - generic [ref=e556]:
      - paragraph [ref=e557]: "© 2026 Velasia Airways. A fictional airline: no real reservations, tickets, payments, or financial transactions. All fares shown are illustrative."
      - paragraph [ref=e558]: Velasia Airways is a fictional airline created for demonstration. No real flights, bookings, or payments are offered.
  - alert [ref=e559]
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