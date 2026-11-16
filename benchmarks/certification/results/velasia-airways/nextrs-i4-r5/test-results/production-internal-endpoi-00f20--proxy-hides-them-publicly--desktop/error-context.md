# Instructions

- Following Playwright test failed.
- Explain why, be concise, respect Playwright best practices.
- Provide a snippet of code with the fix, if possible.

# Test info

- Name: production.spec.ts >> internal endpoints >> liveness, readiness, and metrics answer (the proxy hides them publicly)
- Location: tests/e2e/production.spec.ts:123:3

# Error details

```
Error: expect(received).not.toMatch(expected)

Expected pattern: not /ord_|ses_|VEL-/
Received string:      "# HELP process_resident_memory_bytes Resident set size.
# TYPE process_resident_memory_bytes gauge
process_resident_memory_bytes{service=\"web\"} 141312000
# HELP nodejs_heap_bytes V8 heap, used and total.
# TYPE nodejs_heap_bytes gauge
nodejs_heap_bytes{service=\"web\",kind=\"used\"} 77586544
nodejs_heap_bytes{service=\"web\",kind=\"total\"} 141463552
nodejs_heap_bytes{service=\"web\",kind=\"external\"} 3153780
nodejs_heap_bytes{service=\"web\",kind=\"array_buffers\"} 0
# HELP process_cpu_seconds_total User and system CPU time spent (a counter, exposed as a gauge for simplicity).
# TYPE process_cpu_seconds_total gauge
process_cpu_seconds_total{service=\"web\",mode=\"user\"} 43.007281
process_cpu_seconds_total{service=\"web\",mode=\"system\"} 3.599496
# HELP nodejs_eventloop_delay_seconds Event-loop delay since the previous scrape (p50, p99, max).
# TYPE nodejs_eventloop_delay_seconds gauge
nodejs_eventloop_delay_seconds{service=\"web\",quantile=\"0.5\"} 0.01018817
nodejs_eventloop_delay_seconds{service=\"web\",quantile=\"0.99\"} 0.02015988
nodejs_eventloop_delay_seconds{service=\"web\",quantile=\"1\"} 0.13280009
# HELP process_start_time_seconds When the process started (Unix seconds).
# TYPE process_start_time_seconds gauge
process_start_time_seconds{service=\"web\"} 1794828757.372
# HELP velasia_web_upstream_requests_total Retailing API calls from the storefront, by operation, method, and outcome.
# TYPE velasia_web_upstream_requests_total counter
velasia_web_upstream_requests_total{operation=\"/v1/destinations\",method=\"GET\",outcome=\"ok\"} 1
velasia_web_upstream_requests_total{operation=\"/v1/airports\",method=\"GET\",outcome=\"ok\"} 20
velasia_web_upstream_requests_total{operation=\"/v1/offers\",method=\"GET\",outcome=\"ok\"} 43
velasia_web_upstream_requests_total{operation=\"/v1/fare-families\",method=\"GET\",outcome=\"ok\"} 1
velasia_web_upstream_requests_total{operation=\"/v1/shopping-sessions\",method=\"POST\",outcome=\"ok\"} 29
velasia_web_upstream_requests_total{operation=\"/v1/shopping-sessions/:id/select\",method=\"POST\",outcome=\"ok\"} 37
velasia_web_upstream_requests_total{operation=\"/v1/shopping-sessions/:id/reprice\",method=\"POST\",outcome=\"ok\"} 28
velasia_web_upstream_requests_total{operation=\"/v1/shopping-sessions/:id\",method=\"GET\",outcome=\"ok\"} 87
velasia_web_upstream_requests_total{operation=\"/v1/shopping-sessions/:id/passengers\",method=\"PUT\",outcome=\"ok\"} 22
velasia_web_upstream_requests_total{operation=\"/v1/shopping-sessions/:id/seats\",method=\"PUT\",outcome=\"ok\"} 23
velasia_web_upstream_requests_total{operation=\"/v1/shopping-sessions/:id/seatmaps/:id\",method=\"GET\",outcome=\"ok\"} 27
velasia_web_upstream_requests_total{operation=\"/v1/shopping-sessions/:id/steps\",method=\"POST\",outcome=\"ok\"} 42
velasia_web_upstream_requests_total{operation=\"/v1/shopping-sessions/:id/ancillaries\",method=\"PUT\",outcome=\"ok\"} 5
velasia_web_upstream_requests_total{operation=\"/v1/shopping-sessions/:id/ancillaries\",method=\"GET\",outcome=\"ok\"} 5
velasia_web_upstream_requests_total{operation=\"/v1/shopping-sessions/:id/validate\",method=\"POST\",outcome=\"ok\"} 23
velasia_web_upstream_requests_total{operation=\"/v1/shopping-sessions/:id/review\",method=\"GET\",outcome=\"ok\"} 5
velasia_web_upstream_requests_total{operation=\"/v1/checkout/prepare\",method=\"POST\",outcome=\"ok\"} 18
velasia_web_upstream_requests_total{operation=\"/v1/shopping-sessions/:id/seats\",method=\"PUT\",outcome=\"timeout\"} 1
velasia_web_upstream_requests_total{operation=\"/v1/shopping-sessions/:id\",method=\"GET\",outcome=\"4xx\"} 2
velasia_web_upstream_requests_total{operation=\"/v1/destinations/cebu\",method=\"GET\",outcome=\"ok\"} 1
velasia_web_upstream_requests_total{operation=\"/v1/flights/calendar\",method=\"GET\",outcome=\"ok\"} 39
velasia_web_upstream_requests_total{operation=\"/v1/flights\",method=\"GET\",outcome=\"ok\"} 42
velasia_web_upstream_requests_total{operation=\"/v1/flights/calendar\",method=\"GET\",outcome=\"5xx\"} 2
velasia_web_upstream_requests_total{operation=\"/v1/flights\",method=\"GET\",outcome=\"5xx\"} 2
velasia_web_upstream_requests_total{operation=\"/v1/orders/:id/payment-attempts\",method=\"POST\",outcome=\"ok\"} 12
velasia_web_upstream_requests_total{operation=\"/v1/orders/:id\",method=\"GET\",outcome=\"ok\"} 8
velasia_web_upstream_requests_total{operation=\"/v1/payments/options\",method=\"GET\",outcome=\"ok\"} 18
velasia_web_upstream_requests_total{operation=\"/v1/orders/:id/payment-status\",method=\"GET\",outcome=\"ok\"} 2
velasia_web_upstream_requests_total{operation=\"/v1/checkout/prepare\",method=\"POST\",outcome=\"4xx\"} 1
velasia_web_upstream_requests_total{operation=\"/v1/destinations/tokyo\",method=\"GET\",outcome=\"ok\"} 1
velasia_web_upstream_requests_total{operation=\"/v1/destinations/atlantis\",method=\"GET\",outcome=\"4xx\"} 1
velasia_web_upstream_requests_total{operation=\"/v1/flights/:id/availability\",method=\"GET\",outcome=\"ok\"} 3
velasia_web_upstream_requests_total{operation=\"/v1/flights/:id\",method=\"GET\",outcome=\"ok\"} 3
velasia_web_upstream_requests_total{operation=\"/v1/routes\",method=\"GET\",outcome=\"ok\"} 1
velasia_web_upstream_requests_total{operation=\"/v1/guest/orders\",method=\"GET\",outcome=\"ok\"} 3
velasia_web_upstream_requests_total{operation=\"/v1/orders/:id/servicing\",method=\"GET\",outcome=\"ok\"} 15
velasia_web_upstream_requests_total{operation=\"/v1/orders/:id/events\",method=\"GET\",outcome=\"ok\"} 5
velasia_web_upstream_requests_total{operation=\"/v1/orders/:id/change-quotes\",method=\"POST\",outcome=\"ok\"} 4
velasia_web_upstream_requests_total{operation=\"/v1/orders/:id/changes\",method=\"POST\",outcome=\"ok\"} 2
velasia_web_upstream_requests_total{operation=\"/v1/orders/:id/seatmaps/:id\",method=\"GET\",outcome=\"ok\"} 1
velasia_web_upstream_requests_total{operation=\"/v1/orders/:id/seat-quotes\",method=\"POST\",outcome=\"ok\"} 1
velasia_web_upstream_requests_total{operation=\"/v1/orders/:id/seat-changes\",method=\"POST\",outcome=\"ok\"} 1
velasia_web_upstream_requests_total{operation=\"/v1/orders/reference/VEL-SBGEQT\",method=\"GET\",outcome=\"ok\"} 1
velasia_web_upstream_requests_total{operation=\"/v1/ancillaries\",method=\"GET\",outcome=\"ok\"} 3
velasia_web_upstream_requests_total{operation=\"/v1/orders/:id/ancillaries\",method=\"GET\",outcome=\"ok\"} 3
velasia_web_upstream_requests_total{operation=\"/v1/orders/:id/ancillary-quotes\",method=\"POST\",outcome=\"ok\"} 2
velasia_web_upstream_requests_total{operation=\"/v1/orders/:id/ancillary-purchases\",method=\"POST\",outcome=\"ok\"} 2
velasia_web_upstream_requests_total{operation=\"/v1/accounts\",method=\"POST\",outcome=\"ok\"} 1
velasia_web_upstream_requests_total{operation=\"/v1/me\",method=\"GET\",outcome=\"ok\"} 3
velasia_web_upstream_requests_total{operation=\"/v1/me/orders\",method=\"GET\",outcome=\"ok\"} 2
velasia_web_upstream_requests_total{operation=\"/v1/me/orders/link\",method=\"POST\",outcome=\"ok\"} 1
velasia_web_upstream_requests_total{operation=\"/v1/me/travellers\",method=\"GET\",outcome=\"ok\"} 1
velasia_web_upstream_requests_total{operation=\"/v1/me/travellers\",method=\"POST\",outcome=\"ok\"} 1
velasia_web_upstream_requests_total{operation=\"/v1/sessions/current\",method=\"DELETE\",outcome=\"ok\"} 1
velasia_web_upstream_requests_total{operation=\"/v1/orders/:id/servicing\",method=\"GET\",outcome=\"4xx\"} 1
velasia_web_upstream_requests_total{operation=\"/v1/sessions\",method=\"POST\",outcome=\"ok\"} 1
velasia_web_upstream_requests_total{operation=\"/v1/orders/:id/cancellation-quotes\",method=\"POST\",outcome=\"ok\"} 2
velasia_web_upstream_requests_total{operation=\"/v1/orders/:id/cancellations\",method=\"POST\",outcome=\"ok\"} 1
velasia_web_upstream_requests_total{operation=\"/v1/orders/:id/servicing-status\",method=\"GET\",outcome=\"ok\"} 1
velasia_web_upstream_requests_total{operation=\"/v1/orders/:id/servicing/:id/abandon\",method=\"POST\",outcome=\"ok\"} 1
velasia_web_upstream_requests_total{operation=\"/v1/offers\",method=\"GET\",outcome=\"5xx\"} 1
velasia_web_upstream_requests_total{operation=\"/v1/shopping-sessions\",method=\"POST\",outcome=\"5xx\"} 1
velasia_web_upstream_requests_total{operation=\"/v1/offers\",method=\"GET\",outcome=\"timeout\"} 1
# HELP velasia_web_upstream_request_duration_seconds Retailing API call latency as the storefront sees it, by operation.
# TYPE velasia_web_upstream_request_duration_seconds histogram
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/destinations\",method=\"GET\",le=\"0.001\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/destinations\",method=\"GET\",le=\"0.0025\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/destinations\",method=\"GET\",le=\"0.005\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/destinations\",method=\"GET\",le=\"0.01\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/destinations\",method=\"GET\",le=\"0.025\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/destinations\",method=\"GET\",le=\"0.05\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/destinations\",method=\"GET\",le=\"0.1\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/destinations\",method=\"GET\",le=\"0.25\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/destinations\",method=\"GET\",le=\"0.5\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/destinations\",method=\"GET\",le=\"1\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/destinations\",method=\"GET\",le=\"2.5\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/destinations\",method=\"GET\",le=\"5\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/destinations\",method=\"GET\",le=\"10\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/destinations\",method=\"GET\",le=\"+Inf\"} 1
velasia_web_upstream_request_duration_seconds_sum{operation=\"/v1/destinations\",method=\"GET\"} 0.013102824
velasia_web_upstream_request_duration_seconds_count{operation=\"/v1/destinations\",method=\"GET\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/airports\",method=\"GET\",le=\"0.001\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/airports\",method=\"GET\",le=\"0.0025\"} 11
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/airports\",method=\"GET\",le=\"0.005\"} 16
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/airports\",method=\"GET\",le=\"0.01\"} 18
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/airports\",method=\"GET\",le=\"0.025\"} 20
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/airports\",method=\"GET\",le=\"0.05\"} 20
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/airports\",method=\"GET\",le=\"0.1\"} 20
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/airports\",method=\"GET\",le=\"0.25\"} 20
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/airports\",method=\"GET\",le=\"0.5\"} 20
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/airports\",method=\"GET\",le=\"1\"} 20
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/airports\",method=\"GET\",le=\"2.5\"} 20
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/airports\",method=\"GET\",le=\"5\"} 20
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/airports\",method=\"GET\",le=\"10\"} 20
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/airports\",method=\"GET\",le=\"+Inf\"} 20
velasia_web_upstream_request_duration_seconds_sum{operation=\"/v1/airports\",method=\"GET\"} 0.072475979
velasia_web_upstream_request_duration_seconds_count{operation=\"/v1/airports\",method=\"GET\"} 20
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/offers\",method=\"GET\",le=\"0.001\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/offers\",method=\"GET\",le=\"0.0025\"} 9
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/offers\",method=\"GET\",le=\"0.005\"} 27
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/offers\",method=\"GET\",le=\"0.01\"} 39
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/offers\",method=\"GET\",le=\"0.025\"} 44
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/offers\",method=\"GET\",le=\"0.05\"} 44
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/offers\",method=\"GET\",le=\"0.1\"} 44
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/offers\",method=\"GET\",le=\"0.25\"} 44
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/offers\",method=\"GET\",le=\"0.5\"} 44
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/offers\",method=\"GET\",le=\"1\"} 44
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/offers\",method=\"GET\",le=\"2.5\"} 44
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/offers\",method=\"GET\",le=\"5\"} 45
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/offers\",method=\"GET\",le=\"10\"} 45
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/offers\",method=\"GET\",le=\"+Inf\"} 45
velasia_web_upstream_request_duration_seconds_sum{operation=\"/v1/offers\",method=\"GET\"} 2.7397064339999995
velasia_web_upstream_request_duration_seconds_count{operation=\"/v1/offers\",method=\"GET\"} 45
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/fare-families\",method=\"GET\",le=\"0.001\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/fare-families\",method=\"GET\",le=\"0.0025\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/fare-families\",method=\"GET\",le=\"0.005\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/fare-families\",method=\"GET\",le=\"0.01\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/fare-families\",method=\"GET\",le=\"0.025\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/fare-families\",method=\"GET\",le=\"0.05\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/fare-families\",method=\"GET\",le=\"0.1\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/fare-families\",method=\"GET\",le=\"0.25\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/fare-families\",method=\"GET\",le=\"0.5\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/fare-families\",method=\"GET\",le=\"1\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/fare-families\",method=\"GET\",le=\"2.5\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/fare-families\",method=\"GET\",le=\"5\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/fare-families\",method=\"GET\",le=\"10\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/fare-families\",method=\"GET\",le=\"+Inf\"} 1
velasia_web_upstream_request_duration_seconds_sum{operation=\"/v1/fare-families\",method=\"GET\"} 0.015138623
velasia_web_upstream_request_duration_seconds_count{operation=\"/v1/fare-families\",method=\"GET\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions\",method=\"POST\",le=\"0.001\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions\",method=\"POST\",le=\"0.0025\"} 13
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions\",method=\"POST\",le=\"0.005\"} 25
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions\",method=\"POST\",le=\"0.01\"} 28
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions\",method=\"POST\",le=\"0.025\"} 30
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions\",method=\"POST\",le=\"0.05\"} 30
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions\",method=\"POST\",le=\"0.1\"} 30
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions\",method=\"POST\",le=\"0.25\"} 30
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions\",method=\"POST\",le=\"0.5\"} 30
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions\",method=\"POST\",le=\"1\"} 30
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions\",method=\"POST\",le=\"2.5\"} 30
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions\",method=\"POST\",le=\"5\"} 30
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions\",method=\"POST\",le=\"10\"} 30
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions\",method=\"POST\",le=\"+Inf\"} 30
velasia_web_upstream_request_duration_seconds_sum{operation=\"/v1/shopping-sessions\",method=\"POST\"} 0.11782928699999999
velasia_web_upstream_request_duration_seconds_count{operation=\"/v1/shopping-sessions\",method=\"POST\"} 30
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/select\",method=\"POST\",le=\"0.001\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/select\",method=\"POST\",le=\"0.0025\"} 25
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/select\",method=\"POST\",le=\"0.005\"} 34
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/select\",method=\"POST\",le=\"0.01\"} 37
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/select\",method=\"POST\",le=\"0.025\"} 37
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/select\",method=\"POST\",le=\"0.05\"} 37
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/select\",method=\"POST\",le=\"0.1\"} 37
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/select\",method=\"POST\",le=\"0.25\"} 37
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/select\",method=\"POST\",le=\"0.5\"} 37
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/select\",method=\"POST\",le=\"1\"} 37
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/select\",method=\"POST\",le=\"2.5\"} 37
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/select\",method=\"POST\",le=\"5\"} 37
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/select\",method=\"POST\",le=\"10\"} 37
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/select\",method=\"POST\",le=\"+Inf\"} 37
velasia_web_upstream_request_duration_seconds_sum{operation=\"/v1/shopping-sessions/:id/select\",method=\"POST\"} 0.092949103
velasia_web_upstream_request_duration_seconds_count{operation=\"/v1/shopping-sessions/:id/select\",method=\"POST\"} 37
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/reprice\",method=\"POST\",le=\"0.001\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/reprice\",method=\"POST\",le=\"0.0025\"} 19
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/reprice\",method=\"POST\",le=\"0.005\"} 28
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/reprice\",method=\"POST\",le=\"0.01\"} 28
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/reprice\",method=\"POST\",le=\"0.025\"} 28
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/reprice\",method=\"POST\",le=\"0.05\"} 28
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/reprice\",method=\"POST\",le=\"0.1\"} 28
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/reprice\",method=\"POST\",le=\"0.25\"} 28
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/reprice\",method=\"POST\",le=\"0.5\"} 28
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/reprice\",method=\"POST\",le=\"1\"} 28
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/reprice\",method=\"POST\",le=\"2.5\"} 28
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/reprice\",method=\"POST\",le=\"5\"} 28
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/reprice\",method=\"POST\",le=\"10\"} 28
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/reprice\",method=\"POST\",le=\"+Inf\"} 28
velasia_web_upstream_request_duration_seconds_sum{operation=\"/v1/shopping-sessions/:id/reprice\",method=\"POST\"} 0.05645362500000001
velasia_web_upstream_request_duration_seconds_count{operation=\"/v1/shopping-sessions/:id/reprice\",method=\"POST\"} 28
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id\",method=\"GET\",le=\"0.001\"} 2
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id\",method=\"GET\",le=\"0.0025\"} 45
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id\",method=\"GET\",le=\"0.005\"} 70
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id\",method=\"GET\",le=\"0.01\"} 87
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id\",method=\"GET\",le=\"0.025\"} 89
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id\",method=\"GET\",le=\"0.05\"} 89
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id\",method=\"GET\",le=\"0.1\"} 89
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id\",method=\"GET\",le=\"0.25\"} 89
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id\",method=\"GET\",le=\"0.5\"} 89
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id\",method=\"GET\",le=\"1\"} 89
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id\",method=\"GET\",le=\"2.5\"} 89
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id\",method=\"GET\",le=\"5\"} 89
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id\",method=\"GET\",le=\"10\"} 89
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id\",method=\"GET\",le=\"+Inf\"} 89
velasia_web_upstream_request_duration_seconds_sum{operation=\"/v1/shopping-sessions/:id\",method=\"GET\"} 0.297994427
velasia_web_upstream_request_duration_seconds_count{operation=\"/v1/shopping-sessions/:id\",method=\"GET\"} 89
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/passengers\",method=\"PUT\",le=\"0.001\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/passengers\",method=\"PUT\",le=\"0.0025\"} 10
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/passengers\",method=\"PUT\",le=\"0.005\"} 21
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/passengers\",method=\"PUT\",le=\"0.01\"} 22
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/passengers\",method=\"PUT\",le=\"0.025\"} 22
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/passengers\",method=\"PUT\",le=\"0.05\"} 22
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/passengers\",method=\"PUT\",le=\"0.1\"} 22
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/passengers\",method=\"PUT\",le=\"0.25\"} 22
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/passengers\",method=\"PUT\",le=\"0.5\"} 22
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/passengers\",method=\"PUT\",le=\"1\"} 22
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/passengers\",method=\"PUT\",le=\"2.5\"} 22
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/passengers\",method=\"PUT\",le=\"5\"} 22
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/passengers\",method=\"PUT\",le=\"10\"} 22
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/passengers\",method=\"PUT\",le=\"+Inf\"} 22
velasia_web_upstream_request_duration_seconds_sum{operation=\"/v1/shopping-sessions/:id/passengers\",method=\"PUT\"} 0.061586145
velasia_web_upstream_request_duration_seconds_count{operation=\"/v1/shopping-sessions/:id/passengers\",method=\"PUT\"} 22
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/seats\",method=\"PUT\",le=\"0.001\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/seats\",method=\"PUT\",le=\"0.0025\"} 14
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/seats\",method=\"PUT\",le=\"0.005\"} 20
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/seats\",method=\"PUT\",le=\"0.01\"} 23
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/seats\",method=\"PUT\",le=\"0.025\"} 23
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/seats\",method=\"PUT\",le=\"0.05\"} 23
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/seats\",method=\"PUT\",le=\"0.1\"} 23
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/seats\",method=\"PUT\",le=\"0.25\"} 23
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/seats\",method=\"PUT\",le=\"0.5\"} 23
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/seats\",method=\"PUT\",le=\"1\"} 23
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/seats\",method=\"PUT\",le=\"2.5\"} 23
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/seats\",method=\"PUT\",le=\"5\"} 24
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/seats\",method=\"PUT\",le=\"10\"} 24
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/seats\",method=\"PUT\",le=\"+Inf\"} 24
velasia_web_upstream_request_duration_seconds_sum{operation=\"/v1/shopping-sessions/:id/seats\",method=\"PUT\"} 2.567180313
velasia_web_upstream_request_duration_seconds_count{operation=\"/v1/shopping-sessions/:id/seats\",method=\"PUT\"} 24
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/seatmaps/:id\",method=\"GET\",le=\"0.001\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/seatmaps/:id\",method=\"GET\",le=\"0.0025\"} 16
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/seatmaps/:id\",method=\"GET\",le=\"0.005\"} 26
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/seatmaps/:id\",method=\"GET\",le=\"0.01\"} 27
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/seatmaps/:id\",method=\"GET\",le=\"0.025\"} 27
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/seatmaps/:id\",method=\"GET\",le=\"0.05\"} 27
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/seatmaps/:id\",method=\"GET\",le=\"0.1\"} 27
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/seatmaps/:id\",method=\"GET\",le=\"0.25\"} 27
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/seatmaps/:id\",method=\"GET\",le=\"0.5\"} 27
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/seatmaps/:id\",method=\"GET\",le=\"1\"} 27
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/seatmaps/:id\",method=\"GET\",le=\"2.5\"} 27
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/seatmaps/:id\",method=\"GET\",le=\"5\"} 27
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/seatmaps/:id\",method=\"GET\",le=\"10\"} 27
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/seatmaps/:id\",method=\"GET\",le=\"+Inf\"} 27
velasia_web_upstream_request_duration_seconds_sum{operation=\"/v1/shopping-sessions/:id/seatmaps/:id\",method=\"GET\"} 0.071856398
velasia_web_upstream_request_duration_seconds_count{operation=\"/v1/shopping-sessions/:id/seatmaps/:id\",method=\"GET\"} 27
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/steps\",method=\"POST\",le=\"0.001\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/steps\",method=\"POST\",le=\"0.0025\"} 31
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/steps\",method=\"POST\",le=\"0.005\"} 40
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/steps\",method=\"POST\",le=\"0.01\"} 42
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/steps\",method=\"POST\",le=\"0.025\"} 42
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/steps\",method=\"POST\",le=\"0.05\"} 42
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/steps\",method=\"POST\",le=\"0.1\"} 42
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/steps\",method=\"POST\",le=\"0.25\"} 42
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/steps\",method=\"POST\",le=\"0.5\"} 42
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/steps\",method=\"POST\",le=\"1\"} 42
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/steps\",method=\"POST\",le=\"2.5\"} 42
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/steps\",method=\"POST\",le=\"5\"} 42
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/steps\",method=\"POST\",le=\"10\"} 42
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/steps\",method=\"POST\",le=\"+Inf\"} 42
velasia_web_upstream_request_duration_seconds_sum{operation=\"/v1/shopping-sessions/:id/steps\",method=\"POST\"} 0.09288272600000003
velasia_web_upstream_request_duration_seconds_count{operation=\"/v1/shopping-sessions/:id/steps\",method=\"POST\"} 42
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/ancillaries\",method=\"PUT\",le=\"0.001\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/ancillaries\",method=\"PUT\",le=\"0.0025\"} 2
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/ancillaries\",method=\"PUT\",le=\"0.005\"} 4
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/ancillaries\",method=\"PUT\",le=\"0.01\"} 5
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/ancillaries\",method=\"PUT\",le=\"0.025\"} 5
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/ancillaries\",method=\"PUT\",le=\"0.05\"} 5
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/ancillaries\",method=\"PUT\",le=\"0.1\"} 5
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/ancillaries\",method=\"PUT\",le=\"0.25\"} 5
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/ancillaries\",method=\"PUT\",le=\"0.5\"} 5
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/ancillaries\",method=\"PUT\",le=\"1\"} 5
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/ancillaries\",method=\"PUT\",le=\"2.5\"} 5
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/ancillaries\",method=\"PUT\",le=\"5\"} 5
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/ancillaries\",method=\"PUT\",le=\"10\"} 5
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/ancillaries\",method=\"PUT\",le=\"+Inf\"} 5
velasia_web_upstream_request_duration_seconds_sum{operation=\"/v1/shopping-sessions/:id/ancillaries\",method=\"PUT\"} 0.017100244
velasia_web_upstream_request_duration_seconds_count{operation=\"/v1/shopping-sessions/:id/ancillaries\",method=\"PUT\"} 5
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/ancillaries\",method=\"GET\",le=\"0.001\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/ancillaries\",method=\"GET\",le=\"0.0025\"} 4
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/ancillaries\",method=\"GET\",le=\"0.005\"} 5
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/ancillaries\",method=\"GET\",le=\"0.01\"} 5
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/ancillaries\",method=\"GET\",le=\"0.025\"} 5
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/ancillaries\",method=\"GET\",le=\"0.05\"} 5
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/ancillaries\",method=\"GET\",le=\"0.1\"} 5
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/ancillaries\",method=\"GET\",le=\"0.25\"} 5
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/ancillaries\",method=\"GET\",le=\"0.5\"} 5
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/ancillaries\",method=\"GET\",le=\"1\"} 5
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/ancillaries\",method=\"GET\",le=\"2.5\"} 5
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/ancillaries\",method=\"GET\",le=\"5\"} 5
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/ancillaries\",method=\"GET\",le=\"10\"} 5
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/ancillaries\",method=\"GET\",le=\"+Inf\"} 5
velasia_web_upstream_request_duration_seconds_sum{operation=\"/v1/shopping-sessions/:id/ancillaries\",method=\"GET\"} 0.010012776
velasia_web_upstream_request_duration_seconds_count{operation=\"/v1/shopping-sessions/:id/ancillaries\",method=\"GET\"} 5
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/validate\",method=\"POST\",le=\"0.001\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/validate\",method=\"POST\",le=\"0.0025\"} 5
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/validate\",method=\"POST\",le=\"0.005\"} 20
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/validate\",method=\"POST\",le=\"0.01\"} 23
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/validate\",method=\"POST\",le=\"0.025\"} 23
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/validate\",method=\"POST\",le=\"0.05\"} 23
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/validate\",method=\"POST\",le=\"0.1\"} 23
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/validate\",method=\"POST\",le=\"0.25\"} 23
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/validate\",method=\"POST\",le=\"0.5\"} 23
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/validate\",method=\"POST\",le=\"1\"} 23
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/validate\",method=\"POST\",le=\"2.5\"} 23
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/validate\",method=\"POST\",le=\"5\"} 23
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/validate\",method=\"POST\",le=\"10\"} 23
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/validate\",method=\"POST\",le=\"+Inf\"} 23
velasia_web_upstream_request_duration_seconds_sum{operation=\"/v1/shopping-sessions/:id/validate\",method=\"POST\"} 0.08088875599999999
velasia_web_upstream_request_duration_seconds_count{operation=\"/v1/shopping-sessions/:id/validate\",method=\"POST\"} 23
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/review\",method=\"GET\",le=\"0.001\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/review\",method=\"GET\",le=\"0.0025\"} 4
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/review\",method=\"GET\",le=\"0.005\"} 5
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/review\",method=\"GET\",le=\"0.01\"} 5
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/review\",method=\"GET\",le=\"0.025\"} 5
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/review\",method=\"GET\",le=\"0.05\"} 5
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/review\",method=\"GET\",le=\"0.1\"} 5
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/review\",method=\"GET\",le=\"0.25\"} 5
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/review\",method=\"GET\",le=\"0.5\"} 5
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/review\",method=\"GET\",le=\"1\"} 5
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/review\",method=\"GET\",le=\"2.5\"} 5
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/review\",method=\"GET\",le=\"5\"} 5
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/review\",method=\"GET\",le=\"10\"} 5
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/shopping-sessions/:id/review\",method=\"GET\",le=\"+Inf\"} 5
velasia_web_upstream_request_duration_seconds_sum{operation=\"/v1/shopping-sessions/:id/review\",method=\"GET\"} 0.011902655000000002
velasia_web_upstream_request_duration_seconds_count{operation=\"/v1/shopping-sessions/:id/review\",method=\"GET\"} 5
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/checkout/prepare\",method=\"POST\",le=\"0.001\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/checkout/prepare\",method=\"POST\",le=\"0.0025\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/checkout/prepare\",method=\"POST\",le=\"0.005\"} 4
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/checkout/prepare\",method=\"POST\",le=\"0.01\"} 11
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/checkout/prepare\",method=\"POST\",le=\"0.025\"} 19
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/checkout/prepare\",method=\"POST\",le=\"0.05\"} 19
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/checkout/prepare\",method=\"POST\",le=\"0.1\"} 19
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/checkout/prepare\",method=\"POST\",le=\"0.25\"} 19
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/checkout/prepare\",method=\"POST\",le=\"0.5\"} 19
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/checkout/prepare\",method=\"POST\",le=\"1\"} 19
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/checkout/prepare\",method=\"POST\",le=\"2.5\"} 19
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/checkout/prepare\",method=\"POST\",le=\"5\"} 19
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/checkout/prepare\",method=\"POST\",le=\"10\"} 19
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/checkout/prepare\",method=\"POST\",le=\"+Inf\"} 19
velasia_web_upstream_request_duration_seconds_sum{operation=\"/v1/checkout/prepare\",method=\"POST\"} 0.19727452700000003
velasia_web_upstream_request_duration_seconds_count{operation=\"/v1/checkout/prepare\",method=\"POST\"} 19
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/destinations/cebu\",method=\"GET\",le=\"0.001\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/destinations/cebu\",method=\"GET\",le=\"0.0025\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/destinations/cebu\",method=\"GET\",le=\"0.005\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/destinations/cebu\",method=\"GET\",le=\"0.01\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/destinations/cebu\",method=\"GET\",le=\"0.025\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/destinations/cebu\",method=\"GET\",le=\"0.05\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/destinations/cebu\",method=\"GET\",le=\"0.1\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/destinations/cebu\",method=\"GET\",le=\"0.25\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/destinations/cebu\",method=\"GET\",le=\"0.5\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/destinations/cebu\",method=\"GET\",le=\"1\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/destinations/cebu\",method=\"GET\",le=\"2.5\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/destinations/cebu\",method=\"GET\",le=\"5\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/destinations/cebu\",method=\"GET\",le=\"10\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/destinations/cebu\",method=\"GET\",le=\"+Inf\"} 1
velasia_web_upstream_request_duration_seconds_sum{operation=\"/v1/destinations/cebu\",method=\"GET\"} 0.001445768
velasia_web_upstream_request_duration_seconds_count{operation=\"/v1/destinations/cebu\",method=\"GET\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/flights/calendar\",method=\"GET\",le=\"0.001\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/flights/calendar\",method=\"GET\",le=\"0.0025\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/flights/calendar\",method=\"GET\",le=\"0.005\"} 10
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/flights/calendar\",method=\"GET\",le=\"0.01\"} 29
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/flights/calendar\",method=\"GET\",le=\"0.025\"} 40
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/flights/calendar\",method=\"GET\",le=\"0.05\"} 40
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/flights/calendar\",method=\"GET\",le=\"0.1\"} 40
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/flights/calendar\",method=\"GET\",le=\"0.25\"} 40
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/flights/calendar\",method=\"GET\",le=\"0.5\"} 40
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/flights/calendar\",method=\"GET\",le=\"1\"} 40
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/flights/calendar\",method=\"GET\",le=\"2.5\"} 41
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/flights/calendar\",method=\"GET\",le=\"5\"} 41
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/flights/calendar\",method=\"GET\",le=\"10\"} 41
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/flights/calendar\",method=\"GET\",le=\"+Inf\"} 41
velasia_web_upstream_request_duration_seconds_sum{operation=\"/v1/flights/calendar\",method=\"GET\"} 1.8115299950000001
velasia_web_upstream_request_duration_seconds_count{operation=\"/v1/flights/calendar\",method=\"GET\"} 41
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/flights\",method=\"GET\",le=\"0.001\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/flights\",method=\"GET\",le=\"0.0025\"} 3
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/flights\",method=\"GET\",le=\"0.005\"} 12
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/flights\",method=\"GET\",le=\"0.01\"} 32
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/flights\",method=\"GET\",le=\"0.025\"} 43
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/flights\",method=\"GET\",le=\"0.05\"} 43
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/flights\",method=\"GET\",le=\"0.1\"} 43
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/flights\",method=\"GET\",le=\"0.25\"} 43
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/flights\",method=\"GET\",le=\"0.5\"} 43
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/flights\",method=\"GET\",le=\"1\"} 43
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/flights\",method=\"GET\",le=\"2.5\"} 44
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/flights\",method=\"GET\",le=\"5\"} 44
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/flights\",method=\"GET\",le=\"10\"} 44
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/flights\",method=\"GET\",le=\"+Inf\"} 44
velasia_web_upstream_request_duration_seconds_sum{operation=\"/v1/flights\",method=\"GET\"} 1.8413298859999998
velasia_web_upstream_request_duration_seconds_count{operation=\"/v1/flights\",method=\"GET\"} 44
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/payment-attempts\",method=\"POST\",le=\"0.001\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/payment-attempts\",method=\"POST\",le=\"0.0025\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/payment-attempts\",method=\"POST\",le=\"0.005\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/payment-attempts\",method=\"POST\",le=\"0.01\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/payment-attempts\",method=\"POST\",le=\"0.025\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/payment-attempts\",method=\"POST\",le=\"0.05\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/payment-attempts\",method=\"POST\",le=\"0.1\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/payment-attempts\",method=\"POST\",le=\"0.25\"} 11
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/payment-attempts\",method=\"POST\",le=\"0.5\"} 11
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/payment-attempts\",method=\"POST\",le=\"1\"} 11
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/payment-attempts\",method=\"POST\",le=\"2.5\"} 12
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/payment-attempts\",method=\"POST\",le=\"5\"} 12
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/payment-attempts\",method=\"POST\",le=\"10\"} 12
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/payment-attempts\",method=\"POST\",le=\"+Inf\"} 12
velasia_web_upstream_request_duration_seconds_sum{operation=\"/v1/orders/:id/payment-attempts\",method=\"POST\"} 3.2979274999999997
velasia_web_upstream_request_duration_seconds_count{operation=\"/v1/orders/:id/payment-attempts\",method=\"POST\"} 12
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id\",method=\"GET\",le=\"0.001\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id\",method=\"GET\",le=\"0.0025\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id\",method=\"GET\",le=\"0.005\"} 6
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id\",method=\"GET\",le=\"0.01\"} 7
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id\",method=\"GET\",le=\"0.025\"} 8
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id\",method=\"GET\",le=\"0.05\"} 8
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id\",method=\"GET\",le=\"0.1\"} 8
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id\",method=\"GET\",le=\"0.25\"} 8
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id\",method=\"GET\",le=\"0.5\"} 8
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id\",method=\"GET\",le=\"1\"} 8
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id\",method=\"GET\",le=\"2.5\"} 8
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id\",method=\"GET\",le=\"5\"} 8
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id\",method=\"GET\",le=\"10\"} 8
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id\",method=\"GET\",le=\"+Inf\"} 8
velasia_web_upstream_request_duration_seconds_sum{operation=\"/v1/orders/:id\",method=\"GET\"} 0.051219112
velasia_web_upstream_request_duration_seconds_count{operation=\"/v1/orders/:id\",method=\"GET\"} 8
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/payments/options\",method=\"GET\",le=\"0.001\"} 5
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/payments/options\",method=\"GET\",le=\"0.0025\"} 15
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/payments/options\",method=\"GET\",le=\"0.005\"} 18
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/payments/options\",method=\"GET\",le=\"0.01\"} 18
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/payments/options\",method=\"GET\",le=\"0.025\"} 18
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/payments/options\",method=\"GET\",le=\"0.05\"} 18
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/payments/options\",method=\"GET\",le=\"0.1\"} 18
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/payments/options\",method=\"GET\",le=\"0.25\"} 18
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/payments/options\",method=\"GET\",le=\"0.5\"} 18
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/payments/options\",method=\"GET\",le=\"1\"} 18
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/payments/options\",method=\"GET\",le=\"2.5\"} 18
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/payments/options\",method=\"GET\",le=\"5\"} 18
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/payments/options\",method=\"GET\",le=\"10\"} 18
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/payments/options\",method=\"GET\",le=\"+Inf\"} 18
velasia_web_upstream_request_duration_seconds_sum{operation=\"/v1/payments/options\",method=\"GET\"} 0.034178109
velasia_web_upstream_request_duration_seconds_count{operation=\"/v1/payments/options\",method=\"GET\"} 18
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/payment-status\",method=\"GET\",le=\"0.001\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/payment-status\",method=\"GET\",le=\"0.0025\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/payment-status\",method=\"GET\",le=\"0.005\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/payment-status\",method=\"GET\",le=\"0.01\"} 2
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/payment-status\",method=\"GET\",le=\"0.025\"} 2
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/payment-status\",method=\"GET\",le=\"0.05\"} 2
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/payment-status\",method=\"GET\",le=\"0.1\"} 2
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/payment-status\",method=\"GET\",le=\"0.25\"} 2
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/payment-status\",method=\"GET\",le=\"0.5\"} 2
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/payment-status\",method=\"GET\",le=\"1\"} 2
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/payment-status\",method=\"GET\",le=\"2.5\"} 2
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/payment-status\",method=\"GET\",le=\"5\"} 2
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/payment-status\",method=\"GET\",le=\"10\"} 2
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/payment-status\",method=\"GET\",le=\"+Inf\"} 2
velasia_web_upstream_request_duration_seconds_sum{operation=\"/v1/orders/:id/payment-status\",method=\"GET\"} 0.012889615
velasia_web_upstream_request_duration_seconds_count{operation=\"/v1/orders/:id/payment-status\",method=\"GET\"} 2
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/destinations/tokyo\",method=\"GET\",le=\"0.001\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/destinations/tokyo\",method=\"GET\",le=\"0.0025\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/destinations/tokyo\",method=\"GET\",le=\"0.005\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/destinations/tokyo\",method=\"GET\",le=\"0.01\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/destinations/tokyo\",method=\"GET\",le=\"0.025\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/destinations/tokyo\",method=\"GET\",le=\"0.05\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/destinations/tokyo\",method=\"GET\",le=\"0.1\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/destinations/tokyo\",method=\"GET\",le=\"0.25\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/destinations/tokyo\",method=\"GET\",le=\"0.5\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/destinations/tokyo\",method=\"GET\",le=\"1\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/destinations/tokyo\",method=\"GET\",le=\"2.5\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/destinations/tokyo\",method=\"GET\",le=\"5\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/destinations/tokyo\",method=\"GET\",le=\"10\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/destinations/tokyo\",method=\"GET\",le=\"+Inf\"} 1
velasia_web_upstream_request_duration_seconds_sum{operation=\"/v1/destinations/tokyo\",method=\"GET\"} 0.018446751
velasia_web_upstream_request_duration_seconds_count{operation=\"/v1/destinations/tokyo\",method=\"GET\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/destinations/atlantis\",method=\"GET\",le=\"0.001\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/destinations/atlantis\",method=\"GET\",le=\"0.0025\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/destinations/atlantis\",method=\"GET\",le=\"0.005\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/destinations/atlantis\",method=\"GET\",le=\"0.01\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/destinations/atlantis\",method=\"GET\",le=\"0.025\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/destinations/atlantis\",method=\"GET\",le=\"0.05\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/destinations/atlantis\",method=\"GET\",le=\"0.1\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/destinations/atlantis\",method=\"GET\",le=\"0.25\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/destinations/atlantis\",method=\"GET\",le=\"0.5\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/destinations/atlantis\",method=\"GET\",le=\"1\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/destinations/atlantis\",method=\"GET\",le=\"2.5\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/destinations/atlantis\",method=\"GET\",le=\"5\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/destinations/atlantis\",method=\"GET\",le=\"10\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/destinations/atlantis\",method=\"GET\",le=\"+Inf\"} 1
velasia_web_upstream_request_duration_seconds_sum{operation=\"/v1/destinations/atlantis\",method=\"GET\"} 0.001136232
velasia_web_upstream_request_duration_seconds_count{operation=\"/v1/destinations/atlantis\",method=\"GET\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/flights/:id/availability\",method=\"GET\",le=\"0.001\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/flights/:id/availability\",method=\"GET\",le=\"0.0025\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/flights/:id/availability\",method=\"GET\",le=\"0.005\"} 2
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/flights/:id/availability\",method=\"GET\",le=\"0.01\"} 3
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/flights/:id/availability\",method=\"GET\",le=\"0.025\"} 3
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/flights/:id/availability\",method=\"GET\",le=\"0.05\"} 3
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/flights/:id/availability\",method=\"GET\",le=\"0.1\"} 3
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/flights/:id/availability\",method=\"GET\",le=\"0.25\"} 3
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/flights/:id/availability\",method=\"GET\",le=\"0.5\"} 3
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/flights/:id/availability\",method=\"GET\",le=\"1\"} 3
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/flights/:id/availability\",method=\"GET\",le=\"2.5\"} 3
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/flights/:id/availability\",method=\"GET\",le=\"5\"} 3
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/flights/:id/availability\",method=\"GET\",le=\"10\"} 3
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/flights/:id/availability\",method=\"GET\",le=\"+Inf\"} 3
velasia_web_upstream_request_duration_seconds_sum{operation=\"/v1/flights/:id/availability\",method=\"GET\"} 0.014222310000000002
velasia_web_upstream_request_duration_seconds_count{operation=\"/v1/flights/:id/availability\",method=\"GET\"} 3
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/flights/:id\",method=\"GET\",le=\"0.001\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/flights/:id\",method=\"GET\",le=\"0.0025\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/flights/:id\",method=\"GET\",le=\"0.005\"} 2
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/flights/:id\",method=\"GET\",le=\"0.01\"} 3
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/flights/:id\",method=\"GET\",le=\"0.025\"} 3
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/flights/:id\",method=\"GET\",le=\"0.05\"} 3
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/flights/:id\",method=\"GET\",le=\"0.1\"} 3
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/flights/:id\",method=\"GET\",le=\"0.25\"} 3
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/flights/:id\",method=\"GET\",le=\"0.5\"} 3
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/flights/:id\",method=\"GET\",le=\"1\"} 3
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/flights/:id\",method=\"GET\",le=\"2.5\"} 3
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/flights/:id\",method=\"GET\",le=\"5\"} 3
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/flights/:id\",method=\"GET\",le=\"10\"} 3
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/flights/:id\",method=\"GET\",le=\"+Inf\"} 3
velasia_web_upstream_request_duration_seconds_sum{operation=\"/v1/flights/:id\",method=\"GET\"} 0.01427143
velasia_web_upstream_request_duration_seconds_count{operation=\"/v1/flights/:id\",method=\"GET\"} 3
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/routes\",method=\"GET\",le=\"0.001\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/routes\",method=\"GET\",le=\"0.0025\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/routes\",method=\"GET\",le=\"0.005\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/routes\",method=\"GET\",le=\"0.01\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/routes\",method=\"GET\",le=\"0.025\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/routes\",method=\"GET\",le=\"0.05\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/routes\",method=\"GET\",le=\"0.1\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/routes\",method=\"GET\",le=\"0.25\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/routes\",method=\"GET\",le=\"0.5\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/routes\",method=\"GET\",le=\"1\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/routes\",method=\"GET\",le=\"2.5\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/routes\",method=\"GET\",le=\"5\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/routes\",method=\"GET\",le=\"10\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/routes\",method=\"GET\",le=\"+Inf\"} 1
velasia_web_upstream_request_duration_seconds_sum{operation=\"/v1/routes\",method=\"GET\"} 0.027202556
velasia_web_upstream_request_duration_seconds_count{operation=\"/v1/routes\",method=\"GET\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/guest/orders\",method=\"GET\",le=\"0.001\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/guest/orders\",method=\"GET\",le=\"0.0025\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/guest/orders\",method=\"GET\",le=\"0.005\"} 3
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/guest/orders\",method=\"GET\",le=\"0.01\"} 3
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/guest/orders\",method=\"GET\",le=\"0.025\"} 3
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/guest/orders\",method=\"GET\",le=\"0.05\"} 3
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/guest/orders\",method=\"GET\",le=\"0.1\"} 3
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/guest/orders\",method=\"GET\",le=\"0.25\"} 3
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/guest/orders\",method=\"GET\",le=\"0.5\"} 3
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/guest/orders\",method=\"GET\",le=\"1\"} 3
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/guest/orders\",method=\"GET\",le=\"2.5\"} 3
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/guest/orders\",method=\"GET\",le=\"5\"} 3
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/guest/orders\",method=\"GET\",le=\"10\"} 3
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/guest/orders\",method=\"GET\",le=\"+Inf\"} 3
velasia_web_upstream_request_duration_seconds_sum{operation=\"/v1/guest/orders\",method=\"GET\"} 0.011527395999999999
velasia_web_upstream_request_duration_seconds_count{operation=\"/v1/guest/orders\",method=\"GET\"} 3
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/servicing\",method=\"GET\",le=\"0.001\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/servicing\",method=\"GET\",le=\"0.0025\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/servicing\",method=\"GET\",le=\"0.005\"} 12
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/servicing\",method=\"GET\",le=\"0.01\"} 15
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/servicing\",method=\"GET\",le=\"0.025\"} 16
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/servicing\",method=\"GET\",le=\"0.05\"} 16
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/servicing\",method=\"GET\",le=\"0.1\"} 16
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/servicing\",method=\"GET\",le=\"0.25\"} 16
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/servicing\",method=\"GET\",le=\"0.5\"} 16
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/servicing\",method=\"GET\",le=\"1\"} 16
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/servicing\",method=\"GET\",le=\"2.5\"} 16
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/servicing\",method=\"GET\",le=\"5\"} 16
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/servicing\",method=\"GET\",le=\"10\"} 16
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/servicing\",method=\"GET\",le=\"+Inf\"} 16
velasia_web_upstream_request_duration_seconds_sum{operation=\"/v1/orders/:id/servicing\",method=\"GET\"} 0.06615198200000001
velasia_web_upstream_request_duration_seconds_count{operation=\"/v1/orders/:id/servicing\",method=\"GET\"} 16
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/events\",method=\"GET\",le=\"0.001\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/events\",method=\"GET\",le=\"0.0025\"} 2
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/events\",method=\"GET\",le=\"0.005\"} 5
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/events\",method=\"GET\",le=\"0.01\"} 5
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/events\",method=\"GET\",le=\"0.025\"} 5
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/events\",method=\"GET\",le=\"0.05\"} 5
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/events\",method=\"GET\",le=\"0.1\"} 5
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/events\",method=\"GET\",le=\"0.25\"} 5
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/events\",method=\"GET\",le=\"0.5\"} 5
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/events\",method=\"GET\",le=\"1\"} 5
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/events\",method=\"GET\",le=\"2.5\"} 5
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/events\",method=\"GET\",le=\"5\"} 5
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/events\",method=\"GET\",le=\"10\"} 5
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/events\",method=\"GET\",le=\"+Inf\"} 5
velasia_web_upstream_request_duration_seconds_sum{operation=\"/v1/orders/:id/events\",method=\"GET\"} 0.013839336
velasia_web_upstream_request_duration_seconds_count{operation=\"/v1/orders/:id/events\",method=\"GET\"} 5
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/change-quotes\",method=\"POST\",le=\"0.001\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/change-quotes\",method=\"POST\",le=\"0.0025\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/change-quotes\",method=\"POST\",le=\"0.005\"} 2
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/change-quotes\",method=\"POST\",le=\"0.01\"} 4
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/change-quotes\",method=\"POST\",le=\"0.025\"} 4
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/change-quotes\",method=\"POST\",le=\"0.05\"} 4
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/change-quotes\",method=\"POST\",le=\"0.1\"} 4
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/change-quotes\",method=\"POST\",le=\"0.25\"} 4
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/change-quotes\",method=\"POST\",le=\"0.5\"} 4
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/change-quotes\",method=\"POST\",le=\"1\"} 4
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/change-quotes\",method=\"POST\",le=\"2.5\"} 4
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/change-quotes\",method=\"POST\",le=\"5\"} 4
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/change-quotes\",method=\"POST\",le=\"10\"} 4
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/change-quotes\",method=\"POST\",le=\"+Inf\"} 4
velasia_web_upstream_request_duration_seconds_sum{operation=\"/v1/orders/:id/change-quotes\",method=\"POST\"} 0.01907133
velasia_web_upstream_request_duration_seconds_count{operation=\"/v1/orders/:id/change-quotes\",method=\"POST\"} 4
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/changes\",method=\"POST\",le=\"0.001\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/changes\",method=\"POST\",le=\"0.0025\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/changes\",method=\"POST\",le=\"0.005\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/changes\",method=\"POST\",le=\"0.01\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/changes\",method=\"POST\",le=\"0.025\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/changes\",method=\"POST\",le=\"0.05\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/changes\",method=\"POST\",le=\"0.1\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/changes\",method=\"POST\",le=\"0.25\"} 2
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/changes\",method=\"POST\",le=\"0.5\"} 2
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/changes\",method=\"POST\",le=\"1\"} 2
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/changes\",method=\"POST\",le=\"2.5\"} 2
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/changes\",method=\"POST\",le=\"5\"} 2
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/changes\",method=\"POST\",le=\"10\"} 2
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/changes\",method=\"POST\",le=\"+Inf\"} 2
velasia_web_upstream_request_duration_seconds_sum{operation=\"/v1/orders/:id/changes\",method=\"POST\"} 0.350675347
velasia_web_upstream_request_duration_seconds_count{operation=\"/v1/orders/:id/changes\",method=\"POST\"} 2
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/seatmaps/:id\",method=\"GET\",le=\"0.001\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/seatmaps/:id\",method=\"GET\",le=\"0.0025\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/seatmaps/:id\",method=\"GET\",le=\"0.005\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/seatmaps/:id\",method=\"GET\",le=\"0.01\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/seatmaps/:id\",method=\"GET\",le=\"0.025\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/seatmaps/:id\",method=\"GET\",le=\"0.05\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/seatmaps/:id\",method=\"GET\",le=\"0.1\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/seatmaps/:id\",method=\"GET\",le=\"0.25\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/seatmaps/:id\",method=\"GET\",le=\"0.5\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/seatmaps/:id\",method=\"GET\",le=\"1\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/seatmaps/:id\",method=\"GET\",le=\"2.5\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/seatmaps/:id\",method=\"GET\",le=\"5\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/seatmaps/:id\",method=\"GET\",le=\"10\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/seatmaps/:id\",method=\"GET\",le=\"+Inf\"} 1
velasia_web_upstream_request_duration_seconds_sum{operation=\"/v1/orders/:id/seatmaps/:id\",method=\"GET\"} 0.007791084
velasia_web_upstream_request_duration_seconds_count{operation=\"/v1/orders/:id/seatmaps/:id\",method=\"GET\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/seat-quotes\",method=\"POST\",le=\"0.001\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/seat-quotes\",method=\"POST\",le=\"0.0025\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/seat-quotes\",method=\"POST\",le=\"0.005\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/seat-quotes\",method=\"POST\",le=\"0.01\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/seat-quotes\",method=\"POST\",le=\"0.025\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/seat-quotes\",method=\"POST\",le=\"0.05\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/seat-quotes\",method=\"POST\",le=\"0.1\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/seat-quotes\",method=\"POST\",le=\"0.25\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/seat-quotes\",method=\"POST\",le=\"0.5\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/seat-quotes\",method=\"POST\",le=\"1\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/seat-quotes\",method=\"POST\",le=\"2.5\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/seat-quotes\",method=\"POST\",le=\"5\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/seat-quotes\",method=\"POST\",le=\"10\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/seat-quotes\",method=\"POST\",le=\"+Inf\"} 1
velasia_web_upstream_request_duration_seconds_sum{operation=\"/v1/orders/:id/seat-quotes\",method=\"POST\"} 0.005413798
velasia_web_upstream_request_duration_seconds_count{operation=\"/v1/orders/:id/seat-quotes\",method=\"POST\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/seat-changes\",method=\"POST\",le=\"0.001\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/seat-changes\",method=\"POST\",le=\"0.0025\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/seat-changes\",method=\"POST\",le=\"0.005\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/seat-changes\",method=\"POST\",le=\"0.01\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/seat-changes\",method=\"POST\",le=\"0.025\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/seat-changes\",method=\"POST\",le=\"0.05\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/seat-changes\",method=\"POST\",le=\"0.1\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/seat-changes\",method=\"POST\",le=\"0.25\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/seat-changes\",method=\"POST\",le=\"0.5\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/seat-changes\",method=\"POST\",le=\"1\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/seat-changes\",method=\"POST\",le=\"2.5\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/seat-changes\",method=\"POST\",le=\"5\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/seat-changes\",method=\"POST\",le=\"10\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/seat-changes\",method=\"POST\",le=\"+Inf\"} 1
velasia_web_upstream_request_duration_seconds_sum{operation=\"/v1/orders/:id/seat-changes\",method=\"POST\"} 0.16841622
velasia_web_upstream_request_duration_seconds_count{operation=\"/v1/orders/:id/seat-changes\",method=\"POST\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/reference/VEL-SBGEQT\",method=\"GET\",le=\"0.001\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/reference/VEL-SBGEQT\",method=\"GET\",le=\"0.0025\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/reference/VEL-SBGEQT\",method=\"GET\",le=\"0.005\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/reference/VEL-SBGEQT\",method=\"GET\",le=\"0.01\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/reference/VEL-SBGEQT\",method=\"GET\",le=\"0.025\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/reference/VEL-SBGEQT\",method=\"GET\",le=\"0.05\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/reference/VEL-SBGEQT\",method=\"GET\",le=\"0.1\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/reference/VEL-SBGEQT\",method=\"GET\",le=\"0.25\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/reference/VEL-SBGEQT\",method=\"GET\",le=\"0.5\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/reference/VEL-SBGEQT\",method=\"GET\",le=\"1\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/reference/VEL-SBGEQT\",method=\"GET\",le=\"2.5\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/reference/VEL-SBGEQT\",method=\"GET\",le=\"5\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/reference/VEL-SBGEQT\",method=\"GET\",le=\"10\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/reference/VEL-SBGEQT\",method=\"GET\",le=\"+Inf\"} 1
velasia_web_upstream_request_duration_seconds_sum{operation=\"/v1/orders/reference/VEL-SBGEQT\",method=\"GET\"} 0.00360144
velasia_web_upstream_request_duration_seconds_count{operation=\"/v1/orders/reference/VEL-SBGEQT\",method=\"GET\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/ancillaries\",method=\"GET\",le=\"0.001\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/ancillaries\",method=\"GET\",le=\"0.0025\"} 3
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/ancillaries\",method=\"GET\",le=\"0.005\"} 3
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/ancillaries\",method=\"GET\",le=\"0.01\"} 3
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/ancillaries\",method=\"GET\",le=\"0.025\"} 3
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/ancillaries\",method=\"GET\",le=\"0.05\"} 3
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/ancillaries\",method=\"GET\",le=\"0.1\"} 3
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/ancillaries\",method=\"GET\",le=\"0.25\"} 3
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/ancillaries\",method=\"GET\",le=\"0.5\"} 3
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/ancillaries\",method=\"GET\",le=\"1\"} 3
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/ancillaries\",method=\"GET\",le=\"2.5\"} 3
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/ancillaries\",method=\"GET\",le=\"5\"} 3
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/ancillaries\",method=\"GET\",le=\"10\"} 3
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/ancillaries\",method=\"GET\",le=\"+Inf\"} 3
velasia_web_upstream_request_duration_seconds_sum{operation=\"/v1/ancillaries\",method=\"GET\"} 0.004632605
velasia_web_upstream_request_duration_seconds_count{operation=\"/v1/ancillaries\",method=\"GET\"} 3
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/ancillaries\",method=\"GET\",le=\"0.001\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/ancillaries\",method=\"GET\",le=\"0.0025\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/ancillaries\",method=\"GET\",le=\"0.005\"} 2
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/ancillaries\",method=\"GET\",le=\"0.01\"} 3
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/ancillaries\",method=\"GET\",le=\"0.025\"} 3
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/ancillaries\",method=\"GET\",le=\"0.05\"} 3
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/ancillaries\",method=\"GET\",le=\"0.1\"} 3
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/ancillaries\",method=\"GET\",le=\"0.25\"} 3
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/ancillaries\",method=\"GET\",le=\"0.5\"} 3
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/ancillaries\",method=\"GET\",le=\"1\"} 3
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/ancillaries\",method=\"GET\",le=\"2.5\"} 3
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/ancillaries\",method=\"GET\",le=\"5\"} 3
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/ancillaries\",method=\"GET\",le=\"10\"} 3
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/ancillaries\",method=\"GET\",le=\"+Inf\"} 3
velasia_web_upstream_request_duration_seconds_sum{operation=\"/v1/orders/:id/ancillaries\",method=\"GET\"} 0.012131159999999998
velasia_web_upstream_request_duration_seconds_count{operation=\"/v1/orders/:id/ancillaries\",method=\"GET\"} 3
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/ancillary-quotes\",method=\"POST\",le=\"0.001\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/ancillary-quotes\",method=\"POST\",le=\"0.0025\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/ancillary-quotes\",method=\"POST\",le=\"0.005\"} 2
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/ancillary-quotes\",method=\"POST\",le=\"0.01\"} 2
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/ancillary-quotes\",method=\"POST\",le=\"0.025\"} 2
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/ancillary-quotes\",method=\"POST\",le=\"0.05\"} 2
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/ancillary-quotes\",method=\"POST\",le=\"0.1\"} 2
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/ancillary-quotes\",method=\"POST\",le=\"0.25\"} 2
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/ancillary-quotes\",method=\"POST\",le=\"0.5\"} 2
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/ancillary-quotes\",method=\"POST\",le=\"1\"} 2
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/ancillary-quotes\",method=\"POST\",le=\"2.5\"} 2
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/ancillary-quotes\",method=\"POST\",le=\"5\"} 2
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/ancillary-quotes\",method=\"POST\",le=\"10\"} 2
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/ancillary-quotes\",method=\"POST\",le=\"+Inf\"} 2
velasia_web_upstream_request_duration_seconds_sum{operation=\"/v1/orders/:id/ancillary-quotes\",method=\"POST\"} 0.006194206
velasia_web_upstream_request_duration_seconds_count{operation=\"/v1/orders/:id/ancillary-quotes\",method=\"POST\"} 2
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/ancillary-purchases\",method=\"POST\",le=\"0.001\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/ancillary-purchases\",method=\"POST\",le=\"0.0025\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/ancillary-purchases\",method=\"POST\",le=\"0.005\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/ancillary-purchases\",method=\"POST\",le=\"0.01\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/ancillary-purchases\",method=\"POST\",le=\"0.025\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/ancillary-purchases\",method=\"POST\",le=\"0.05\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/ancillary-purchases\",method=\"POST\",le=\"0.1\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/ancillary-purchases\",method=\"POST\",le=\"0.25\"} 2
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/ancillary-purchases\",method=\"POST\",le=\"0.5\"} 2
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/ancillary-purchases\",method=\"POST\",le=\"1\"} 2
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/ancillary-purchases\",method=\"POST\",le=\"2.5\"} 2
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/ancillary-purchases\",method=\"POST\",le=\"5\"} 2
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/ancillary-purchases\",method=\"POST\",le=\"10\"} 2
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/ancillary-purchases\",method=\"POST\",le=\"+Inf\"} 2
velasia_web_upstream_request_duration_seconds_sum{operation=\"/v1/orders/:id/ancillary-purchases\",method=\"POST\"} 0.32064037599999995
velasia_web_upstream_request_duration_seconds_count{operation=\"/v1/orders/:id/ancillary-purchases\",method=\"POST\"} 2
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/accounts\",method=\"POST\",le=\"0.001\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/accounts\",method=\"POST\",le=\"0.0025\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/accounts\",method=\"POST\",le=\"0.005\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/accounts\",method=\"POST\",le=\"0.01\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/accounts\",method=\"POST\",le=\"0.025\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/accounts\",method=\"POST\",le=\"0.05\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/accounts\",method=\"POST\",le=\"0.1\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/accounts\",method=\"POST\",le=\"0.25\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/accounts\",method=\"POST\",le=\"0.5\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/accounts\",method=\"POST\",le=\"1\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/accounts\",method=\"POST\",le=\"2.5\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/accounts\",method=\"POST\",le=\"5\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/accounts\",method=\"POST\",le=\"10\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/accounts\",method=\"POST\",le=\"+Inf\"} 1
velasia_web_upstream_request_duration_seconds_sum{operation=\"/v1/accounts\",method=\"POST\"} 0.057038221
velasia_web_upstream_request_duration_seconds_count{operation=\"/v1/accounts\",method=\"POST\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/me\",method=\"GET\",le=\"0.001\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/me\",method=\"GET\",le=\"0.0025\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/me\",method=\"GET\",le=\"0.005\"} 2
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/me\",method=\"GET\",le=\"0.01\"} 3
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/me\",method=\"GET\",le=\"0.025\"} 3
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/me\",method=\"GET\",le=\"0.05\"} 3
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/me\",method=\"GET\",le=\"0.1\"} 3
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/me\",method=\"GET\",le=\"0.25\"} 3
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/me\",method=\"GET\",le=\"0.5\"} 3
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/me\",method=\"GET\",le=\"1\"} 3
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/me\",method=\"GET\",le=\"2.5\"} 3
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/me\",method=\"GET\",le=\"5\"} 3
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/me\",method=\"GET\",le=\"10\"} 3
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/me\",method=\"GET\",le=\"+Inf\"} 3
velasia_web_upstream_request_duration_seconds_sum{operation=\"/v1/me\",method=\"GET\"} 0.013630717999999998
velasia_web_upstream_request_duration_seconds_count{operation=\"/v1/me\",method=\"GET\"} 3
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/me/orders\",method=\"GET\",le=\"0.001\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/me/orders\",method=\"GET\",le=\"0.0025\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/me/orders\",method=\"GET\",le=\"0.005\"} 2
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/me/orders\",method=\"GET\",le=\"0.01\"} 2
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/me/orders\",method=\"GET\",le=\"0.025\"} 2
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/me/orders\",method=\"GET\",le=\"0.05\"} 2
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/me/orders\",method=\"GET\",le=\"0.1\"} 2
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/me/orders\",method=\"GET\",le=\"0.25\"} 2
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/me/orders\",method=\"GET\",le=\"0.5\"} 2
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/me/orders\",method=\"GET\",le=\"1\"} 2
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/me/orders\",method=\"GET\",le=\"2.5\"} 2
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/me/orders\",method=\"GET\",le=\"5\"} 2
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/me/orders\",method=\"GET\",le=\"10\"} 2
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/me/orders\",method=\"GET\",le=\"+Inf\"} 2
velasia_web_upstream_request_duration_seconds_sum{operation=\"/v1/me/orders\",method=\"GET\"} 0.006754973
velasia_web_upstream_request_duration_seconds_count{operation=\"/v1/me/orders\",method=\"GET\"} 2
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/me/orders/link\",method=\"POST\",le=\"0.001\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/me/orders/link\",method=\"POST\",le=\"0.0025\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/me/orders/link\",method=\"POST\",le=\"0.005\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/me/orders/link\",method=\"POST\",le=\"0.01\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/me/orders/link\",method=\"POST\",le=\"0.025\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/me/orders/link\",method=\"POST\",le=\"0.05\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/me/orders/link\",method=\"POST\",le=\"0.1\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/me/orders/link\",method=\"POST\",le=\"0.25\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/me/orders/link\",method=\"POST\",le=\"0.5\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/me/orders/link\",method=\"POST\",le=\"1\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/me/orders/link\",method=\"POST\",le=\"2.5\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/me/orders/link\",method=\"POST\",le=\"5\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/me/orders/link\",method=\"POST\",le=\"10\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/me/orders/link\",method=\"POST\",le=\"+Inf\"} 1
velasia_web_upstream_request_duration_seconds_sum{operation=\"/v1/me/orders/link\",method=\"POST\"} 0.005373995
velasia_web_upstream_request_duration_seconds_count{operation=\"/v1/me/orders/link\",method=\"POST\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/me/travellers\",method=\"GET\",le=\"0.001\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/me/travellers\",method=\"GET\",le=\"0.0025\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/me/travellers\",method=\"GET\",le=\"0.005\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/me/travellers\",method=\"GET\",le=\"0.01\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/me/travellers\",method=\"GET\",le=\"0.025\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/me/travellers\",method=\"GET\",le=\"0.05\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/me/travellers\",method=\"GET\",le=\"0.1\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/me/travellers\",method=\"GET\",le=\"0.25\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/me/travellers\",method=\"GET\",le=\"0.5\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/me/travellers\",method=\"GET\",le=\"1\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/me/travellers\",method=\"GET\",le=\"2.5\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/me/travellers\",method=\"GET\",le=\"5\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/me/travellers\",method=\"GET\",le=\"10\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/me/travellers\",method=\"GET\",le=\"+Inf\"} 1
velasia_web_upstream_request_duration_seconds_sum{operation=\"/v1/me/travellers\",method=\"GET\"} 0.001248621
velasia_web_upstream_request_duration_seconds_count{operation=\"/v1/me/travellers\",method=\"GET\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/me/travellers\",method=\"POST\",le=\"0.001\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/me/travellers\",method=\"POST\",le=\"0.0025\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/me/travellers\",method=\"POST\",le=\"0.005\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/me/travellers\",method=\"POST\",le=\"0.01\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/me/travellers\",method=\"POST\",le=\"0.025\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/me/travellers\",method=\"POST\",le=\"0.05\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/me/travellers\",method=\"POST\",le=\"0.1\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/me/travellers\",method=\"POST\",le=\"0.25\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/me/travellers\",method=\"POST\",le=\"0.5\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/me/travellers\",method=\"POST\",le=\"1\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/me/travellers\",method=\"POST\",le=\"2.5\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/me/travellers\",method=\"POST\",le=\"5\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/me/travellers\",method=\"POST\",le=\"10\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/me/travellers\",method=\"POST\",le=\"+Inf\"} 1
velasia_web_upstream_request_duration_seconds_sum{operation=\"/v1/me/travellers\",method=\"POST\"} 0.009136768
velasia_web_upstream_request_duration_seconds_count{operation=\"/v1/me/travellers\",method=\"POST\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/sessions/current\",method=\"DELETE\",le=\"0.001\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/sessions/current\",method=\"DELETE\",le=\"0.0025\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/sessions/current\",method=\"DELETE\",le=\"0.005\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/sessions/current\",method=\"DELETE\",le=\"0.01\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/sessions/current\",method=\"DELETE\",le=\"0.025\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/sessions/current\",method=\"DELETE\",le=\"0.05\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/sessions/current\",method=\"DELETE\",le=\"0.1\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/sessions/current\",method=\"DELETE\",le=\"0.25\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/sessions/current\",method=\"DELETE\",le=\"0.5\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/sessions/current\",method=\"DELETE\",le=\"1\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/sessions/current\",method=\"DELETE\",le=\"2.5\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/sessions/current\",method=\"DELETE\",le=\"5\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/sessions/current\",method=\"DELETE\",le=\"10\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/sessions/current\",method=\"DELETE\",le=\"+Inf\"} 1
velasia_web_upstream_request_duration_seconds_sum{operation=\"/v1/sessions/current\",method=\"DELETE\"} 0.001632116
velasia_web_upstream_request_duration_seconds_count{operation=\"/v1/sessions/current\",method=\"DELETE\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/sessions\",method=\"POST\",le=\"0.001\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/sessions\",method=\"POST\",le=\"0.0025\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/sessions\",method=\"POST\",le=\"0.005\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/sessions\",method=\"POST\",le=\"0.01\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/sessions\",method=\"POST\",le=\"0.025\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/sessions\",method=\"POST\",le=\"0.05\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/sessions\",method=\"POST\",le=\"0.1\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/sessions\",method=\"POST\",le=\"0.25\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/sessions\",method=\"POST\",le=\"0.5\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/sessions\",method=\"POST\",le=\"1\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/sessions\",method=\"POST\",le=\"2.5\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/sessions\",method=\"POST\",le=\"5\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/sessions\",method=\"POST\",le=\"10\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/sessions\",method=\"POST\",le=\"+Inf\"} 1
velasia_web_upstream_request_duration_seconds_sum{operation=\"/v1/sessions\",method=\"POST\"} 0.042224743
velasia_web_upstream_request_duration_seconds_count{operation=\"/v1/sessions\",method=\"POST\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/cancellation-quotes\",method=\"POST\",le=\"0.001\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/cancellation-quotes\",method=\"POST\",le=\"0.0025\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/cancellation-quotes\",method=\"POST\",le=\"0.005\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/cancellation-quotes\",method=\"POST\",le=\"0.01\"} 2
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/cancellation-quotes\",method=\"POST\",le=\"0.025\"} 2
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/cancellation-quotes\",method=\"POST\",le=\"0.05\"} 2
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/cancellation-quotes\",method=\"POST\",le=\"0.1\"} 2
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/cancellation-quotes\",method=\"POST\",le=\"0.25\"} 2
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/cancellation-quotes\",method=\"POST\",le=\"0.5\"} 2
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/cancellation-quotes\",method=\"POST\",le=\"1\"} 2
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/cancellation-quotes\",method=\"POST\",le=\"2.5\"} 2
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/cancellation-quotes\",method=\"POST\",le=\"5\"} 2
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/cancellation-quotes\",method=\"POST\",le=\"10\"} 2
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/cancellation-quotes\",method=\"POST\",le=\"+Inf\"} 2
velasia_web_upstream_request_duration_seconds_sum{operation=\"/v1/orders/:id/cancellation-quotes\",method=\"POST\"} 0.012713551
velasia_web_upstream_request_duration_seconds_count{operation=\"/v1/orders/:id/cancellation-quotes\",method=\"POST\"} 2
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/cancellations\",method=\"POST\",le=\"0.001\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/cancellations\",method=\"POST\",le=\"0.0025\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/cancellations\",method=\"POST\",le=\"0.005\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/cancellations\",method=\"POST\",le=\"0.01\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/cancellations\",method=\"POST\",le=\"0.025\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/cancellations\",method=\"POST\",le=\"0.05\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/cancellations\",method=\"POST\",le=\"0.1\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/cancellations\",method=\"POST\",le=\"0.25\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/cancellations\",method=\"POST\",le=\"0.5\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/cancellations\",method=\"POST\",le=\"1\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/cancellations\",method=\"POST\",le=\"2.5\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/cancellations\",method=\"POST\",le=\"5\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/cancellations\",method=\"POST\",le=\"10\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/cancellations\",method=\"POST\",le=\"+Inf\"} 1
velasia_web_upstream_request_duration_seconds_sum{operation=\"/v1/orders/:id/cancellations\",method=\"POST\"} 0.162122741
velasia_web_upstream_request_duration_seconds_count{operation=\"/v1/orders/:id/cancellations\",method=\"POST\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/servicing-status\",method=\"GET\",le=\"0.001\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/servicing-status\",method=\"GET\",le=\"0.0025\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/servicing-status\",method=\"GET\",le=\"0.005\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/servicing-status\",method=\"GET\",le=\"0.01\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/servicing-status\",method=\"GET\",le=\"0.025\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/servicing-status\",method=\"GET\",le=\"0.05\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/servicing-status\",method=\"GET\",le=\"0.1\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/servicing-status\",method=\"GET\",le=\"0.25\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/servicing-status\",method=\"GET\",le=\"0.5\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/servicing-status\",method=\"GET\",le=\"1\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/servicing-status\",method=\"GET\",le=\"2.5\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/servicing-status\",method=\"GET\",le=\"5\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/servicing-status\",method=\"GET\",le=\"10\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/servicing-status\",method=\"GET\",le=\"+Inf\"} 1
velasia_web_upstream_request_duration_seconds_sum{operation=\"/v1/orders/:id/servicing-status\",method=\"GET\"} 0.006023557
velasia_web_upstream_request_duration_seconds_count{operation=\"/v1/orders/:id/servicing-status\",method=\"GET\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/servicing/:id/abandon\",method=\"POST\",le=\"0.001\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/servicing/:id/abandon\",method=\"POST\",le=\"0.0025\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/servicing/:id/abandon\",method=\"POST\",le=\"0.005\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/servicing/:id/abandon\",method=\"POST\",le=\"0.01\"} 0
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/servicing/:id/abandon\",method=\"POST\",le=\"0.025\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/servicing/:id/abandon\",method=\"POST\",le=\"0.05\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/servicing/:id/abandon\",method=\"POST\",le=\"0.1\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/servicing/:id/abandon\",method=\"POST\",le=\"0.25\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/servicing/:id/abandon\",method=\"POST\",le=\"0.5\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/servicing/:id/abandon\",method=\"POST\",le=\"1\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/servicing/:id/abandon\",method=\"POST\",le=\"2.5\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/servicing/:id/abandon\",method=\"POST\",le=\"5\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/servicing/:id/abandon\",method=\"POST\",le=\"10\"} 1
velasia_web_upstream_request_duration_seconds_bucket{operation=\"/v1/orders/:id/servicing/:id/abandon\",method=\"POST\",le=\"+Inf\"} 1
velasia_web_upstream_request_duration_seconds_sum{operation=\"/v1/orders/:id/servicing/:id/abandon\",method=\"POST\"} 0.014982775
velasia_web_upstream_request_duration_seconds_count{operation=\"/v1/orders/:id/servicing/:id/abandon\",method=\"POST\"} 1
# HELP velasia_web_request_errors_total Server errors reported by Next.js (onRequestError), by route type.
# TYPE velasia_web_request_errors_total counter
velasia_web_request_errors_total{route_type=\"action\",render_source=\"react-server-components-payload\"} 2
"
```

