/*
 * Browser checks for the independent applications (Phase 3C). The specs are
 * NextRS-owned; the applications stay unchanged. Run by browser-apps.mjs,
 * once per server (next start, NextRS) with BASE_URL set; Playwright comes
 * from the Velasia clone's node_modules (NODE_PATH).
 */
import { defineConfig, devices } from '@playwright/test';

export default defineConfig({
  testDir: __dirname,
  testMatch: new RegExp(`${process.env.APP_SPEC ?? '.*'}\\.spec\\.ts$`),
  fullyParallel: false,
  workers: 1,
  retries: 0,
  reporter: [['list'], ['json', { outputFile: process.env.BROWSER_JSON ?? 'browser.json' }]],
  timeout: 60_000,
  expect: { timeout: 15_000 },
  use: { baseURL: process.env.BASE_URL, trace: 'retain-on-failure', ...devices['Desktop Chrome'] },
});
