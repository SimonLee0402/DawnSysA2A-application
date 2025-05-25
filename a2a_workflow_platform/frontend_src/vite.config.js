import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'
import path from 'path'
import { nodeResolve } from '@rollup/plugin-node-resolve'
import fs from 'fs'

// https://vitejs.dev/config/
export default defineConfig({
  plugins: [
    vue(),
    nodeResolve({
      browser: true,
      preferBuiltins: true
    })
  ],
  optimizeDeps: {
    include: ['axios', 'pinia', 'vue-router', 'element-plus']
  },
  build: {
    outDir: path.resolve(__dirname, '../static/vue'),
    assetsDir: 'assets',
    sourcemap: true,
    rollupOptions: {
      input: {
        main: path.resolve(__dirname, 'index.html'),
      },
      output: {
        entryFileNames: 'assets/[name].js',
        chunkFileNames: 'assets/[name].js',
        assetFileNames: 'assets/[name].[ext]'
      }
    },
    cssCodeSplit: false,
  },
  server: {
    port: 3000,
    proxy: {
      '/api': {
        target: 'http://127.0.0.1:8000',
        changeOrigin: true,
        bypass: (req, res, proxyOptions) => {
          const baseUrl = req.url.split('?')[0];
          if (baseUrl.startsWith('/api/') && baseUrl.endsWith('.js')) {
            const filePath = path.join(__dirname, baseUrl); 
            if (fs.existsSync(filePath)) {
              console.log(`[Vite Proxy Bypass] Serving directly: ${req.url} (resolved to ${filePath})`);
              return req.url;
            }
          }
          console.log(`[Vite Proxy] Forwarding to Django: ${req.url}`);
          return null; 
        }
      },
      '/accounts': {
        target: 'http://127.0.0.1:8000',
        changeOrigin: true
      },
      '/register': {
        target: 'http://127.0.0.1:8000',
        changeOrigin: true
      },
      '/csrf': {
        target: 'http://127.0.0.1:8000',
        changeOrigin: true
      },
      '/static': {
        target: 'http://127.0.0.1:8000',
        changeOrigin: true
      },
      '/media': {
        target: 'http://127.0.0.1:8000',
        changeOrigin: true
      }
    }
  },
  resolve: {
    alias: {
      '@': path.resolve(__dirname)
    }
  }
}) 