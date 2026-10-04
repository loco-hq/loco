import react from '@vitejs/plugin-react';
import { defineConfig } from 'vite';

// docs/brocksbricks.md runs its server on :3200, beside the BrickLink mock on
// :3100. LOCO_ORIGIN points the dev proxy elsewhere.
const target = process.env.LOCO_ORIGIN ?? 'http://localhost:3200';
const apiProxy = Object.fromEntries(['/auth', '/data', '/schema', '/actions'].map((p) => [p, target]));

export default defineConfig(({ command }) => ({
  plugins: [react()],
  // The bundle is served from the site's root, but a relative base keeps it
  // working under any path (docs/hosting.md, "Frontend contract").
  base: command === 'build' ? './' : '/',
  server: { port: 5178, proxy: apiProxy },
}));
