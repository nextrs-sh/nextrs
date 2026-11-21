# Smoke test summary

Generated 2026-11-20T07:55:29.650Z from /home/dennispaler/Documents/OpenSource/nextrs/integration/velasia-airways/results/smoke/results.json

| Browser | PASS | FAIL | BLOCKED | NOT TESTED |
|---|---|---|---|---|
| chromium | 33 | 0 | 0 | 0 |
| firefox | 33 | 0 | 0 | 0 |

## chromium

| Result | Area | Test | Time | Detail |
|---|---|---|---|---|
| PASS | runtime and delivery | homepage renders server HTML that is the Velasia app | 0.1s |  |
| PASS | runtime and delivery | HTML, CSS and JavaScript are delivered and applied | 1.2s |  |
| PASS | runtime and delivery | homepage hydrates without hydration or console errors | 1.1s |  |
| PASS | runtime and delivery | public images and next/image optimisation | 0.8s |  |
| PASS | runtime and delivery | metadata routes: robots.txt, sitemap.xml, manifest | 0.1s |  |
| PASS | runtime and delivery | security headers from next.config are sent | 0.1s |  |
| PASS | runtime and delivery | cache-control: static homepage cacheable, personal pages private | 0.1s |  |
| PASS | routing | client-side navigation uses RSC payloads, no full reloads | 1.6s |  |
| PASS | routing | dynamic route: destination detail, then refresh of the nested route | 0.9s |  |
| PASS | routing | browser back and forward navigation | 0.9s |  |
| PASS | routing | unknown page: real 404 status with the app not-found page | 0.3s |  |
| PASS | routing | redirects from next.config | 0.1s |  |
| PASS | routing | mobile layout: menu dialog navigation | 1.0s |  |
| PASS | API route handlers (via NextRS, which calls the retailing API) | reference data: airports, destinations, fare families | 0.1s |  |
| PASS | API route handlers (via NextRS, which calls the retailing API) | live flight search JSON is private and has results | 0.1s |  |
| PASS | API route handlers (via NextRS, which calls the retailing API) | health endpoints | 0.1s |  |
| PASS | API route handlers (via NextRS, which calls the retailing API) | invalid search returns a client error, not a crash | 0.1s |  |
| PASS | flight search and results | search form from the homepage shows results from the API | 1.0s |  |
| PASS | flight search and results | results filter (stops) and nearby-dates navigation | 0.7s |  |
| PASS | flight search and results | invalid search shows the app error, not a crash | 0.4s |  |
| PASS | booking journey (Server Actions, PPR, cookies, retailing API writes) | select a flight and fare | 2.1s |  |
| PASS | booking journey (Server Actions, PPR, cookies, retailing API writes) | passenger information | 0.6s |  |
| PASS | booking journey (Server Actions, PPR, cookies, retailing API writes) | seat selection | 0.6s |  |
| PASS | booking journey (Server Actions, PPR, cookies, retailing API writes) | extras, then booking review | 0.7s |  |
| PASS | booking journey (Server Actions, PPR, cookies, retailing API writes) | refresh of the review keeps the booking state (server-side session) | 26.6s |  |
| PASS | booking journey (Server Actions, PPR, cookies, retailing API writes) | checkout: the API holds the seats | 0.6s |  |
| PASS | booking journey (Server Actions, PPR, cookies, retailing API writes) | payment simulation and confirmation | 1.1s |  |
| PASS | booking journey (Server Actions, PPR, cookies, retailing API writes) | booking management: look up by reference | 0.7s |  |
| PASS | booking journey (Server Actions, PPR, cookies, retailing API writes) | confirmation page survives a hard refresh | 0.5s |  |
| PASS | booking journey (Server Actions, PPR, cookies, retailing API writes) | no browser errors during the whole journey | 0.1s |  |
| PASS | Server Actions | unknown reference: action runs and shows the app error | 0.4s |  |
| PASS | Server Actions | cross-site action request is refused | 0.4s |  |
| PASS | account flows | register a demo account, sign out state, sign in | 1.2s |  |

