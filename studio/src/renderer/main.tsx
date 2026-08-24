import React from 'react'
import ReactDOM from 'react-dom/client'
import App from './App'
import { SettingsApp } from './components/SettingsApp'
import './index.css'
import './product-shell.css'

const windowParam = new URLSearchParams(window.location.search).get('window')
const isSettingsWindow = windowParam === 'settings'
document.documentElement.classList.toggle('platform-macos', /Mac/u.test(navigator.platform))
document.documentElement.classList.toggle('is-settings-window', isSettingsWindow)

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    {isSettingsWindow ? <SettingsApp /> : <App />}
  </React.StrictMode>,
)
