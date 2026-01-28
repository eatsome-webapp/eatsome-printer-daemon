import { useState } from 'react'

interface AuthenticationStepProps {
  onComplete: (token: string, restaurantId: string) => void
}

export default function AuthenticationStep({ onComplete }: AuthenticationStepProps) {
  const [token, setToken] = useState('')
  const [restaurantId, setRestaurantId] = useState('')
  const [error, setError] = useState('')
  const [isValidating, setIsValidating] = useState(false)

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault()
    setError('')

    if (!token.trim()) {
      setError('Authentication token is required')
      return
    }

    if (!restaurantId.trim()) {
      setError('Restaurant ID is required')
      return
    }

    setIsValidating(true)

    try {
      // Validate token format (basic JWT structure check)
      const parts = token.split('.')
      if (parts.length !== 3) {
        throw new Error('Invalid token format. Expected JWT token.')
      }

      // TODO: In production, validate token with Supabase
      // For now, just accept any properly formatted JWT

      onComplete(token, restaurantId)
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Invalid authentication token')
      setIsValidating(false)
    }
  }

  return (
    <div className="wizard-step auth-step">
      <h2>Restaurant Authentication</h2>

      <p className="step-description">
        Enter your restaurant authentication details from the POS system.
        You can find these in the POS admin dashboard under Settings → Printer Service.
      </p>

      <form onSubmit={handleSubmit} className="auth-form">
        <div className="form-group">
          <label htmlFor="restaurantId">
            Restaurant ID
            <span className="required">*</span>
          </label>
          <input
            id="restaurantId"
            type="text"
            value={restaurantId}
            onChange={(e) => setRestaurantId(e.target.value)}
            placeholder="rest_abc123"
            className="form-input"
            disabled={isValidating}
          />
          <small className="form-hint">
            Your unique restaurant identifier (e.g., rest_abc123)
          </small>
        </div>

        <div className="form-group">
          <label htmlFor="authToken">
            Authentication Token
            <span className="required">*</span>
          </label>
          <textarea
            id="authToken"
            value={token}
            onChange={(e) => setToken(e.target.value)}
            placeholder="Paste your JWT authentication token here..."
            className="form-input token-input"
            rows={4}
            disabled={isValidating}
          />
          <small className="form-hint">
            JWT token from POS admin dashboard
          </small>
        </div>

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
            type="submit"
            className="btn-primary"
            disabled={isValidating}
          >
            {isValidating ? 'Validating...' : 'Continue'}
          </button>
        </div>
      </form>

      <div className="auth-help">
        <h3>QR Code Scanner (Coming Soon)</h3>
        <p>
          In the next version, you'll be able to scan a QR code from the POS dashboard
          to automatically configure authentication.
        </p>
      </div>
    </div>
  )
}
