import { useState } from 'react'
import { Store, QrCode, AlertTriangle, Zap, ChevronRight, ArrowLeft } from 'lucide-react'

interface ConnectStepProps {
  onComplete: (token: string, restaurantId: string) => Promise<void>
}

export default function ConnectStep({ onComplete }: ConnectStepProps) {
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [manualMode, setManualMode] = useState(false)
  const [restaurantId, setRestaurantId] = useState('')
  const [token, setToken] = useState('')

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault()
    setError(null)

    if (!restaurantId.trim() || !token.trim()) {
      setError('Vul alle velden in')
      return
    }

    setLoading(true)
    try {
      await onComplete(token.trim(), restaurantId.trim())
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Verbinden mislukt')
      setLoading(false)
    }
  }

  return (
    <div className="connect-step">
      {/* Hero icon */}
      <div className="hero-icon">
        <Store size={28} className="icon-restaurant" />
      </div>

      {/* Title */}
      <h1 className="connect-title">Verbind je Restaurant</h1>
      <p className="connect-subtitle">Koppel de printer service aan je restaurant dashboard</p>

      {!manualMode ? (
        <div className="qr-mode">
          {/* QR Code placeholder */}
          <div className="qr-placeholder">
            <div className="qr-icon">
              <QrCode size={48} />
            </div>
            <p className="qr-text">Scan QR code in restaurant dashboard</p>
            <p className="qr-hint">Dashboard &rarr; Devices &rarr; QR Code</p>
          </div>

          {/* Manual mode toggle */}
          <button type="button" className="manual-toggle" onClick={() => setManualMode(true)}>
            Of handmatig koppelen <ChevronRight size={14} />
          </button>
        </div>
      ) : (
        <form onSubmit={handleSubmit} className="manual-form">
          <div className="form-field">
            <input
              type="text"
              placeholder="e.g. W434N"
              value={restaurantId}
              onChange={(e) => setRestaurantId(e.target.value)}
              className="modern-input"
              disabled={loading}
            />
          </div>

          <div className="form-field">
            <input
              type="password"
              placeholder="Access Token"
              value={token}
              onChange={(e) => setToken(e.target.value)}
              className="modern-input"
              disabled={loading}
            />
          </div>

          {error && (
            <div className="error-box">
              <AlertTriangle size={16} className="error-icon" />
              {error}
            </div>
          )}

          <button type="submit" className="connect-button" disabled={loading}>
            {loading ? (
              <>
                <div className="spinner spinner-connect"></div>
                Verbinden...
              </>
            ) : (
              <>
                <Zap size={16} />
                Verbind Restaurant
              </>
            )}
          </button>

          <button
            type="button"
            className="back-button"
            onClick={() => setManualMode(false)}
            disabled={loading}
          >
            <ArrowLeft size={14} /> Terug naar QR code
          </button>
        </form>
      )}
    </div>
  )
}
