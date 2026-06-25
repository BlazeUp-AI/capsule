import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';

export default defineConfig({
  plugins: [svelte()],
  optimizeDeps: {
    include: ['monaco-editor'],
  },
  server: {
    port: 3002,
    proxy: {
      '/ws': {
        target: 'http://localhost:3001',
        ws: true,
      },
      '/api': {
        target: 'http://localhost:3001',
      },
      '/observal': {
        target: 'http://localhost:3001',
      },
      '/assets': {
        target: 'http://localhost:3001',
      },
      '/fonts': {
        target: 'http://localhost:3001',
      },
      '/observal-logo.svg': {
        target: 'http://localhost:3001',
      },
    },
  },
});
