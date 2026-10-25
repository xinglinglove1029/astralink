import { createApp } from 'vue'
import App from './App.vue'
import './style.css'
import './themes.css'
import './dashboard.css'
import './motion.css'
import { initializeAppearance } from './appearance'

initializeAppearance()
createApp(App).mount('#app')
