import { defineConfig } from '@playwright/test'

export default defineConfig({
  testDir: './e2e',
  timeout: 30_000,
  use: {
    baseURL: 'http://127.0.0.1:4173',
    headless: true,
  },
  webServer: {
    command:
      'npm run build && cargo run --manifest-path ../Cargo.toml -q -p cli -- dev gen-fixture ../target/e2e-demo.mlirtrace && cargo run --manifest-path ../Cargo.toml -q -p cli -- serve ../target/e2e-demo.mlirtrace --listen 127.0.0.1:4173',
    url: 'http://127.0.0.1:4173',
    reuseExistingServer: false,
    timeout: 120_000,
  },
})
