import { defineConfig } from '@playwright/test';

// Point E2E_BASE_URL at the running demo. It must be a secure origin (localhost or https).
const baseURL = process.env.E2E_BASE_URL ?? 'http://localhost:8090';

export default defineConfig({
  testDir: './tests',
  timeout: 45_000,
  expect: { timeout: 10_000 },
  fullyParallel: false, // the server keeps its state (providers, ledger, agent), so tests run one at a time
  workers: 1,
  retries: process.env.CI ? 1 : 0,
  reporter: process.env.CI ? [['github'], ['list']] : [['list']],
  use: {
    baseURL,
    trace: 'retain-on-failure',
  },
});
