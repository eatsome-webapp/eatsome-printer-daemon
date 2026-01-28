interface WelcomeStepProps {
  onNext: () => void
}

export default function WelcomeStep({ onNext }: WelcomeStepProps) {
  return (
    <div className="wizard-step welcome-step">
      <div className="welcome-content">
        <div className="welcome-icon">
          <svg
            width="120"
            height="120"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="2"
          >
            <rect x="2" y="3" width="20" height="14" rx="2" />
            <path d="M8 21h8" />
            <path d="M12 17v4" />
            <path d="M6 8h12" />
            <path d="M6 12h12" />
            <path d="M6 16h6" />
          </svg>
        </div>

        <h2>Welcome to Eatsome Printer Service</h2>

        <p className="welcome-description">
          This wizard will guide you through setting up your thermal printers for your restaurant.
          The setup process takes approximately 5 minutes.
        </p>

        <div className="welcome-features">
          <div className="feature">
            <h3>✓ Automatic Discovery</h3>
            <p>Finds USB, Network, and Bluetooth printers automatically</p>
          </div>
          <div className="feature">
            <h3>✓ Easy Configuration</h3>
            <p>Drag-and-drop station assignment for Bar, Grill, Kitchen</p>
          </div>
          <div className="feature">
            <h3>✓ Real-time Printing</h3>
            <p>Instant order printing via Supabase Realtime</p>
          </div>
        </div>

        <div className="welcome-requirements">
          <h3>Before you begin:</h3>
          <ul>
            <li>Ensure printers are powered on and connected</li>
            <li>Have your restaurant authentication token ready (from POS)</li>
            <li>Network printers should be on the same subnet</li>
          </ul>
        </div>

        <button className="btn-primary" onClick={onNext}>
          Get Started
        </button>
      </div>
    </div>
  )
}
