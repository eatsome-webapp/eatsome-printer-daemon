import { useState, useEffect } from 'react'
import { invoke } from '@tauri-apps/api/core'
import SetupWizard from './components/SetupWizard'
import MainDashboard from './components/MainDashboard'
import ConfirmDialog from './components/ConfirmDialog'

interface AppConfig {
  version: string
  restaurant_id: string | null
  location_id: string | null
  auth_token: string | null
  supabase_url: string
  supabase_anon_key: string
  printers: any[]
}

function App() {
  const [loading, setLoading] = useState(true)
  const [showWizard, setShowWizard] = useState(false)
  const [showResetConfirm, setShowResetConfirm] = useState(false)

  useEffect(() => {
    loadConfig()
  }, [])

  async function loadConfig() {
    try {
      const cfg = await invoke<AppConfig>('get_config')

      // Show wizard if restaurant_id is not configured
      if (!cfg.restaurant_id) {
        setShowWizard(true)
      } else {
        // Auto-connect to Realtime on startup
        try {
          await invoke('start_polling', { restaurantId: cfg.restaurant_id })
        } catch (error) {
          console.warn('Auto-connect on startup failed:', error)
        }
      }
    } catch (error) {
      console.error('Failed to load config:', error)
      // Show wizard on config load error (first run)
      setShowWizard(true)
    } finally {
      setLoading(false)
    }
  }

  async function handleWizardComplete() {
    setShowWizard(false)
    // loadConfig() already calls start_polling when restaurant_id is set
    await loadConfig()
  }

  function handleReset() {
    setShowResetConfirm(true)
  }

  async function confirmReset() {
    setShowResetConfirm(false)

    try {
      const defaultConfig: AppConfig = {
        version: '1.0.0',
        restaurant_id: null,
        location_id: null,
        auth_token: null,
        supabase_url: 'https://gtlpzikuozrdgomsvqmo.supabase.co',
        supabase_anon_key:
          'eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJpc3MiOiJzdXBhYmFzZSIsInJlZiI6Imd0bHB6aWt1b3pyZGdvbXN2cW1vIiwicm9sZSI6ImFub24iLCJpYXQiOjE3NjIxMDA1NTksImV4cCI6MjA3NzY3NjU1OX0.Yi1a1-wv-qvN9NVZhqYqQEQ_4H8FMKVANsyEipzHGfA',
        printers: [],
      }

      await invoke('save_config', { config: defaultConfig })

      // Clear all print jobs from the queue
      try {
        await invoke('clear_queue')
      } catch (e) {
        console.warn('Failed to clear queue during reset:', e)
      }

      setShowWizard(true)
    } catch (error) {
      console.error('Failed to reset config:', error)
    }
  }

  if (loading) {
    return (
      <div className="dashboard-loading">
        <div className="spinner spinner-lg"></div>
        <p>Loading...</p>
      </div>
    )
  }

  // Show setup wizard on first run or after reset
  if (showWizard) {
    return <SetupWizard onComplete={handleWizardComplete} />
  }

  // Show main dashboard
  return (
    <>
      <MainDashboard onReset={handleReset} />
      {showResetConfirm && (
        <ConfirmDialog
          title="Reset & Reconfigure"
          message="This will delete all configuration including printers, credentials, and restaurant settings. You'll need to set up the app again."
          confirmLabel="Reset Everything"
          variant="danger"
          onConfirm={confirmReset}
          onCancel={() => setShowResetConfirm(false)}
        />
      )}
    </>
  )
}

export default App
