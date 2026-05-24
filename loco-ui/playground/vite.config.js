import { fileURLToPath } from 'node:url';
import react from '@vitejs/plugin-react';

export default {
  root: fileURLToPath(new URL('.', import.meta.url)),
  plugins: [react()],
  server: {
    port: 5175,
  },
};
