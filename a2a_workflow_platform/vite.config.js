import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'
import path from 'path'

// https://vitejs.dev/config/
export default defineConfig({
  plugins: [vue()],
  root: 'frontend_src', // 设置根目录为 frontend_src
  resolve: {
    alias: {
      '@': path.resolve(__dirname, './frontend_src')
    }
  },
  build: {
    // 输出目录设置为Django的静态资源目录
    outDir: path.resolve(__dirname, './static/vue'),
    emptyOutDir: true,
    // 生成静态资源的名称格式
    assetsDir: 'assets',
    manifest: true,
  },
  server: {
    // 开发服务器配置
    host: 'localhost',
    port: 3000,
    // 代理API请求到Django后端
    proxy: {
      '/api': {
        target: 'http://localhost:8000',
        changeOrigin: true
      },
      '/media': {
        target: 'http://localhost:8000',
        changeOrigin: true
      }
    }
  }
}) 