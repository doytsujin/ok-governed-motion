/// <reference types="vitest" />
import { defineConfig } from "vite";

// Relative base so the build works from any host or subpath, the same
// convention the agentic-doom-web build uses.
export default defineConfig({
  base: "./",
  test: {
    environment: "node",
    include: ["src/**/*.test.ts"],
  },
});
