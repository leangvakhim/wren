import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import { fileURLToPath } from 'node:url';

// The phone page: built into relay/public, which the relay serves.
// `base: './'` so it also works when the relay lives under a path.
const here = fileURLToPath(new URL('.', import.meta.url));

export default defineConfig({
  root: here,
  base: './',
  plugins: [svelte({ configFile: false })],
  build: {
    outDir: fileURLToPath(new URL('../relay/public', import.meta.url)),
    emptyOutDir: true,
    target: 'es2022',
    // No inline scripts or data: modules (the relay's CSP forbids them).
    assetsInlineLimit: 0,
  },
  server: { port: 1430, strictPort: true },
});
