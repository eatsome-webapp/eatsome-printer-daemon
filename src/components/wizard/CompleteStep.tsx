interface CompleteStepProps {
  restaurantId: string
  printerCount: number
  onFinish: () => void
}

export default function CompleteStep({ restaurantId, printerCount, onFinish }: CompleteStepProps) {
  return (
    <div className="wizard-step complete-step">
      <div className="success-icon">
        <svg width="80" height="80" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
          <circle cx="12" cy="12" r="10" />
          <path d="M9 12l2 2 4-4" />
        </svg>
      </div>

      <h2>Setup Complete!</h2>

      <p className="success-message">
        Your printer service is now configured and ready to use.
      </p>

      <div className="setup-summary">
        <div className="summary-item">
          <span className="summary-label">Restaurant ID:</span>
          <span className="summary-value">{restaurantId}</span>
        </div>

        <div className="summary-item">
          <span className="summary-label">Printers Configured:</span>
          <span className="summary-value">{printerCount}</span>
        </div>
      </div>

      <div className="next-steps">
        <h3>What happens next?</h3>
        <ul>
          <li>
            <strong>Background Service Started</strong>
            <p>The printer service is now running in the background</p>
          </li>
          <li>
            <strong>Real-time Connection</strong>
            <p>Connected to Supabase Realtime for instant order printing</p>
          </li>
          <li>
            <strong>System Tray Icon</strong>
            <p>Access settings and status from the system tray</p>
          </li>
        </ul>
      </div>

      <div className="tips">
        <h3>Quick Tips:</h3>
        <ul>
          <li>Check the system tray icon to see printer status</li>
          <li>Use "Test Print" from settings to verify each printer</li>
          <li>Failed jobs will automatically retry 3 times</li>
          <li>All jobs are saved locally for offline reliability</li>
        </ul>
      </div>

      <button className="btn-primary btn-large" onClick={onFinish}>
        Start Using Printer Service
      </button>
    </div>
  )
}
