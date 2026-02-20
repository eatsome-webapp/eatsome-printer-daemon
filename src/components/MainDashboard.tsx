import { useState, useEffect } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import {
  Printer,
  Settings,
  X,
  CheckCircle,
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
  Download,
  Loader2,
} from 'lucide-react'
import ConfirmDialog from './ConfirmDialog'
import DiscoveryModal from './DiscoveryModal'
import type { DiscoveredPrinter } from './DiscoveryModal'
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
  const [updateAvailable, setUpdateAvailable] = useState<{
    current_version: string
    latest_version: string
  } | null>(null)
  const [updateInstalling, setUpdateInstalling] = useState(false)
  const [updateChecking, setUpdateChecking] = useState(false)
  const [updateCheckResult, setUpdateCheckResult] = useState<'up-to-date' | 'error' | null>(null)
  const [showDiscovery, setShowDiscovery] = useState(false)
  const [testPrintStates, setTestPrintStates] = useState<
    Map<string, 'idle' | 'printing' | 'success' | 'error'>
  >(new Map())

  useEffect(() => {
    loadConfig()
    loadQueueStats()
    loadUptime()
    checkConnection()

    const unlistenStats = listen<QueueStats>('queue-stats-updated', (event) => {
      setQueueStats(event.payload)
    })

    const unlistenUpdate = listen<{ current_version: string; latest_version: string }>(
      'update-available',
      (event) => {
        setUpdateAvailable(event.payload)
      }
    )

    const unlistenInstalling = listen('update-installing', () => {
      setUpdateInstalling(true)
    })

    const unlistenInstalled = listen('update-installed', () => {
      setUpdateInstalling(false)
      setUpdateAvailable(null)
    })

    const unlistenError = listen<string>('update-error', (event) => {
      setUpdateInstalling(false)
      setErrorMessage(`Update mislukt: ${event.payload}`)
    })

    const interval = setInterval(() => {
      loadQueueStats()
      loadUptime()
      checkConnection()
    }, 5000)

    return () => {
      clearInterval(interval)
      unlistenStats.then((fn) => fn())
      unlistenUpdate.then((fn) => fn())
      unlistenInstalling.then((fn) => fn())
      unlistenInstalled.then((fn) => fn())
      unlistenError.then((fn) => fn())
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
    setTestPrintStates((prev) => new Map(prev).set(printerId, 'printing'))
    try {
      await invoke('test_print', { printerId })
      setTestPrintStates((prev) => new Map(prev).set(printerId, 'success'))
      setTimeout(() => {
        setTestPrintStates((prev) => {
          const next = new Map(prev)
          next.delete(printerId)
          return next
        })
      }, 2000)
    } catch (error) {
      console.error('Test print failed:', error)
      setErrorMessage(`Test print failed: ${error}`)
      setTestPrintStates((prev) => new Map(prev).set(printerId, 'error'))
      setTimeout(() => {
        setTestPrintStates((prev) => {
          const next = new Map(prev)
          next.delete(printerId)
          return next
        })
      }, 3000)
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

  function handleAddPrinters() {
    setShowDiscovery(true)
  }

  async function handleAddSelectedPrinters(printers: DiscoveredPrinter[]) {
    if (!config || printers.length === 0) return

    try {
      const newPrinters: PrinterConfig[] = printers.map((p) => ({
        id: p.id,
        name: p.name,
        connection_type: p.connection_type.toLowerCase(),
        address: p.address,
        protocol: p.protocol === 'escpos' ? 'escpos' : 'escpos',
        station: null,
        is_primary: false,
        capabilities: p.capabilities
          ? {
              cutter: (p.capabilities as Record<string, unknown>).cutter === true,
              drawer: (p.capabilities as Record<string, unknown>).drawer === true,
              qrcode: (p.capabilities as Record<string, unknown>).qrcode === true,
              max_width:
                typeof (p.capabilities as Record<string, unknown>).maxWidth === 'number'
                  ? ((p.capabilities as Record<string, unknown>).maxWidth as number)
                  : 48,
            }
          : { cutter: true, drawer: false, qrcode: true, max_width: 48 },
      }))

      const updatedConfig = {
        ...config,
        printers: [...config.printers, ...newPrinters],
      }

      await invoke('save_config', { config: updatedConfig })
      await loadConfig()
    } catch (error) {
      console.error('Failed to add printers:', error)
      setErrorMessage(`Failed to add printers: ${error}`)
    }
  }

  async function handleInstallUpdate() {
    try {
      setUpdateInstalling(true)
      await invoke('install_update')
    } catch (error) {
      setUpdateInstalling(false)
      setErrorMessage(`Update mislukt: ${error}`)
    }
  }

  async function handleCheckForUpdates() {
    try {
      setUpdateChecking(true)
      setUpdateCheckResult(null)
      const result = await invoke<{
        available: boolean
        current_version: string
        latest_version?: string
      }>('check_for_updates')
      if (result.available) {
        setUpdateAvailable({
          current_version: result.current_version,
          latest_version: result.latest_version!,
        })
        setUpdateCheckResult(null)
      } else {
        setUpdateCheckResult('up-to-date')
      }
    } catch (error) {
      setUpdateCheckResult('error')
      setErrorMessage(`Update check mislukt: ${error}`)
    } finally {
      setUpdateChecking(false)
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

      {/* Update Banner */}
      {updateAvailable && (
        <div className="update-banner">
          <div className="update-banner-text">
            <Download size={16} />
            <span>
              Versie {updateAvailable.latest_version} beschikbaar
              <span className="update-current"> (huidig: {updateAvailable.current_version})</span>
            </span>
          </div>
          <button
            className="btn-sm btn-update"
            onClick={handleInstallUpdate}
            disabled={updateInstalling}
          >
            {updateInstalling ? (
              <>
                <Loader2 size={14} className="spin" />
                Installeren...
              </>
            ) : (
              'Nu updaten'
            )}
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
                  {(() => {
                    const testState = testPrintStates.get(printer.id) || 'idle'
                    return (
                      <button
                        className={`btn-icon-sm ${testState === 'success' ? 'btn-icon-success' : testState === 'error' ? 'btn-icon-danger' : ''}`}
                        onClick={() => handleTestPrint(printer.id)}
                        title="Test Print"
                        disabled={testState === 'printing'}
                      >
                        {testState === 'printing' ? (
                          <Loader2 size={14} className="spin" />
                        ) : testState === 'success' ? (
                          <CheckCircle size={14} />
                        ) : (
                          <TestTube size={14} />
                        )}
                      </button>
                    )
                  })()}
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

              <div className="settings-info-row">
                <span className="settings-info-label">Verbinding</span>
                <div className={`connection-badge ${connectionState}`}>
                  {connectionState === 'connected' ? <Wifi size={12} /> : <WifiOff size={12} />}
                  {connectionState === 'connected' ? 'Verbonden' : 'Niet verbonden'}
                </div>
              </div>

              <div className="settings-info-row">
                <span className="settings-info-label">Versie</span>
                <div className="settings-version-row">
                  <span className="settings-version-number">v{config.version}</span>
                  {updateAvailable ? (
                    <button
                      className="btn-sm btn-update"
                      onClick={handleInstallUpdate}
                      disabled={updateInstalling}
                    >
                      {updateInstalling ? (
                        <>
                          <Loader2 size={12} className="spin" />
                          Installeren...
                        </>
                      ) : (
                        <>
                          <Download size={12} />
                          Update naar {updateAvailable.latest_version}
                        </>
                      )}
                    </button>
                  ) : updateCheckResult === 'up-to-date' ? (
                    <span className="settings-uptodate-badge">
                      <CheckCircle size={12} />
                      Up-to-date
                    </span>
                  ) : (
                    <button
                      className="btn-sm btn-secondary"
                      onClick={handleCheckForUpdates}
                      disabled={updateChecking}
                    >
                      {updateChecking ? (
                        <Loader2 size={12} className="spin" />
                      ) : (
                        <RefreshCw size={12} />
                      )}
                      Controleren
                    </button>
                  )}
                </div>
              </div>
            </div>

            <div className="modal-footer">
              <button className="btn-sm btn-danger-outline" onClick={onReset}>
                Reset App
              </button>
              <div className="modal-footer-right">
                <button className="btn-secondary" onClick={() => setShowSettings(false)}>
                  Annuleren
                </button>
                <button className="btn-primary" onClick={handleSaveSettings}>
                  Opslaan
                </button>
              </div>
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

      {showDiscovery && (
        <DiscoveryModal
          existingPrinterIds={new Set(config.printers.map((p) => p.id))}
          onClose={() => setShowDiscovery(false)}
          onAdd={handleAddSelectedPrinters}
        />
      )}
    </div>
  )
}
