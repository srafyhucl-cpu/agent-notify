import { defineConfig, devices } from "@playwright/test";
import process from "node:process";
import { fileURLToPath } from "node:url";

const appRoot = fileURLToPath(new URL("..", import.meta.url));
const port = 1421;
const baseURL = `http://127.0.0.1:${String(port)}`;

export default defineConfig({
  testDir: ".",
  testMatch: /.*\.spec\.ts/,
  outputDir: fileURLToPath(new URL("../test-results", import.meta.url)),
  fullyParallel: true,
  forbidOnly: Boolean(process.env.CI),
  retries: process.env.CI ? 2 : 0,
  workers: process.env.CI ? 1 : undefined,
  reporter: [
    ["list"],
    ["html", { open: "never", outputFolder: fileURLToPath(new URL("../playwright-report", import.meta.url)) }],
  ],
  expect: {
    timeout: 10_000,
    toHaveScreenshot: {
      animations: "disabled",
      caret: "hide",
      maxDiffPixelRatio: 0.01,
    },
  },
  use: {
    baseURL,
    trace: "retain-on-failure",
    screenshot: "only-on-failure",
    video: "off",
  },
  projects: [
    {
      name: "desktop",
      grepInvert: /@zoom/,
      use: {
        ...devices["Desktop Chrome"],
        viewport: { width: 1280, height: 800 },
        deviceScaleFactor: 1,
      },
    },
    {
      // 640x400 配合 2 倍像素密度等价于 1280x800 窗口下的 200% 缩放。
      name: "zoom-200",
      grep: /@zoom/,
      use: {
        ...devices["Desktop Chrome"],
        viewport: { width: 640, height: 400 },
        deviceScaleFactor: 2,
      },
    },
  ],
  webServer: {
    command: `npm run dev -- --host 127.0.0.1 --port ${String(port)} --strictPort`,
    cwd: appRoot,
    url: `${baseURL}/tests/harness/`,
    reuseExistingServer: !process.env.CI,
    timeout: 120_000,
    env: {
      npm_config_cache: "D:\\Temp\\npm-cache",
      TEMP: "D:\\Temp\\agentnotify-temp",
      TMP: "D:\\Temp\\agentnotify-temp",
    },
  },
});
