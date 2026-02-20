import { useState, useEffect, useRef } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { Printer, X, Loader2, Search, Usb, Wifi, Bluetooth, Check, AlertCircle } from 'lucide-react'
import './DiscoveryModal.css'

export interface DiscoveredPrinter {
  id: string
  name: string
  connection_type: string
  address: string
  vendor: string
  capabilities: Record<string, unknown> | null
  protocol: string
}

interface DiscoveryModalProps {
  existingPrinterIds: Set<string>
  onClose: () => void
  onAdd: (printers: DiscoveredPrinter[]) => void
}

type Phase = 'scanning' | 'results' | 'empty'

export default function DiscoveryModal({
  existingPrinterIds,
  onClose,
  onAdd,
}: DiscoveryModalProps) {
  const [phase, setPhase] = useState<Phase>('scanning')
  const [discoveredPrinters, setDiscoveredPrinters] = useState<DiscoveredPrinter[]>([])
  const [selectedIds, setSelectedIds] = useState<Set<string>>(new Set())
  const [scanError, setScanError] = useState<string | null>(null)
  const unmountedRef = useRef(false)

  useEffect(() => {
    unmountedRef.current = false
    startScan()
    return () => {
      unmountedRef.current = true
    }
  }, [])

  async function startScan() {
    setPhase('scanning')
    setScanError(null)
    setSelectedIds(new Set())

    try {
      const printers = await invoke<DiscoveredPrinter[]>('discover_printers', { force: true })
      if (unmountedRef.current) return

      if (printers.length === 0) {
        setPhase('empty')
      } else {
        setDiscoveredPrinters(printers)
        // Auto-select new printers that aren't already configured
        const newIds = new Set(
          printers.filter((p) => !existingPrinterIds.has(p.id)).map((p) => p.id)
        )
        setSelectedIds(newIds)
        setPhase('results')
      }
    } catch (error) {
      if (unmountedRef.current) return
      setScanError(String(error))
      setPhase('empty')
    }
  }

  function toggleSelection(id: string) {
    if (existingPrinterIds.has(id)) return
    setSelectedIds((prev) => {
      const next = new Set(prev)
      if (next.has(id)) {
        next.delete(id)
      } else {
        next.add(id)
      }
      return next
    })
  }

  function selectAll() {
    const selectableIds = discoveredPrinters
      .filter((p) => !existingPrinterIds.has(p.id))
      .map((p) => p.id)
    setSelectedIds(new Set(selectableIds))
  }

  function deselectAll() {
    setSelectedIds(new Set())
  }

  function handleAdd() {
    const selected = discoveredPrinters.filter((p) => selectedIds.has(p.id))
    onAdd(selected)
    onClose()
  }

  const selectableCount = discoveredPrinters.filter((p) => !existingPrinterIds.has(p.id)).length
  const allSelected = selectableCount > 0 && selectedIds.size === selectableCount

  function ConnectionIcon({ type }: { type: string }) {
    switch (type.toLowerCase()) {
      case 'usb':
        return <Usb size={12} />
      case 'network':
        return <Wifi size={12} />
      case 'bluetooth':
      case 'ble':
        return <Bluetooth size={12} />
      default:
        return <Wifi size={12} />
    }
  }

  function connBadgeClass(type: string): string {
    switch (type.toLowerCase()) {
      case 'usb':
        return 'badge-conn-usb'
      case 'network':
        return 'badge-conn-network'
      case 'bluetooth':
      case 'ble':
        return 'badge-conn-ble'
      default:
        return 'badge-conn-network'
    }
  }

  function protocolBadgeClass(protocol: string): string {
    switch (protocol.toLowerCase()) {
      case 'escpos':
        return 'badge-protocol-escpos'
      case 'unsupported':
        return 'badge-protocol-unsupported'
      default:
        return 'badge-protocol-unknown'
    }
  }

  return (
    <div className="modal-overlay" onClick={onClose}>
      <div className="discovery-modal" onClick={(e) => e.stopPropagation()}>
        <div className="modal-header">
          <h2>
            <Search size={18} /> Discover Printers
          </h2>
          <button className="btn-close" onClick={onClose}>
            <X size={16} />
          </button>
        </div>

        {/* Scanning Phase */}
        {phase === 'scanning' && (
          <div className="discovery-scanning">
            <Loader2 size={32} className="spin discovery-spinner" />
            <p className="discovery-scanning-text">Scanning for printers...</p>
            <p className="discovery-scanning-hint">Checking USB, network, and Bluetooth</p>
            <button className="btn-sm btn-secondary" onClick={onClose}>
              Cancel
            </button>
          </div>
        )}

        {/* Results Phase */}
        {phase === 'results' && (
          <>
            <div className="discovery-toolbar">
              <span className="discovery-count">
                {discoveredPrinters.length} printer{discoveredPrinters.length !== 1 ? 's' : ''}{' '}
                found
              </span>
              {selectableCount > 1 && (
                <button className="btn-text" onClick={allSelected ? deselectAll : selectAll}>
                  {allSelected ? 'Deselect all' : 'Select all'}
                </button>
              )}
            </div>
            <div className="discovery-list">
              {discoveredPrinters.map((printer) => {
                const isExisting = existingPrinterIds.has(printer.id)
                const isSelected = selectedIds.has(printer.id)

                return (
                  <div
                    key={printer.id}
                    className={`discovery-row ${isExisting ? 'discovery-row-disabled' : ''} ${isSelected ? 'discovery-row-selected' : ''}`}
                    onClick={() => toggleSelection(printer.id)}
                  >
                    <div
                      className={`discovery-checkbox ${isSelected ? 'checked' : ''} ${isExisting ? 'disabled' : ''}`}
                    >
                      {isSelected && <Check size={12} />}
                    </div>
                    <Printer size={16} className="discovery-row-icon" />
                    <div className="discovery-row-info">
                      <div className="discovery-row-name">
                        {printer.name}
                        {isExisting && <span className="badge-already-added">Already added</span>}
                      </div>
                      <div className="discovery-row-meta">
                        <span className={`badge-conn ${connBadgeClass(printer.connection_type)}`}>
                          <ConnectionIcon type={printer.connection_type} />
                          {printer.connection_type.toUpperCase()}
                        </span>
                        <span className="discovery-row-address">{printer.address}</span>
                        <span className={`badge-protocol ${protocolBadgeClass(printer.protocol)}`}>
                          {printer.protocol}
                        </span>
                      </div>
                    </div>
                  </div>
                )
              })}
            </div>
            <div className="modal-footer">
              <button className="btn-sm btn-secondary" onClick={startScan}>
                <Search size={14} />
                Scan Again
              </button>
              <button
                className="btn-sm btn-primary"
                onClick={handleAdd}
                disabled={selectedIds.size === 0}
              >
                Add Selected ({selectedIds.size})
              </button>
            </div>
          </>
        )}

        {/* Empty Phase */}
        {phase === 'empty' && (
          <div className="discovery-empty">
            {scanError && (
              <div className="discovery-error">
                <AlertCircle size={14} />
                <span>{scanError}</span>
              </div>
            )}
            <div className="discovery-empty-icon">
              <Printer size={32} />
            </div>
            <p className="discovery-empty-title">No printers found</p>
            <div className="discovery-tips">
              <p className="discovery-tips-heading">Troubleshooting:</p>
              <ul>
                <li>Check that your printer is powered on</li>
                <li>Verify the USB cable is securely connected</li>
                <li>Ensure the printer is on the same network</li>
                <li>For Bluetooth, make sure it's paired in system settings</li>
              </ul>
            </div>
            <button className="btn-sm btn-secondary" onClick={startScan}>
              <Search size={14} />
              Scan Again
            </button>
          </div>
        )}
      </div>
    </div>
  )
}
