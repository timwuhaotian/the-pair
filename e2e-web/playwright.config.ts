import { defineConfig, devices } from '@playwright/test'

export default defineConfig({
  testDir: './specs',
  timeout: 30_000,
  fullyParallel: false,
  forbidOnly: !!process.env.CI,
  reporter: 'list',
  use: {
    baseURL: 'http://127.0.0.1:5199',
    headless: true
  },
  webServer: {
    command: 'npm run dev:renderer -- --port 5199 --strictPort --host 127.0.0.1',
    url: 'http://127.0.0.1:5199',
    reuseExistingServer: !process.env.CI,
    timeout: 30_000
  },
  projects: [
    {
      name: 'chromium',
      use: { ...devices['Desktop Chrome'] }
    }
  ]
})
