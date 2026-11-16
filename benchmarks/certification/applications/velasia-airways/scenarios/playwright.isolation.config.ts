/*
 * NextRS-owned scenarios for Velasia Airways (Phase 3B.6). Velasia stays
 * unchanged: this configuration and its specs live in NextRS, run from the
 * disposable Velasia clone (for its node_modules and test helpers):
 *
 *   cd <clone> && VELASIA_SRC=<clone> NODE_PATH=<clone>/node_modules \
 *     pnpm exec playwright test --config <nextrs>/.../playwright.isolation.config.ts
 *
 * The servers must already be running (certify.mjs up), on Velasia's E2E ports.
 */
import { defineConfig, devices } from '@playwright/test';

export default defineConfig({
  testDir: __dirname,
  testMatch: /.*\.spec\.ts$/,
  fullyParallel: false,
  workers: 1,
  retries: 0,
  reporter: [['list'], ['json', { outputFile: process.env.ISOLATION_JSON ?? 'isolation.json' }]],
  timeout: 600_000,
  expect: { timeout: 20_000 },
  use: {
    baseURL: `http://127.0.0.1:${process.env.E2E_WEB_PORT ?? 3200}`,
    timezoneId: 'Asia/Manila',
    locale: 'en-PH',
    trace: 'retain-on-failure',
    ...devices['Desktop Chrome'],
    viewport: { width: 1440, height: 900 },
  },
});
