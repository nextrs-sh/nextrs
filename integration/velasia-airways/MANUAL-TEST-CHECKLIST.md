# Manual browser test checklist: Velasia Airways on NextRS

**Target:** <http://localhost:3000>, served by `@nextrs/cli@0.1.0-rc.1` (see `TEST-REPORT.md`).
**Browsers:** run every test in Chrome **and** Firefox, ideally in a private window each so cookies
from one run do not leak into the next.
**Data:** everything is fictional. Use invented travellers and the demo payment methods only.

## Before you start

1. Start the environment and wait until both services are `healthy`:
   ```bash
   docker compose -f integration/velasia-airways/compose.yaml up --build -d
   docker compose -f integration/velasia-airways/compose.yaml ps
   ```
2. Confirm that NextRS is serving (every check must say PASS):
   ```bash
   integration/velasia-airways/scripts/verify-runtime.sh
   ```
3. Open DevTools (F12) and keep two panels visible for every test:
   - **Console:** note every red error. React hydration errors look like
     `Hydration failed…` or `A tree hydrated but some attributes…`.
   - **Network:** turn on *Preserve log* (Chrome) or *Persist Logs* (Firefox). Note every
     request with a 4xx/5xx status or a "(failed)"/"NS_ERROR" state.
4. Follow the server logs in a second terminal:
   ```bash
   docker compose -f integration/velasia-airways/compose.yaml logs -f velasia-web
   ```

**Dates.** The environment uses the real date (Asia/Manila). Pick a departure date **2 to 6 weeks
from today** (`YYYY-MM-DD` below means such a date). Flights are published for 120 days.

**Payments.** By default, every demo payment is approved and there is no outcome picker. To test
declines, timeouts and pending payments, put `PAYMENT_OUTCOME_CONTROLS=enabled` in
`integration/velasia-airways/.env` and run `docker compose -f integration/velasia-airways/compose.yaml up -d`.
Tests that need it are marked **[outcome controls]**.

