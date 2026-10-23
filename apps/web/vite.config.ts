import { fileURLToPath } from 'node:url'

import tailwindcss from '@tailwindcss/vite'
import vue from '@vitejs/plugin-vue'
import AutoImport from 'unplugin-auto-import/vite'
import { defineConfig } from 'vitest/config'

/** The Rust server during development: `holocron serve` on its default port. */
const apiTarget = process.env.HOLOCRON_DEV_API ?? 'http://127.0.0.1:8080'

export default defineConfig({
  plugins: [
    // Only the framework APIs, as in Kit. Composables are imported explicitly.
    AutoImport({
      imports: ['vue', 'vue-router'],
      dts: 'src/auto-imports.d.ts',
      vueTemplate: true,
    }),
    vue(),
    tailwindcss(),
  ],
  resolve: {
    alias: { '~': fileURLToPath(new URL('./src', import.meta.url)) },
  },
  server: {
    port: 5173,
    strictPort: true,
    // changeOrigin stays off: commands must carry the browser's own origin.
    proxy: {
      '/api': apiTarget,
      '/config.json': apiTarget,
    },
  },
  build: {
    target: 'baseline-widely-available',
    // Flags stay files, each loaded when shown, instead of 260 data URLs in the main chunk.
    assetsInlineLimit: (file) => (file.includes('/country-flag-icons/') ? false : undefined),
    rollupOptions: {
      // The replay frame is its own page: the server serves it sandboxed, without network access.
      input: {
        main: fileURLToPath(new URL('./index.html', import.meta.url)),
        replay: fileURLToPath(new URL('./replay-frame.html', import.meta.url)),
      },
      output: {
        // Stable vendors in their own chunks stay cached across deploys.
        advancedChunks: {
          groups: [
            { name: 'vue', test: /node_modules[\\/](vue|@vue|vue-router|vue-i18n|@intlify)[\\/]/ },
            { name: 'query', test: /node_modules[\\/]@tanstack[\\/]/ },
            { name: 'reka', test: /node_modules[\\/](reka-ui|@floating-ui)[\\/]/ },
            { name: 'icons', test: /node_modules[\\/]@iconify(-json)?[\\/]/ },
          ],
        },
      },
    },
  },
  test: {
    include: ['src/**/*.test.ts'],
    environment: 'node',
  },
})
