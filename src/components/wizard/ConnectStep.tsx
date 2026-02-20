import { useState } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { Store, AlertTriangle, Zap } from 'lucide-react'

interface ConnectStepProps {
  onComplete: (token: string, restaurantId: string) => Promise<void>
}

export default function ConnectStep({ onComplete }: ConnectStepProps) {
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [pairingCode, setPairingCode] = useState('')

  // Format display: "XX XXXXXX X" (2-6-1 grouping)
  const formatCode = (raw: string): string => {
    const digits = raw.replace(/\D/g, '').slice(0, 9)
    if (digits.length <= 2) return digits
    if (digits.length <= 8) return `${digits.slice(0, 2)} ${digits.slice(2)}`
    return `${digits.slice(0, 2)} ${digits.slice(2, 8)} ${digits.slice(8)}`
  }

  const handleInput = (e: React.ChangeEvent<HTMLInputElement>) => {
    const raw = e.target.value.replace(/\D/g, '').slice(0, 9)
    setPairingCode(raw)
    setError(null)
  }

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault()
    setError(null)

    const code = pairingCode.trim()
    if (code.length !== 9) {
      setError('Vul alle 9 cijfers in')
      return
    }

    setLoading(true)
    try {
      const result = await invoke<{
        token: string
        restaurantId: string
        restaurantCode: string
      }>('claim_pairing_code', { code })

      await onComplete(result.token, result.restaurantId)
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
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
      <p className="connect-subtitle">Voer de koppelingscode in van je restaurant dashboard</p>

      <form onSubmit={handleSubmit} className="connect-form">
        <div className="form-field">
          <input
            type="text"
            inputMode="numeric"
            placeholder="00 000000 0"
            value={formatCode(pairingCode)}
            onChange={handleInput}
            className="modern-input pairing-input"
            disabled={loading}
            autoFocus
          />
        </div>

        {error && (
          <div className="error-box">
            <AlertTriangle size={16} className="error-icon" />
            {error}
          </div>
        )}

        <button
          type="submit"
          className="connect-button"
          disabled={loading || pairingCode.length !== 9}
        >
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

        <p className="connect-hint">Dashboard &rarr; Devices &rarr; &quot;Genereer Code&quot;</p>
      </form>
    </div>
  )
}
