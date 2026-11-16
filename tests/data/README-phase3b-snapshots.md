# Phase 3B build snapshots

Redacted `next build` outputs (`scripts/snapshot-build.mjs`, then server
bundles, source maps and `static/` removed: the tests that read them run
without JavaScript) of two examples from `vercel/next.js` at tag `v16.4.6`
(commit `54b876b6`), MIT-licensed, built unchanged with Next.js 16.4.6:

- `next-16.4.6-pages-optimized-dynamic/`: `examples/api-routes-rest`
  (Pages Router; `pages/user/[id].tsx` has no data functions, so Next
  optimizes it to one `user/[id].html` for every id).
- `next-16.4.6-static-params/`: `examples/blog-starter` (App Router;
  `/posts/[slug]` prerendered with `generateStaticParams`).

Used by `crates/nextrs-server/tests/http.rs` (Phase 3B independent
applications, gaps 11 and 12).
