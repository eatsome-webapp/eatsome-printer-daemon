import { useState, useEffect } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import {
  Printer,
  Settings,
  X,
  ClipboardList,
  CircleCheck,
  CircleX,
  Timer,
  BarChart3,
  Wifi,
  WifiOff,
  RefreshCw,
  Plus,
  TestTube,
  Trash2,
} from 'lucide-react'
import ConfirmDialog from './ConfirmDialog'
import './MainDashboard.css'

interface AppConfig {
  version: string
  restaurant_id: string | null
  location_id: string | null
  auth_token: string | null
  supabase_url: string
  supabase_anon_key: string
  printers: PrinterConfig[]
}

interface PrinterConfig {
  id: string
  name: string
  connection_type: string
  address: string
  protocol: string
  station: string | null
  is_primary: boolean
  capabilities: {
    cutter: boolean
    drawer: boolean
    qrcode: boolean
    max_width: number
  }
}

interface QueueStats {
  total: number
  pending: number
  processing: number
  completed: number
  failed: number
}

interface MainDashboardProps {
  onReset: () => void
}

export default function MainDashboard({ onReset }: MainDashboardProps) {
  const [config, setConfig] = useState<AppConfig | null>(null)
  const [queueStats, setQueueStats] = useState<QueueStats | null>(null)
  const [uptime, setUptime] = useState<number>(0)
  const [showSettings, setShowSettings] = useState(false)
  const [editRestaurantId, setEditRestaurantId] = useState('')
  const [connectionState, setConnectionState] = useState<'connected' | 'disconnected'>(
    'disconnected'
  )
  const [removePrinterId, setRemovePrinterId] = useState<string | null>(null)
  const [errorMessage, setErrorMessage] = useState<string | null>(null)

  useEffect(() => {
    loadConfig()
    loadQueueStats()
    loadUptime()
    checkConnection()

    const unlistenStats = listen<QueueStats>('queue-stats-updated', (event) => {
      setQueueStats(event.payload)
    })

    const interval = setInterval(() => {
      loadQueueStats()
      loadUptime()
      checkConnection()
    }, 5000)

    return () => {
      clearInterval(interval)
      unlistenStats.then((fn) => fn())
    }
  }, [])

  async function loadConfig() {
    try {
      const cfg = await invoke<AppConfig>('get_config')
      setConfig(cfg)
      setEditRestaurantId(cfg.restaurant_id || '')
    } catch (error) {
      console.error('Failed to load config:', error)
    }
  }

  async function loadQueueStats() {
    try {
      const stats = await invoke<QueueStats>('get_queue_stats')
      setQueueStats(stats)
    } catch (error) {
      console.error('Failed to load queue stats:', error)
    }
  }

  async function checkConnection() {
    try {
      const state = await invoke<string>('get_connection_state')
      setConnectionState(state as 'connected' | 'disconnected')
    } catch (error) {
      setConnectionState('disconnected')
    }
  }

  async function loadUptime() {
    try {
      const secs = await invoke<number>('get_uptime')
      setUptime(secs)
    } catch (error) {
      console.error('Failed to load uptime:', error)
    }
  }

  function formatUptime(seconds: number): string {
    if (seconds < 60) return `${seconds}s`
    if (seconds < 3600) return `${Math.floor(seconds / 60)}m`
    const h = Math.floor(seconds / 3600)
    const m = Math.floor((seconds % 3600) / 60)
    return `${h}h ${m}m`
  }

  async function handleSaveSettings() {
    if (!config) return

    try {
      const updatedConfig = {
        ...config,
        restaurant_id: editRestaurantId || null,
      }

      await invoke('save_config', { config: updatedConfig })
      await loadConfig()
      setShowSettings(false)

      if (editRestaurantId) {
        await invoke('start_polling', { restaurantId: editRestaurantId })
      }
    } catch (error) {
      console.error('Failed to save settings:', error)
      setErrorMessage(`Failed to save settings: ${error}`)
    }
  }

  async function handleTestPrint(printerId: string) {
    try {
      await invoke('test_print', { printerId })
    } catch (error) {
      console.error('Test print failed:', error)
      setErrorMessage(`Test print failed: ${error}`)
    }
  }

  function handleRemovePrinter(printerId: string) {
    setRemovePrinterId(printerId)
  }

  async function confirmRemovePrinter() {
    if (!removePrinterId) return
    const printerId = removePrinterId
    setRemovePrinterId(null)

    try {
      await invoke('remove_printer', { printerId })
      await loadConfig()
    } catch (error) {
      console.error('Failed to remove printer:', error)
    }
  }

  async function handleAddPrinters() {
    try {
      const printers = await invoke<any[]>('discover_printers')
      if (printers.length === 0) {
        setErrorMessage('No printers found. Make sure your printer is connected and turned on.')
        return
      }

      const cfg = await invoke<AppConfig>('get_config')
      const existingIds = new Set(cfg.printers.map((p) => p.id))
      const newPrinters = printers
        .filter((p) => !existingIds.has(p.id))
        .map((p) => ({
          id: p.id,
          name: p.name,
          connection_type: p.connection_type.toLowerCase(),
          address: p.address,
          protocol: 'escpos',
          station: null,
          is_primary: false,
          capabilities: {
            cutter: true,
            drawer: false,
            qrcode: true,
            max_width: 48,
          },
        }))

      if (newPrinters.length === 0) {
        setErrorMessage('All discovered printers are already configured.')
        return
      }

      cfg.printers = [...cfg.printers, ...newPrinters]
      await invoke('save_config', { config: cfg })
      await loadConfig()
    } catch (error) {
      console.error('Failed to discover printers:', error)
      setErrorMessage(`Failed to discover printers: ${error}`)
    }
  }

  async function handleReconnect() {
    if (!config?.restaurant_id) return

    try {
      await invoke('stop_polling')
      await invoke('start_polling', { restaurantId: config.restaurant_id })
      await checkConnection()
    } catch (error) {
      console.error('Failed to reconnect:', error)
      setErrorMessage(`Failed to reconnect: ${error}`)
    }
  }

  if (!config) {
    return (
      <div className="dashboard-loading">
        <div className="spinner spinner-lg"></div>
        <p>Loading dashboard...</p>
      </div>
    )
  }

  return (
    <div className="main-dashboard">
      {/* Header */}
      <header className="dashboard-header">
        <div className="header-left">
          <div className="logo-icon">
            <Printer size={20} />
          </div>
          <div>
            <h1>Eatsome Printer Service</h1>
            <p className="subtitle">Restaurant: {config.restaurant_id || 'Not configured'}</p>
          </div>
        </div>
        <div className="header-right">
          <div className={`connection-badge ${connectionState}`}>
            {connectionState === 'connected' ? <Wifi size={14} /> : <WifiOff size={14} />}
            {connectionState === 'connected' ? 'Connected' : 'Disconnected'}
          </div>
          <button className="btn-icon" onClick={() => setShowSettings(true)} title="Instellingen">
            <Settings size={18} />
          </button>
        </div>
      </header>

      {/* Error Banner */}
      {errorMessage && (
        <div className="error-banner">
          <span>{errorMessage}</span>
          <button className="btn-close" onClick={() => setErrorMessage(null)}>
            <X size={14} />
          </button>
        </div>
      )}

      {/* Stats Strip */}
      <div className="stats-strip">
        <div className="stat-cell">
          <Printer size={14} />
          <span className="stat-val">{config.printers.length}</span>
          <span className="stat-lbl">Printers</span>
        </div>
        <div className="stat-cell">
          <ClipboardList size={14} />
          <span className="stat-val">{queueStats?.pending || 0}</span>
          <span className="stat-lbl">Queue</span>
        </div>
        <div className="stat-cell stat-success">
          <CircleCheck size={14} />
          <span className="stat-val">{queueStats?.completed || 0}</span>
          <span className="stat-lbl">Done</span>
        </div>
        <div className="stat-cell stat-danger">
          <CircleX size={14} />
          <span className="stat-val">{queueStats?.failed || 0}</span>
          <span className="stat-lbl">Failed</span>
        </div>
        <div className="stat-cell">
          <Timer size={14} />
          <span className="stat-val">{formatUptime(uptime)}</span>
          <span className="stat-lbl">Uptime</span>
        </div>
        <div className="stat-cell">
          <BarChart3 size={14} />
          <span className="stat-val">{queueStats?.total || 0}</span>
          <span className="stat-lbl">Total</span>
        </div>
      </div>

      {/* Printers Section */}
      <div className="printers-section">
        <div className="section-header">
          <h2>Configured Printers</h2>
          <div className="section-actions">
            <button className="btn-sm btn-secondary" onClick={handleReconnect}>
              <RefreshCw size={14} />
              Reconnect
            </button>
            <button className="btn-sm btn-primary" onClick={handleAddPrinters}>
              <Plus size={14} />
              Add
            </button>
          </div>
        </div>

        <div className="printers-list">
          {config.printers.length === 0 ? (
            <div className="empty-state">
              <p>No printers configured</p>
            </div>
          ) : (
            config.printers.map((printer) => (
              <div key={printer.id} className="printer-row">
                <div className="printer-row-main">
                  <Printer size={16} className="printer-row-icon" />
                  <div className="printer-row-info">
                    <div className="printer-row-name">
                      {printer.name}
                      {printer.is_primary && <span className="badge-primary">Primary</span>}
                    </div>
                    <div className="printer-row-meta">
                      {printer.connection_type.toUpperCase()}
                      {printer.address && <> &middot; {printer.address}</>}
                      {printer.station && <> &middot; {printer.station}</>} &middot;{' '}
                      {[
                        printer.capabilities.cutter && 'Cutter',
                        printer.capabilities.qrcode && 'QR',
                        printer.capabilities.drawer && 'Drawer',
                      ]
                        .filter(Boolean)
                        .join(', ')}
                    </div>
                  </div>
                </div>
                <div className="printer-row-actions">
                  <button
                    className="btn-icon-sm"
                    onClick={() => handleTestPrint(printer.id)}
                    title="Test Print"
                  >
                    <TestTube size={14} />
                  </button>
                  <button
                    className="btn-icon-sm btn-icon-danger"
                    onClick={() => handleRemovePrinter(printer.id)}
                    title="Remove Printer"
                  >
                    <Trash2 size={14} />
                  </button>
                </div>
              </div>
            ))
          )}
        </div>
      </div>

      {/* Settings Modal */}
      {showSettings && (
        <div className="modal-overlay" onClick={() => setShowSettings(false)}>
          <div className="modal-content" onClick={(e) => e.stopPropagation()}>
            <div className="modal-header">
              <h2>
                <Settings size={18} /> Instellingen
              </h2>
              <button className="btn-close" onClick={() => setShowSettings(false)}>
                <X size={16} />
              </button>
            </div>

            <div className="modal-body">
              <div className="form-group">
                <label>Restaurant Code</label>
                <input
                  type="text"
                  value={editRestaurantId}
                  onChange={(e) => setEditRestaurantId(e.target.value)}
                  placeholder="e.g. W434N"
                />
                <p className="form-hint">Je restaurant code uit het admin panel</p>
              </div>

              <div className="form-group">
                <label>Verbinding</label>
                <div className={`settings-connection-status ${connectionState}`}>
                  {connectionState === 'connected' ? <Wifi size={14} /> : <WifiOff size={14} />}
                  {connectionState === 'connected'
                    ? 'Verbonden met Eatsome Cloud'
                    : 'Niet verbonden'}
                </div>
              </div>
            </div>

            <div className="modal-footer">
              <button className="btn-secondary" onClick={() => setShowSettings(false)}>
                Annuleren
              </button>
              <button className="btn-primary" onClick={handleSaveSettings}>
                Opslaan
              </button>
            </div>

            <div className="settings-danger-zone">
              <h3>Gevarenzone</h3>
              <p>
                Verwijder alle configuratie inclusief printers, credentials en
                restaurant-instellingen. Je moet de app opnieuw instellen.
              </p>
              <button className="btn-danger" onClick={onReset}>
                Reset & Opnieuw configureren
              </button>
            </div>
          </div>
        </div>
      )}

      {removePrinterId && (
        <ConfirmDialog
          title="Remove Printer"
          message="Are you sure you want to remove this printer? You can re-discover it later."
          confirmLabel="Remove"
          variant="danger"
          onConfirm={confirmRemovePrinter}
          onCancel={() => setRemovePrinterId(null)}
        />
      )}
    </div>
  )
}
