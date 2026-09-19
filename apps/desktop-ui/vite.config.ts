import react from "@vitejs/plugin-react";
import { defineConfig } from "vitest/config";

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
  },
  test: {
    environment: "jsdom",
    setupFiles: ["./src/test/setup.ts"],
    // Playwright 的 tests/ 目录由独立的 @playwright/test 运行，不能被 Vitest 收集。
    include: ["src/**/*.test.{ts,tsx}"],
    css: true,
  },
});
