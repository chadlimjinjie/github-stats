import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import './index.css'
import App from './App.tsx'
import { userManager } from '@/lib/auth'

const root = createRoot(document.getElementById('root')!)

// Finish sign-in before rendering: the authorization code is single use, and StrictMode runs effects twice.
if (location.pathname === '/callback') {
  try {
    await userManager.signinRedirectCallback()
    history.replaceState(null, '', '/')
  } catch (e) {
    root.render(
      <main className="mx-auto grid max-w-sm gap-3 p-8">
        <h1 className="text-xl font-semibold">Sign-in failed</h1>
        <p role="alert" className="text-sm text-destructive">
          {e instanceof Error ? e.message : String(e)}
        </p>
        <a href="/" className="underline">
          Try again
        </a>
      </main>,
    )
    throw e
  }
}

root.render(
  <StrictMode>
    <App />
  </StrictMode>,
)