# Test source

```ts
  40  |       expect(headers['referrer-policy']).toBe('strict-origin-when-cross-origin');
  41  |       expect(headers['permissions-policy']).toContain('camera=()');
  42  |       expect(headers['x-powered-by']).toBeUndefined();
  43  |     });
  44  |   }
  45  | 
  46  |   test('keep personal pages out of shared caches and let static pages be cached', async ({
  47  |     request,
  48  |   }) => {
  49  |     for (const path of ['/account/trips', '/booking/travellers', '/manage-booking', SEARCH]) {
  50  |       const cacheControl = (await request.get(path)).headers()['cache-control'] ?? '';
  51  |       expect(cacheControl, path).toMatch(/private/);
  52  |       expect(cacheControl, path).toMatch(/no-store/);
  53  |     }
  54  |     expect((await request.get('/')).headers()['cache-control']).toMatch(/s-maxage=\d+/);
  55  |     const live = await request.get(
  56  |       '/api/flights?from=MNL&to=SIN&departureDate=2026-12-15&adults=1',
  57  |     );
  58  |     expect(live.headers()['cache-control']).toBe('private, no-store');
  59  |   });
  60  | });
  61  | 
  62  | test('a full booking runs under the Content Security Policy with no violations', async ({
  63  |   page,
  64  | }) => {
  65  |   const violations = cspViolations(page);
  66  |   await toReview(page);
  67  |   await toCheckout(page);
  68  |   await pay(page, 'Demo Card', 'Approve');
  69  |   await expectConfirmed(page);
  70  |   await page.goto('/destinations');
  71  |   await page.goto('/');
  72  |   // Images (next/image), fonts (next/font), streamed RSC, and Server Actions all worked.
  73  |   expect(violations).toEqual([]);
  74  | });
  75  | 
  76  | async function captureServerAction(page: Page): Promise<Request> {
  77  |   await page.goto('/manage-booking');
  78  |   const action = page.waitForRequest(
  79  |     (request) => request.method() === 'POST' && Boolean(request.headers()['next-action']),
  80  |   );
  81  |   await page.getByRole('textbox', { name: 'Booking reference' }).fill('VEL-8K4P2Q');
  82  |   await page.getByTestId('lookup-submit').click();
  83  |   return action;
  84  | }
  85  | 
  86  | test.describe('Server Actions', () => {
  87  |   test('refuse a cross-site request and accept the same request from the site', async ({
  88  |     page,
  89  |   }) => {
  90  |     const captured = await captureServerAction(page);
  91  |     const headers = { ...captured.headers() };
  92  |     delete headers['content-length'];
  93  |     const body = captured.postDataBuffer()!;
  94  | 
  95  |     const sameSite = await page.request.post(captured.url(), { headers, data: body });
  96  |     expect(sameSite.status()).toBe(200);
  97  |     expect(await sameSite.text()).toContain('couldn’t open a booking');
  98  | 
  99  |     const crossSite = await page.request.post(captured.url(), {
  100 |       headers: { ...headers, origin: 'https://evil.example' },
  101 |       data: body,
  102 |     });
  103 |     expect(crossSite.status()).toBeGreaterThanOrEqual(400);
  104 |     expect(await crossSite.text()).not.toContain('couldn’t open a booking');
  105 |   });
  106 | 
  107 |   test('refuse oversized request bodies', async ({ page }) => {
  108 |     const captured = await captureServerAction(page);
  109 |     const headers = { ...captured.headers() };
  110 |     delete headers['content-length'];
  111 |     const boundary = /boundary=(.+)$/.exec(headers['content-type'] ?? '')?.[1];
  112 |     test.skip(!boundary, 'the action is not multipart in this build');
  113 |     const padding = `--${boundary}\r\nContent-Disposition: form-data; name="padding"\r\n\r\n${'x'.repeat(200 * 1024)}\r\n`;
  114 |     const response = await page.request.post(captured.url(), {
  115 |       headers,
  116 |       data: Buffer.concat([Buffer.from(padding), captured.postDataBuffer()!]),
  117 |     });
  118 |     expect(response.status()).toBeGreaterThanOrEqual(400);
  119 |   });
  120 | });
  121 | 
  122 | test.describe('internal endpoints', () => {
  123 |   test('liveness, readiness, and metrics answer (the proxy hides them publicly)', async ({
  124 |     request,
  125 |   }) => {
  126 |     const live = await request.get('/api/internal/health/live');
  127 |     expect(live.status()).toBe(200);
  128 |     expect(live.headers()['cache-control']).toBe('no-store');
  129 |     const ready = await request.get('/api/internal/health/ready');
  130 |     expect(ready.status()).toBe(200);
  131 |     expect(await ready.json()).toMatchObject({ status: 'ok', retailingApi: 'ready' });
  132 |     await request.get('/api/flights?from=MNL&to=SIN&departureDate=2026-12-15&adults=1');
  133 |     const metrics = await request.get('/api/internal/metrics');
  134 |     expect(metrics.headers()['content-type']).toContain('text/plain');
  135 |     const text = await metrics.text();
  136 |     expect(text).toMatch(
  137 |       /velasia_web_upstream_requests_total\{operation="\/v1\/flights",method="GET",outcome="ok"\} \d+/,
  138 |     );
  139 |     expect(text).toMatch(/process_resident_memory_bytes\{service="web"\}/);
> 140 |     expect(text).not.toMatch(/ord_|ses_|VEL-/);
      |                      ^ Error: expect(received).not.toMatch(expected)
  141 |   });
  142 | });
  143 | 
  144 | test('requests that try to reach files or internals get nothing', async ({ request }) => {
  145 |   for (const path of [
  146 |     '/.env',
  147 |     '/.env.production',
  148 |     '/velasia.db',
  149 |     '/data/velasia.db',
  150 |     '/_next/static/..%2f..%2f..%2fpackage.json',
  151 |     '/_next/static/../../../.env',
  152 |     '/api/offers/..%2F..%2Fv1%2Forders',
  153 |     '/api/flights/..%2F..%2F..%2Fetc%2Fpasswd',
  154 |     '/public/../package.json',
  155 |   ]) {
  156 |     const response = await request.get(path, { maxRedirects: 0 });
  157 |     const body = await response.text();
  158 |     expect([400, 404, 308], path).toContain(response.status());
  159 |     expect(body, path).not.toMatch(/"dependencies"|root:x:0:0|TALADB_PATH|SQLite|redb/);
  160 |   }
  161 |   const bad = await request.get('/api/flights?from=../../etc&to=SIN&departureDate=x');
  162 |   expect(bad.status()).toBe(400);
  163 |   expect(await bad.text()).not.toMatch(/at .*\.js:\d+|127\.0\.0\.1|4210/);
  164 | });
  165 | 
```