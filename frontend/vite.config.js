import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';

const runtimeTarget = process.env.CAPSULE_RUNTIME_TARGET || 'http://localhost:3001';

export default defineConfig({
  plugins: [svelte()],
  optimizeDeps: {
    include: ['monaco-editor'],
  },
  server: {
    port: 3002,
    proxy: {
      '/ws': {
        target: runtimeTarget,
        ws: true,
      },
      '/api': {
        target: runtimeTarget,
      },
      '/observal': {
        target: runtimeTarget,
      },
      '/assets': {
        target: runtimeTarget,
      },
      '/fonts': {
        target: runtimeTarget,
      },
      '/observal-logo.svg': {
        target: runtimeTarget,
      },
    },
  },
});
