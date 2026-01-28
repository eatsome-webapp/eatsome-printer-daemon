import { useState, useEffect } from 'react'
import { invoke } from '@tauri-apps/api/core'
import SetupWizard from './components/SetupWizard'
import './App.css'

interface AppConfig {
  version: string
  restaurant_id: string | null
  location_id: string | null
  auth_token: string | null
  supabase_url: string
  supabase_anon_key: string
  service_role_key: string
  printers: any[]
}

function App() {
  const [config, setConfig] = useState<AppConfig | null>(null)
  const [loading, setLoading] = useState(true)
  const [discoveredPrinters, setDiscoveredPrinters] = useState<any[]>([])
  const [discovering, setDiscovering] = useState(false)
  const [showWizard, setShowWizard] = useState(false)

  useEffect(() => {
    loadConfig()
  }, [])

  async function loadConfig() {
    try {
      const cfg = await invoke<AppConfig>('get_config')
      setConfig(cfg)

      // Show wizard if restaurant_id is not configured
      if (!cfg.restaurant_id) {
        setShowWizard(true)
      }
    } catch (error) {
      console.error('Failed to load config:', error)
    } finally {
      setLoading(false)
    }
  }

  function handleWizardComplete() {
    setShowWizard(false)
    loadConfig() // Reload config after setup
  }

  async function discoverPrinters() {
    setDiscovering(true)
    try {
      const printers = await invoke<any[]>('discover_printers')
      setDiscoveredPrinters(printers)
    } catch (error) {
      console.error('Failed to discover printers:', error)
    } finally {
      setDiscovering(false)
    }
  }

  async function testPrint(printerId: string) {
    try {
      await invoke('test_print', { printerId })
      alert('Test print sent successfully!')
    } catch (error) {
      console.error('Failed to send test print:', error)
      alert(`Test print failed: ${error}`)
    }
  }

  if (loading) {
    return (
      <div className="container">
        <h1>Loading...</h1>
      </div>
    )
  }

  // Show setup wizard on first run
  if (showWizard) {
    return <SetupWizard onComplete={handleWizardComplete} />
  }

  return (
    <div className="container">
      <header>
        <h1>Eatsome Printer Service</h1>
        <p className="subtitle">Thermal Printer Management</p>
      </header>

      <section className="config-section">
        <h2>Configuration</h2>
        {config ? (
          <div className="config-details">
            <p>
              <strong>Version:</strong> {config.version}
            </p>
            <p>
              <strong>Restaurant ID:</strong> {config.restaurant_id || 'Not configured'}
            </p>
            <p>
              <strong>Location ID:</strong> {config.location_id || 'Not configured'}
            </p>
            <p>
              <strong>Supabase URL:</strong> {config.supabase_url}
            </p>
            <p>
              <strong>Printers Configured:</strong> {config.printers.length}
            </p>
          </div>
        ) : (
          <p>No configuration loaded</p>
        )}
      </section>

      <section className="printer-section">
        <h2>Printer Discovery</h2>
        <button onClick={discoverPrinters} disabled={discovering}>
          {discovering ? 'Discovering...' : 'Discover Printers'}
        </button>

        {discoveredPrinters.length > 0 && (
          <div className="printer-list">
            <h3>Discovered Printers ({discoveredPrinters.length})</h3>
            {discoveredPrinters.map((printer, index) => (
              <div key={index} className="printer-card">
                <h4>{printer.name}</h4>
                <p>
                  <strong>ID:</strong> {printer.id}
                </p>
                <p>
                  <strong>Type:</strong> {printer.connection_type}
                </p>
                <p>
                  <strong>Address:</strong> {printer.address}
                </p>
                <p>
                  <strong>Vendor:</strong> {printer.vendor}
                </p>
                <button onClick={() => testPrint(printer.id)}>Test Print</button>
              </div>
            ))}
          </div>
        )}
      </section>

      <footer>
        <p className="version">v{config?.version || '1.0.0'}</p>
        <p className="copyright">© 2024 Eatsome B.V.</p>
      </footer>
    </div>
  )
}

export default App
