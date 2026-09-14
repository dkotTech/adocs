import { defineConfig, type Plugin } from 'vite';
import preact from '@preact/preset-vite';
import { resolve } from 'path';

const BACKEND = 'http://localhost:8080';

// In dev mode /docs/* serves index.html; in production the Rust server does it.
function docsFallback(): Plugin {
  return {
    name: 'docs-fallback',
    configureServer(server) {
      server.middlewares.use((req, _res, next) => {
        const url = req.url ?? '';
        if (url === '/docs' || url.startsWith('/docs/')) req.url = '/index.html';
        next();
      });
    },
  };
}

export default defineConfig({
  plugins: [preact(), docsFallback()],
  server: {
    proxy: {
      '/api': BACKEND,
      '/raw': BACKEND,
      '/llms.txt': BACKEND,
      '/robots.txt': BACKEND,
      '/sitemap.xml': BACKEND,
      '/pub': BACKEND,
    },
  },
  build: {
    rollupOptions: {
      input: { index: resolve(__dirname, 'index.html') },
    },
  },
});
