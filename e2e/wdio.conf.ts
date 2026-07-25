export const config: WebdriverIO.Config = {
  runner: 'local',
  // Resolved relative to this config file (wdio v8+ behavior).
  specs: ['./specs/**/*.spec.ts'],
  maxInstances: 1,

  capabilities: [
    {
      platformName: 'mac',
      'appium:automationName': 'mac2',
      'appium:bundleId': 'com.thepair.app',
      // Point at a specific bundle (e.g. a fresh local build) instead of the
      // LaunchServices-registered copy: E2E_APP_PATH=/path/to/The Pair.app
      ...(process.env.E2E_APP_PATH ? { 'appium:appPath': process.env.E2E_APP_PATH } : {}),
      // Env set on the wdio process does not reach an app launched via XCTest;
      // it must be injected through the mac2 launch environment.
      'appium:environment': {
        THE_PAIR_E2E_MOCK: 'true',
        ...(process.env.THE_PAIR_E2E_MOCK_SCENARIO
          ? { THE_PAIR_E2E_MOCK_SCENARIO: process.env.THE_PAIR_E2E_MOCK_SCENARIO }
          : {})
      },
      // First run on a fresh machine compiles WebDriverAgentMac (minutes) and
      // requires a one-time GUI grant: System Settings → Privacy & Security →
      // Accessibility/Automation for the terminal app and Xcode Helper. Until
      // that grant is clicked, the runner hangs at "Running tests...".
      'appium:serverStartupTimeout': 300000,
      'appium:noReset': false,
      'appium:newCommandTimeout': 300
    }
  ],

  logLevel: 'info',
  waitforTimeout: 10000,
  // First session on a fresh machine compiles WebDriverAgentMac via
  // xcodebuild, which can take well over a minute.
  connectionRetryTimeout: 240000,
  connectionRetryCount: 1,

  services: ['appium'],
  appium: {
    command: 'npx',
    args: {
      address: '127.0.0.1',
      port: 4723,
      relaxedSecurity: true
    }
  },

  framework: 'mocha',
  reporters: ['spec'],
  mochaOpts: {
    ui: 'bdd',
    timeout: 60000,
    grep: process.env.E2E_GREP
  },

  // On Node >= 23 wdio's requests are dispatched through Node's internal
  // undici, which rejects the Content-Length header wdio sets manually
  // ("invalid content-length header") because fetch adds its own. Strip it.
  transformRequest(requestOptions) {
    const headers = requestOptions.headers
    if (headers instanceof Headers) {
      headers.delete('content-length')
    }
    return requestOptions
  },

  beforeSession() {
    process.env.THE_PAIR_E2E_MOCK = 'true'
  },

  before() {
    // eslint-disable-next-line @typescript-eslint/no-require-imports
    require('ts-node/register')
  }
}
