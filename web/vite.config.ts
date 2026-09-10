import { fileURLToPath, URL } from 'node:url';
import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import tailwindcss from '@tailwindcss/vite';

// Build only into web/dist. Never overwrite embedded assets or activate a release.
export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: { alias: { '@': fileURLToPath(new URL('./src', import.meta.url)) } },
  publicDir: false,
  build: {
    outDir: 'dist',
    cssCodeSplit: false,
    modulePreload: false,
    sourcemap: false,
    assetsInlineLimit: 0,
    rolldownOptions: {
      output: {
        entryFileNames: 'app.js',
        assetFileNames: (asset) => {
          if (asset.names.length !== 1 || !asset.names[0].endsWith('.css')) {
            throw new Error('UI package format 1 only permits the CSS asset; no images/fonts/workers');
          }
          return 'style.css';
        },
        codeSplitting: false,
      },
    },
  },
});
