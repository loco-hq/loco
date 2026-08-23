import react from '@vitejs/plugin-react';
import { defineConfig } from 'vite';

const apiProxy = {
  '/auth': 'http://localhost:3000',
  '/config': 'http://localhost:3000',
  '/schema': 'http://localhost:3000',
  '/data': 'http://localhost:3000',
};

export default defineConfig(({ command }) => ({
  plugins: [react()],
  // Relative base is a build concern (python / S3 subfolder / file://).
  // Dev keeps '/' so HMR / @vite/client are unaffected.
  base: command === 'build' ? './' : '/',
  server: { port: 5174, proxy: apiProxy },
  preview: { port: 5174, proxy: apiProxy },
}));
