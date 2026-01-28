import { useState } from 'react'
import type { DiscoveredPrinter, PrinterAssignment } from '../SetupWizard'

interface AssignmentStepProps {
  printers: DiscoveredPrinter[]
  onComplete: (assignments: PrinterAssignment[]) => void
}

type Station = 'bar' | 'grill' | 'kitchen' | 'dessert' | 'unassigned'

const STATIONS: { id: Station; name: string; color: string }[] = [
  { id: 'bar', name: 'Bar', color: '#3b82f6' },
  { id: 'grill', name: 'Grill Station', color: '#ef4444' },
  { id: 'kitchen', name: 'Main Kitchen', color: '#10b981' },
  { id: 'dessert', name: 'Dessert Station', color: '#f59e0b' },
]

export default function AssignmentStep({ printers, onComplete }: AssignmentStepProps) {
  const [assignments, setAssignments] = useState<Map<string, Station>>(
    new Map(printers.map(p => [p.id, 'unassigned']))
  )
  const [primaryPrinters, setPrimaryPrinters] = useState<Set<string>>(new Set())
  const [error, setError] = useState('')

  const handleAssignStation = (printerId: string, station: Station) => {
    setAssignments(new Map(assignments).set(printerId, station))
    setError('')
  }

  const handleTogglePrimary = (printerId: string) => {
    const newPrimary = new Set(primaryPrinters)
    if (newPrimary.has(printerId)) {
      newPrimary.delete(printerId)
    } else {
      newPrimary.add(printerId)
    }
    setPrimaryPrinters(newPrimary)
  }

  const handleContinue = () => {
    // Validate at least one printer is assigned
    const hasAssignments = Array.from(assignments.values()).some(
      station => station !== 'unassigned'
    )

    if (!hasAssignments) {
      setError('Please assign at least one printer to a station')
      return
    }

    // Build assignments array
    const result: PrinterAssignment[] = []

    for (const printer of printers) {
      const station = assignments.get(printer.id)
      if (station && station !== 'unassigned') {
        result.push({
          printer,
          station,
          isPrimary: primaryPrinters.has(printer.id),
        })
      }
    }

    onComplete(result)
  }

  const getPrintersByStation = (station: Station) => {
    return printers.filter(p => assignments.get(p.id) === station)
  }

  return (
    <div className="wizard-step assignment-step">
      <h2>Assign Printers to Stations</h2>

      <p className="step-description">
        Assign each printer to a kitchen station. You can mark one printer per station as primary.
      </p>

      <div className="assignment-grid">
        {STATIONS.map(station => {
          const stationPrinters = getPrintersByStation(station.id)

          return (
            <div key={station.id} className="station-column">
              <div className="station-header" style={{ borderColor: station.color }}>
                <h3>{station.name}</h3>
                <span className="printer-count">
                  {stationPrinters.length} printer{stationPrinters.length !== 1 ? 's' : ''}
                </span>
              </div>

              <div className="station-printers">
                {stationPrinters.map(printer => (
                  <div key={printer.id} className="assigned-printer">
                    <div className="printer-name">
                      <span>{printer.name}</span>
                      <small>{printer.connection_type}</small>
                    </div>

                    <button
                      type="button"
                      className={`btn-primary-toggle ${primaryPrinters.has(printer.id) ? 'active' : ''}`}
                      onClick={() => handleTogglePrimary(printer.id)}
                      title={primaryPrinters.has(printer.id) ? 'Primary printer' : 'Set as primary'}
                    >
                      ★
                    </button>

                    <button
                      type="button"
                      className="btn-remove"
                      onClick={() => handleAssignStation(printer.id, 'unassigned')}
                      title="Remove from station"
                    >
                      ✕
                    </button>
                  </div>
                ))}
              </div>
            </div>
          )
        })}
      </div>

      {getPrintersByStation('unassigned').length > 0 && (
        <div className="unassigned-printers">
          <h3>Unassigned Printers</h3>
          <p>Click a printer to assign it to a station</p>

          <div className="printer-buttons">
            {getPrintersByStation('unassigned').map(printer => (
              <div key={printer.id} className="unassigned-printer">
                <span className="printer-label">
                  {printer.name}
                  <small>({printer.connection_type})</small>
                </span>

                <div className="station-buttons">
                  {STATIONS.map(station => (
                    <button
                      key={station.id}
                      type="button"
                      className="btn-station"
                      style={{ backgroundColor: station.color }}
                      onClick={() => handleAssignStation(printer.id, station.id)}
                    >
                      {station.name}
                    </button>
                  ))}
                </div>
              </div>
            ))}
          </div>
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

      <div className="assignment-tips">
        <h4>Tips:</h4>
        <ul>
          <li>★ = Primary printer (prints by default if multiple printers assigned)</li>
          <li>Backup printers will be used if primary printer fails</li>
          <li>You can assign multiple printers to the same station</li>
        </ul>
      </div>

      <div className="form-actions">
        <button
          type="button"
          className="btn-primary"
          onClick={handleContinue}
        >
          Continue
        </button>
      </div>
    </div>
  )
}
