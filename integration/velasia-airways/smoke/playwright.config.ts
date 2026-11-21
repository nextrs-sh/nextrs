import { defineConfig, devices } from '@playwright/test';

/*
 * Smoke tests against the running Compose environment. Nothing is started here: the target is
 * whatever answers on SMOKE_BASE_URL (default http://localhost:3000), which scripts/smoke-test.sh
 * first proves is NextRS.
 */
const baseURL = process.env.SMOKE_BASE_URL ?? 'http://localhost:3000';
const out = process.env.SMOKE_OUT ?? '../results/smoke';

export default defineConfig({
  testDir: './tests',
  outputDir: `${out}/artifacts`,
  fullyParallel: false,
  workers: 1,
  retries: 0,
  timeout: 90_000,
  expect: { timeout: 15_000 },
  reporter: [
    ['list'],
    ['json', { outputFile: `${out}/results.json` }],
    ['html', { outputFolder: `${out}/html`, open: 'never' }],
  ],
  use: {
    baseURL,
    trace: 'retain-on-failure',
    screenshot: 'only-on-failure',
    video: 'off',
  },
  projects: [
    { name: 'chromium', use: { ...devices['Desktop Chrome'] } },
    { name: 'firefox', use: { ...devices['Desktop Firefox'] } },
  ],
});
