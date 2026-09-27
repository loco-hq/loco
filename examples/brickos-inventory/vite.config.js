import react from '@vitejs/plugin-react';
import { defineConfig } from 'vite';

const apiProxy = {
  '/auth': 'http://localhost:3000',
  '/data': 'http://localhost:3000',
};

export default defineConfig(({ command }) => ({
  plugins: [react()],
  // The bundle is served from the site's root, but a relative base keeps it
  // working under any path (docs/hosting.md, "Frontend contract").
  base: command === 'build' ? './' : '/',
  server: { port: 5177, proxy: apiProxy },
}));
