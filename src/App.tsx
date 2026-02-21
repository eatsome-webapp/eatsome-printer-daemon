import { useState, useEffect } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
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

interface UpdateInfo {
  current_version: string
  latest_version: string
}

function App() {
  const [loading, setLoading] = useState(true)
  const [showWizard, setShowWizard] = useState(false)
  const [showResetConfirm, setShowResetConfirm] = useState(false)
  const [updateAvailable, setUpdateAvailable] = useState<UpdateInfo | null>(null)
  const [updateInstalling, setUpdateInstalling] = useState(false)

  useEffect(() => {
    loadConfig()

    // Disable browser context menu for native app feel
    const handleContextMenu = (e: MouseEvent) => {
      e.preventDefault()
    }
    document.addEventListener('contextmenu', handleContextMenu)

    // Listen for update events (works in both wizard and dashboard)
    const unlistenUpdate = listen<UpdateInfo>('update-available', (event) => {
      setUpdateAvailable(event.payload)
    })
    const unlistenInstalling = listen('update-installing', () => {
      setUpdateInstalling(true)
    })
    const unlistenInstalled = listen('update-installed', () => {
      setUpdateInstalling(false)
      setUpdateAvailable(null)
    })
    const unlistenError = listen('update-error', () => {
      setUpdateInstalling(false)
    })

    return () => {
      document.removeEventListener('contextmenu', handleContextMenu)
      unlistenUpdate.then((f) => f())
      unlistenInstalling.then((f) => f())
      unlistenInstalled.then((f) => f())
      unlistenError.then((f) => f())
    }
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
        version: '0.0.0', // overridden by Rust backend
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

  async function handleInstallUpdate() {
    try {
      setUpdateInstalling(true)
      await invoke('install_update')
    } catch (error) {
      setUpdateInstalling(false)
      console.error('Update failed:', error)
    }
  }

  // Global update banner — renders above both wizard and dashboard
  const updateBanner = updateAvailable && (
    <div
      style={{
        background: 'linear-gradient(135deg, #FFD500 0%, #FFC700 100%)',
        color: '#0A0A0A',
        padding: '8px 16px',
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        gap: '12px',
        fontSize: '13px',
        fontWeight: 600,
      }}
    >
      <span>
        v{updateAvailable.latest_version} beschikbaar (huidig: v{updateAvailable.current_version})
      </span>
      <button
        onClick={handleInstallUpdate}
        disabled={updateInstalling}
        style={{
          background: '#0A0A0A',
          color: '#FFD500',
          border: 'none',
          borderRadius: '6px',
          padding: '4px 12px',
          fontSize: '12px',
          fontWeight: 700,
          cursor: updateInstalling ? 'not-allowed' : 'pointer',
          opacity: updateInstalling ? 0.6 : 1,
        }}
      >
        {updateInstalling ? 'Installeren...' : 'Nu updaten'}
      </button>
    </div>
  )

  if (loading) {
    return (
      <div className="dashboard-loading">
        {updateBanner}
        <div className="spinner spinner-lg"></div>
        <p>Loading...</p>
      </div>
    )
  }

  // Show setup wizard on first run or after reset
  if (showWizard) {
    return (
      <>
        {updateBanner}
        <SetupWizard onComplete={handleWizardComplete} />
      </>
    )
  }

  // Show main dashboard (dashboard has its own update UI, pass state down)
  return (
    <>
      {updateBanner}
      <MainDashboard
        onReset={handleReset}
        updateAvailable={updateAvailable}
        updateInstalling={updateInstalling}
        onInstallUpdate={handleInstallUpdate}
      />
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