## firefox

| Result | Area | Test | Time | Detail |
|---|---|---|---|---|
| PASS | runtime and delivery | homepage renders server HTML that is the Velasia app | 0.8s |  |
| PASS | runtime and delivery | HTML, CSS and JavaScript are delivered and applied | 1.3s |  |
| PASS | runtime and delivery | homepage hydrates without hydration or console errors | 1.1s |  |
| PASS | runtime and delivery | public images and next/image optimisation | 0.8s |  |
| PASS | runtime and delivery | metadata routes: robots.txt, sitemap.xml, manifest | 0.2s |  |
| PASS | runtime and delivery | security headers from next.config are sent | 0.3s |  |
| PASS | runtime and delivery | cache-control: static homepage cacheable, personal pages private | 0.3s |  |
| PASS | routing | client-side navigation uses RSC payloads, no full reloads | 1.9s |  |
| PASS | routing | dynamic route: destination detail, then refresh of the nested route | 1.2s |  |
| PASS | routing | browser back and forward navigation | 1.8s |  |
| PASS | routing | unknown page: real 404 status with the app not-found page | 0.5s |  |
| PASS | routing | redirects from next.config | 0.2s |  |
| PASS | routing | mobile layout: menu dialog navigation | 1.4s |  |
| PASS | API route handlers (via NextRS, which calls the retailing API) | reference data: airports, destinations, fare families | 0.2s |  |
| PASS | API route handlers (via NextRS, which calls the retailing API) | live flight search JSON is private and has results | 0.2s |  |
| PASS | API route handlers (via NextRS, which calls the retailing API) | health endpoints | 0.2s |  |
| PASS | API route handlers (via NextRS, which calls the retailing API) | invalid search returns a client error, not a crash | 0.2s |  |
| PASS | flight search and results | search form from the homepage shows results from the API | 1.9s |  |
| PASS | flight search and results | results filter (stops) and nearby-dates navigation | 1.6s |  |
| PASS | flight search and results | invalid search shows the app error, not a crash | 0.7s |  |
| PASS | booking journey (Server Actions, PPR, cookies, retailing API writes) | select a flight and fare | 2.5s |  |
| PASS | booking journey (Server Actions, PPR, cookies, retailing API writes) | passenger information | 0.6s |  |
| PASS | booking journey (Server Actions, PPR, cookies, retailing API writes) | seat selection | 0.7s |  |
| PASS | booking journey (Server Actions, PPR, cookies, retailing API writes) | extras, then booking review | 0.7s |  |
| PASS | booking journey (Server Actions, PPR, cookies, retailing API writes) | refresh of the review keeps the booking state (server-side session) | 1.3s |  |
| PASS | booking journey (Server Actions, PPR, cookies, retailing API writes) | checkout: the API holds the seats | 0.5s |  |
| PASS | booking journey (Server Actions, PPR, cookies, retailing API writes) | payment simulation and confirmation | 1.4s |  |
| PASS | booking journey (Server Actions, PPR, cookies, retailing API writes) | booking management: look up by reference | 1.2s |  |
| PASS | booking journey (Server Actions, PPR, cookies, retailing API writes) | confirmation page survives a hard refresh | 1.1s |  |
| PASS | booking journey (Server Actions, PPR, cookies, retailing API writes) | no browser errors during the whole journey | 0.3s |  |
| PASS | Server Actions | unknown reference: action runs and shows the app error | 0.6s |  |
| PASS | Server Actions | cross-site action request is refused | 0.5s |  |
| PASS | account flows | register a demo account, sign out state, sign in | 1.8s |  |

## Known app-level console messages (not failures)

- zod v4 `Function("")` eval probe blocked by Velasia's CSP (logged by Firefox only): **19** occurrence(s) annotated. Client-side behaviour of the `next build` output; independent of the server runtime.