**Recording a failure.** For any FAIL, write down the URL, the request method, the HTTP status,
the console error and the matching `velasia-web` log lines (see `README.md`, "When something
fails"). Do not change Velasia Airways to make a test pass.

Status key: ☐ PASS ☐ FAIL ☐ BLOCKED (cannot reach this step) ☐ N/A (not implemented).

---

## 1. Homepage and navigation

### 1.1 Homepage renders
- **URL:** `/`
- **Steps:** 1. Open <http://localhost:3000/>. 2. Look at the page before scrolling. 3. Scroll to the footer.
- **Expected:** Heading "Beyond every horizon." with a hero image; a "Search flights" form with
  From, To, dates and Passengers; section headings further down; the footer with links. Fonts and
  colours applied (not a plain unstyled page). No console errors.
- **Observed:** ____________________
- **Chrome:** ☐ PASS ☐ FAIL  **Firefox:** ☐ PASS ☐ FAIL
- **Notes:**

### 1.2 Primary navigation (client-side)
- **URL:** `/`
- **Steps:** 1. In the header, click each of: Explore destinations, Book flights, Manage booking,
  Help. 2. Click the Velasia Airways logo. 3. Watch the Network panel's "Doc" filter.
- **Expected:** URLs `/destinations`, `/flights`, `/manage-booking`, `/help`, `/`. Headings
  "Destinations across Asia", "Where would you like to go?", "Your trip, in your hands",
  "How can we help?". The current link is highlighted. **No new document request** per click
  (only `?_rsc=` fetch requests with type `text/x-component`): the page does not flash white.
- **Observed:** ____________________
- **Chrome:** ☐ PASS ☐ FAIL  **Firefox:** ☐ PASS ☐ FAIL
- **Notes:**

### 1.3 Footer links
- **URL:** `/`
- **Steps:** Open every footer link (About, Help, Photo credits, …) in turn.
- **Expected:** Each opens a real page with status 200 (`/about`, `/help`, `/credits`, …). No 404s.
- **Observed:** ____________________
- **Chrome:** ☐ PASS ☐ FAIL  **Firefox:** ☐ PASS ☐ FAIL
- **Notes:**

### 1.4 Destinations and a destination detail page (dynamic route)
- **URL:** `/destinations`, then `/destinations/<slug>`
- **Steps:** 1. Open `/destinations`. 2. Click any destination card. 3. Press F5 on the detail page.
- **Expected:** Destination list with photos; the detail page has its own heading, photo and
  route information; refreshing shows the same page (status 200), not an error.
- **Observed:** ____________________
- **Chrome:** ☐ PASS ☐ FAIL  **Firefox:** ☐ PASS ☐ FAIL
- **Notes:**

### 1.5 Images
- **URL:** `/`, `/destinations`
- **Steps:** Scroll slowly through both pages. In Network, filter "Img".
- **Expected:** No broken image icons. Images come from `/_next/image?url=/images/…&w=…&q=…`
  with status 200 and type `image/avif` or `image/webp`.
- **Observed:** ____________________
- **Chrome:** ☐ PASS ☐ FAIL  **Firefox:** ☐ PASS ☐ FAIL
- **Notes:**

## 2. Flight search

### 2.1 Search from the homepage
- **URL:** `/`
- **Steps:** 1. From: type "Manila", choose Manila (MNL). 2. To: type "Singapore", choose
  Singapore (SIN). 3. Choose One way and a departure date. 4. Passengers: 1 adult. 5. Click Search flights.
- **Expected:** The airport lists filter as you type; keyboard (↓, Enter) also works. You land on
  `/flights?trip=one-way&from=MNL&to=SIN&depart=YYYY-MM-DD&adults=1&cabin=economy` with a search
  summary and flight cards.
- **Observed:** ____________________
- **Chrome:** ☐ PASS ☐ FAIL  **Firefox:** ☐ PASS ☐ FAIL
- **Notes:**

### 2.2 Round trip and several passengers
- **URL:** `/`
- **Steps:** 1. Manila (MNL) → Hong Kong (HKG), round trip, return 4 days later. 2. Passengers:
  add one adult and one infant, click Done. 3. Search.
- **Expected:** Results for the outbound flight; the summary shows 2 adults and 1 infant and the
  round trip. The swap button (⇄) between From and To swaps the airports.
- **Observed:** ____________________
- **Chrome:** ☐ PASS ☐ FAIL  **Firefox:** ☐ PASS ☐ FAIL
- **Notes:**

### 2.3 Invalid search
- **URL:** `/flights?trip=round-trip&from=MNL&to=MNL&depart=2001-01-01&adults=0`
- **Steps:** Open the URL.
- **Expected:** An alert "We couldn’t run this search" with the form pre-filled; no flight cards;
  no crash page.
- **Observed:** ____________________
- **Chrome:** ☐ PASS ☐ FAIL  **Firefox:** ☐ PASS ☐ FAIL
- **Notes:**

### 2.4 Search is remembered
- **URL:** `/`
- **Steps:** 1. Search Davao (DVO) → any city. 2. Click Explore destinations, then the logo.
- **Expected:** The search form on the homepage still shows Davao (DVO).
- **Observed:** ____________________
- **Chrome:** ☐ PASS ☐ FAIL  **Firefox:** ☐ PASS ☐ FAIL
- **Notes:**

## 3. Flight results and filtering

### 3.1 Results list
- **URL:** `/flights?trip=one-way&from=MNL&to=SIN&depart=YYYY-MM-DD&adults=1&cabin=economy`
- **Steps:** Read the result cards.
- **Expected:** Each card shows flight number (NV…), times, duration, stops and a price "per adult"
  in ₱. "Choose your flight" heading.
- **Observed:** ____________________
- **Chrome:** ☐ PASS ☐ FAIL  **Firefox:** ☐ PASS ☐ FAIL
- **Notes:**

### 3.2 Stops filter
- **URL:** `/flights?trip=one-way&from=CEB&to=SIN&depart=YYYY-MM-DD&adults=1&cabin=economy`
- **Steps:** 1. Choose "Direct flights only". 2. Choose "Any number of stops".
- **Expected:** The URL gains a `stops=` parameter, the list updates without a full page reload,
  only direct flights are shown, then all flights again.
- **Observed:** ____________________
- **Chrome:** ☐ PASS ☐ FAIL  **Firefox:** ☐ PASS ☐ FAIL
- **Notes:**

### 3.3 Nearby dates
- **URL:** same as 3.1
- **Steps:** Click another day in the "Nearby dates" strip.
- **Expected:** The search summary shows the new date and results refresh.
- **Observed:** ____________________
- **Chrome:** ☐ PASS ☐ FAIL  **Firefox:** ☐ PASS ☐ FAIL
- **Notes:**

### 3.4 Flight details dialog
- **URL:** `/flights?trip=one-way&from=CEB&to=SIN&depart=YYYY-MM-DD&adults=1&cabin=economy`
- **Steps:** On a connecting flight, click Details. Press Esc.
- **Expected:** A "Flight details" dialog with each segment and the connection; "View fares"
  inside it; Esc closes it and returns focus.
- **Observed:** ____________________
- **Chrome:** ☐ PASS ☐ FAIL  **Firefox:** ☐ PASS ☐ FAIL
- **Notes:**

## 4. Flight selection

### 4.1 Fare comparison and choice
- **URL:** results page from 3.1
- **Steps:** 1. On a card click "View fares". 2. Compare Light, Classic, Flex (and Business where
  offered). 3. Click "Select Classic".
- **Expected:** Fare cards with what is included; after selecting, a trip summary with the total
  price and "Continue to traveller details".
- **Observed:** ____________________
- **Chrome:** ☐ PASS ☐ FAIL  **Firefox:** ☐ PASS ☐ FAIL
- **Notes:**

## 5. Passenger information

### 5.1 Traveller form validation
- **URL:** `/booking/travellers`
- **Steps:** 1. Click "Continue to traveller details" (from 4.1). 2. Click Save without filling anything.
- **Expected:** Field-level errors for names, date of birth, nationality, email and phone; focus
  moves to the first error; nothing is submitted.
- **Observed:** ____________________
- **Chrome:** ☐ PASS ☐ FAIL  **Firefox:** ☐ PASS ☐ FAIL
- **Notes:**

### 5.2 Save travellers (Server Action)
- **URL:** `/booking/travellers`
- **Steps:** Given names "Amihan", Family name "Dela Cruz", Date of birth 1990-04-12, Nationality
  Philippines, Email `amihan.delacruz@example.com`, Mobile `+63 917 555 0123`. Save.
- **Expected:** A POST with a `Next-Action` header answers 200; you land on `/booking/seats`.
- **Observed:** ____________________
- **Chrome:** ☐ PASS ☐ FAIL  **Firefox:** ☐ PASS ☐ FAIL
- **Notes:**

### 5.3 US spelling redirect
- **URL:** `/booking/travelers`
- **Steps:** Open the URL.
- **Expected:** Permanent redirect (308) to `/booking/travellers`.
- **Observed:** ____________________
- **Chrome:** ☐ PASS ☐ FAIL  **Firefox:** ☐ PASS ☐ FAIL
- **Notes:**

## 6. Seat selection

### 6.1 Choose a seat
- **URL:** `/booking/seats`
- **Steps:** 1. Wait for the seat map. 2. Click an available seat, then "Choose seat".
  3. Try an occupied seat. 4. Continue.
- **Expected:** The seat map shows available, occupied and extra-legroom seats with a legend; the
  chosen seat is confirmed ("saved") with any price; occupied seats cannot be chosen; Continue
  goes to `/booking/extras`. Skipping seats is also allowed.
- **Observed:** ____________________
- **Chrome:** ☐ PASS ☐ FAIL  **Firefox:** ☐ PASS ☐ FAIL
- **Notes:**

### 6.2 Extras
- **URL:** `/booking/extras`
- **Steps:** Add one checked bag, then continue.
- **Expected:** The price summary updates; Continue goes to `/booking/review`.
- **Observed:** ____________________
- **Chrome:** ☐ PASS ☐ FAIL  **Firefox:** ☐ PASS ☐ FAIL
- **Notes:**

## 7. Booking review

### 7.1 Review and refresh
- **URL:** `/booking/review`
- **Steps:** 1. Check flight, traveller, seat, extras and total. 2. Press F5. 3. Use "Edit" for
  travellers, change nothing, come back.
- **Expected:** All choices shown with the correct total; after refresh everything is still there
  (state is held by the server session, not only in the browser).
- **Observed:** ____________________
- **Chrome:** ☐ PASS ☐ FAIL  **Firefox:** ☐ PASS ☐ FAIL
- **Notes:**

## 8. Checkout and payment simulation

### 8.1 Checkout holds seats
- **URL:** `/booking/checkout` (also reachable as `/checkout`, a 307 redirect)
- **Steps:** From the review click "Continue to checkout".
- **Expected:** A checkout page with a hold countdown, the order total, and payment methods Demo
  Card, Demo Wallet, Demo Bank Transfer.
- **Observed:** ____________________
- **Chrome:** ☐ PASS ☐ FAIL  **Firefox:** ☐ PASS ☐ FAIL
- **Notes:**

### 8.2 Successful payment
- **URL:** `/booking/checkout`
- **Steps:** Choose Demo Card and pay.
- **Expected:** After a short simulated delay you land on `/booking/confirmation/ord_…`.
- **Observed:** ____________________
- **Chrome:** ☐ PASS ☐ FAIL  **Firefox:** ☐ PASS ☐ FAIL
- **Notes:**

### 8.3 Declined, timed-out and pending payments **[outcome controls]**
- **URL:** `/booking/checkout` (start a new booking each time)
- **Steps:** Choose "Decline" and pay; then "Time out"; then "Pending".
- **Expected:** Decline shows an error and lets you retry (up to 5 attempts); Time out shows a
  clear message, no double charge; Pending shows a waiting state that resolves after about 5 s.
- **Observed:** ____________________
- **Chrome:** ☐ PASS ☐ FAIL ☐ N/A  **Firefox:** ☐ PASS ☐ FAIL ☐ N/A
- **Notes:**

## 9. Booking confirmation

### 9.1 Confirmation page
- **URL:** `/booking/confirmation/ord_…`
- **Steps:** 1. Read the page. 2. Write down the booking reference. 3. Press F5. 4. Open the same
  URL in a different browser (or a private window).
- **Expected:** "Booking confirmed", a reference like `VEL-ABC234`, the itinerary and the amount
  paid. Refresh shows the same page. In another browser the booking is **not** shown (it belongs
  to this browser's guest session).
- **Observed:** ____________________
- **Chrome:** ☐ PASS ☐ FAIL  **Firefox:** ☐ PASS ☐ FAIL
- **Notes:**

## 10. Booking management

### 10.1 Find a booking
- **URL:** `/manage-booking`
- **Steps:** 1. Enter the reference from 9.1 and submit. 2. Then try `VEL-ZZZZZZ`.
- **Expected:** The real reference opens `/manage-booking/ord_…`. The unknown one shows
  "couldn’t open a booking" on the same page.
- **Observed:** ____________________
- **Chrome:** ☐ PASS ☐ FAIL  **Firefox:** ☐ PASS ☐ FAIL
- **Notes:**

### 10.2 Change seats
- **URL:** `/manage-booking/ord_…/seats`
- **Steps:** Choose a different seat and confirm (pay any difference with Demo Card).
- **Expected:** Back on the booking with `?updated=…` and a success notice; the new seat shown.
- **Observed:** ____________________
- **Chrome:** ☐ PASS ☐ FAIL  **Firefox:** ☐ PASS ☐ FAIL
- **Notes:**

### 10.3 Add extras after booking
- **URL:** `/manage-booking/ord_…/extras`
- **Steps:** Add a bag and confirm.
- **Expected:** Success notice; the extra listed on the booking.
- **Observed:** ____________________
- **Chrome:** ☐ PASS ☐ FAIL  **Firefox:** ☐ PASS ☐ FAIL
- **Notes:**

### 10.4 Change flight
- **URL:** `/manage-booking/ord_…/change-flight`
- **Steps:** Pick another date/flight, review the fare difference, confirm.
- **Expected:** The booking shows the new flight; the price difference was charged or refunded.
- **Observed:** ____________________
- **Chrome:** ☐ PASS ☐ FAIL  **Firefox:** ☐ PASS ☐ FAIL
- **Notes:**

### 10.5 Cancel
- **URL:** `/manage-booking/ord_…/cancel`
- **Steps:** Read the refund summary, confirm the cancellation.
- **Expected:** The booking shows as cancelled with the refund; servicing pages for it show it is
  no longer available.
- **Observed:** ____________________
- **Chrome:** ☐ PASS ☐ FAIL  **Firefox:** ☐ PASS ☐ FAIL
- **Notes:**

## 11. Account-related flows

### 11.1 Register a demo account
- **URL:** `/account/register`
- **Steps:** Given names, Family name, a new email (`you+1@example.com`), password twice, tick the
  demo-account box, submit.
- **Expected:** You land on `/account`, signed in, with your name.
- **Observed:** ____________________
- **Chrome:** ☐ PASS ☐ FAIL  **Firefox:** ☐ PASS ☐ FAIL
- **Notes:**

### 11.2 Sign out and sign in
- **URL:** `/account`, `/account/sign-in`
- **Steps:** Sign out; sign in with the same email and password; then try a wrong password.
- **Expected:** Correct credentials open `/account`; a wrong password gives a generic error.
- **Observed:** ____________________
- **Chrome:** ☐ PASS ☐ FAIL  **Firefox:** ☐ PASS ☐ FAIL
- **Notes:**

### 11.3 Trips, saved travellers, settings
- **URL:** `/account/trips`, `/account/travellers`, `/account/settings`
- **Steps:** Signed in: make a booking, then open My trips; add a saved traveller; change a setting.
- **Expected:** The booking appears under trips; the saved traveller is offered on the next
  booking's traveller form; settings are kept after refresh.
- **Observed:** ____________________
- **Chrome:** ☐ PASS ☐ FAIL  **Firefox:** ☐ PASS ☐ FAIL
- **Notes:**

### 11.4 Signed-out access
- **URL:** `/account/trips`
- **Steps:** In a private window, open the URL.
- **Expected:** You are sent to sign in (or told to sign in); no other person's data.
- **Observed:** ____________________
- **Chrome:** ☐ PASS ☐ FAIL  **Firefox:** ☐ PASS ☐ FAIL
- **Notes:**

## 12. Mobile responsive layout

Use DevTools device mode (Chrome: Ctrl+Shift+M; Firefox: Ctrl+Shift+M) at 390×844 and 768×1024.

### 12.1 Mobile menu
- **URL:** `/`
- **Steps:** 1. Tap "Open menu". 2. Tap "Explore destinations". 3. Open the menu again, press Esc.
- **Expected:** The header shows a menu button instead of links; the menu dialog opens and closes;
  focus returns to the button.
- **Observed:** ____________________
- **Chrome:** ☐ PASS ☐ FAIL  **Firefox:** ☐ PASS ☐ FAIL
- **Notes:**

### 12.2 Booking on a phone width
- **URL:** `/` to `/booking/confirmation/…`
- **Steps:** Repeat 2.1 → 8.2 at 390 px width.
- **Expected:** No horizontal scrolling; forms, seat map and price summary usable.
- **Observed:** ____________________
- **Chrome:** ☐ PASS ☐ FAIL  **Firefox:** ☐ PASS ☐ FAIL
- **Notes:**

## 13. Browser refresh and back/forward navigation

### 13.1 Back and forward
- **URL:** `/` → `/destinations` → `/help`
- **Steps:** Navigate with the header, then Back twice and Forward once.
- **Expected:** Each step shows the right page and heading instantly, scroll position restored.
- **Observed:** ____________________
- **Chrome:** ☐ PASS ☐ FAIL  **Firefox:** ☐ PASS ☐ FAIL
- **Notes:**

### 13.2 Back through the booking flow
- **URL:** `/booking/travellers` … `/booking/review`
- **Steps:** At the review, press Back to seats, then to travellers, then Forward.
- **Expected:** Earlier steps show your saved choices; going forward again works; no error page.
- **Observed:** ____________________
- **Chrome:** ☐ PASS ☐ FAIL  **Firefox:** ☐ PASS ☐ FAIL
- **Notes:**

### 13.3 Hard refresh on every booking step
- **URL:** `/booking/travellers`, `/booking/seats`, `/booking/extras`, `/booking/review`, `/booking/checkout`
- **Steps:** During a booking, press F5 (and Ctrl+Shift+R) on each step.
- **Expected:** The same step reloads with its data (status 200).
- **Observed:** ____________________
- **Chrome:** ☐ PASS ☐ FAIL  **Firefox:** ☐ PASS ☐ FAIL
- **Notes:**

## 14. Error handling

### 14.1 Unknown page
- **URL:** `/this-page-does-not-exist`
- **Steps:** Open it; check the status in Network.
- **Expected:** Status **404** and the page "This page has flown off course" with links home.
- **Observed:** ____________________
- **Chrome:** ☐ PASS ☐ FAIL  **Firefox:** ☐ PASS ☐ FAIL
- **Notes:**

### 14.2 Unknown booking and unknown destination
- **URL:** `/manage-booking/ord_AAAAAAAAAAAAAAAAAAAAAA`, `/destinations/atlantis`,
  `/booking/confirmation/ord_AAAAAAAAAAAAAAAAAAAAAA`
- **Steps:** Open each.
- **Expected:** A friendly "not available"/not-found page (404 for the destination); never a raw
  stack trace or an empty white page.
- **Observed:** ____________________
- **Chrome:** ☐ PASS ☐ FAIL  **Firefox:** ☐ PASS ☐ FAIL
- **Notes:**

### 14.3 Booking step without a session
- **URL:** `/booking/seats` in a fresh private window
- **Steps:** Open it directly.
- **Expected:** A message that there is no trip in progress, with a way back to search.
- **Observed:** ____________________
- **Chrome:** ☐ PASS ☐ FAIL  **Firefox:** ☐ PASS ☐ FAIL
- **Notes:**

### 14.4 Retailing API unavailable
- **URL:** `/flights?trip=one-way&from=MNL&to=SIN&depart=YYYY-MM-DD&adults=1&cabin=economy`
- **Steps:** 1. `docker compose -f integration/velasia-airways/compose.yaml stop velasia-api`.
  2. Reload the results. 3. Open `/`. 4. `docker compose -f integration/velasia-airways/compose.yaml start velasia-api`, reload.
- **Expected:** Results show the app's own "flights unavailable" error (not a white page);
  prerendered pages such as `/` still load; after the API is back, results return.
  (The web container's health check stays healthy; `/api/internal/health/ready` reports the API down.)
- **Observed:** ____________________
- **Chrome:** ☐ PASS ☐ FAIL  **Firefox:** ☐ PASS ☐ FAIL
- **Notes:**

---

**Tester:** ________ **Date:** ________ **Chrome version:** ________ **Firefox version:** ________
**Image:** `docker image inspect nextrs-velasia-web:local --format '{{.Id}}'` → ________
