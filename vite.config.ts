import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'
export default defineConfig({ plugins: [vue()], server: { port: 1420, strictPort: true, host: '127.0.0.1' }, clearScreen: false })
