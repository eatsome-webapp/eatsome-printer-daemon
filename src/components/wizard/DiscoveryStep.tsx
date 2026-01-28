import { useState, useEffect } from 'react'
import { invoke } from '@tauri-apps/api/core'
import type { DiscoveredPrinter } from '../SetupWizard'

interface DiscoveryStepProps {
  onComplete: (printers: DiscoveredPrinter[]) => void
}

export default function DiscoveryStep({ onComplete }: DiscoveryStepProps) {
  const [isScanning, setIsScanning] = useState(false)
  const [printers, setPrinters] = useState<DiscoveredPrinter[]>([])
  const [error, setError] = useState('')

  const handleScan = async () => {
    setIsScanning(true)
    setError('')
    setPrinters([])

    try {
      const discovered = await invoke<DiscoveredPrinter[]>('discover_printers')
      setPrinters(discovered)

      if (discovered.length === 0) {
        setError('No printers found. Please ensure printers are powered on and connected.')
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to discover printers')
      console.error('Discovery error:', err)
    } finally {
      setIsScanning(false)
    }
  }

  // Auto-start scan on mount
  useEffect(() => {
    handleScan()
  }, [])

  const handleContinue = () => {
    if (printers.length === 0) {
      setError('Please discover at least one printer before continuing')
      return
    }
    onComplete(printers)
  }

  const getConnectionIcon = (type: string) => {
    switch (type) {
      case 'usb':
        return (
          <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
            <path d="M12 2v20" />
            <path d="M8 6h8" />
            <path d="M8 18h8" />
          </svg>
        )
      case 'network':
        return (
          <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
            <rect x="2" y="2" width="20" height="8" rx="2" ry="2" />
            <rect x="2" y="14" width="20" height="8" rx="2" ry="2" />
            <line x1="6" y1="6" x2="6.01" y2="6" />
            <line x1="6" y1="18" x2="6.01" y2="18" />
          </svg>
        )
      case 'bluetooth':
        return (
          <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
            <polyline points="6.5 6.5 17.5 17.5 12 23 12 1 17.5 6.5 6.5 17.5" />
          </svg>
        )
      default:
        return null
    }
  }

  return (
    <div className="wizard-step discovery-step">
      <h2>Discover Printers</h2>

      <p className="step-description">
        Scanning for USB, Network, and Bluetooth printers...
        This may take up to 30 seconds.
      </p>

      {isScanning && (
        <div className="scanning-indicator">
          <div className="spinner"></div>
          <p>Scanning for printers...</p>
        </div>
      )}

      {!isScanning && printers.length > 0 && (
        <div className="printers-list">
          <h3>Found {printers.length} printer{printers.length !== 1 ? 's' : ''}</h3>

          <div className="printer-cards">
            {printers.map((printer) => (
              <div key={printer.id} className="printer-card">
                <div className="printer-icon">
                  {getConnectionIcon(printer.connection_type)}
                </div>

                <div className="printer-info">
                  <h4>{printer.name}</h4>
                  <p className="printer-vendor">{printer.vendor}</p>
                  <p className="printer-connection">
                    <span className={`connection-badge ${printer.connection_type}`}>
                      {printer.connection_type.toUpperCase()}
                    </span>
                    <span className="printer-address">{printer.address}</span>
                  </p>
                </div>
              </div>
            ))}
          </div>
        </div>
      )}

      {!isScanning && printers.length === 0 && !error && (
        <div className="no-printers">
          <svg width="64" height="64" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
            <rect x="2" y="3" width="20" height="14" rx="2" />
            <path d="M8 21h8" />
            <path d="M12 17v4" />
            <line x1="6" y1="11" x2="18" y2="11" />
            <line x1="8" y1="7" x2="8" y2="7.01" />
          </svg>
          <p>No printers found</p>
        </div>
      )}

      {error && (
        <div className="error-message">
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
            <circle cx="12" cy="12" r="10" />
            <line x1="12" y1="8" x2="12" y2="12" />
            <line x1="12" y1="16" x2="12.01" y2="16" />
          </svg>
          {error}
        </div>
      )}

      <div className="form-actions">
        <button
          type="button"
          className="btn-secondary"
          onClick={handleScan}
          disabled={isScanning}
        >
          {isScanning ? 'Scanning...' : 'Scan Again'}
        </button>

        <button
          type="button"
          className="btn-primary"
          onClick={handleContinue}
          disabled={isScanning || printers.length === 0}
        >
          Continue
        </button>
      </div>
    </div>
  )
}
