import { Printer, CircleCheck, ClipboardList } from 'lucide-react'

interface SuccessStepProps {
  restaurantId: string
  printerCount: number
  onFinish: () => void
}

export default function SuccessStep({ printerCount, onFinish }: SuccessStepProps) {
  return (
    <div className="success-step">
      {/* Success animation — keep animated SVG (Lucide can't animate) */}
      <div className="success-animation">
        <div className="success-checkmark">
          <svg viewBox="0 0 52 52">
            <circle className="checkmark-circle" cx="26" cy="26" r="25" fill="none" />
            <path className="checkmark-check" fill="none" d="M14.1 27.2l7.1 7.2 16.7-16.8" />
          </svg>
        </div>
      </div>

      {/* Success message */}
      <h1 className="success-title">Alles Klaar!</h1>
      <p className="success-subtitle">Je printer service is succesvol gekoppeld</p>

      {/* Stats */}
      <div className="success-stats">
        <div className="stat-card">
          <div className="stat-icon">
            <Printer size={24} />
          </div>
          <div className="stat-value">{printerCount}</div>
          <div className="stat-label">
            {printerCount === 1 ? 'Printer gevonden' : 'Printers gevonden'}
          </div>
        </div>

        <div className="stat-card">
          <div className="stat-icon status-online">
            <CircleCheck size={24} />
          </div>
          <div className="stat-value">Online</div>
          <div className="stat-label">Status</div>
        </div>
      </div>

      {/* Next steps */}
      <div className="next-steps-box">
        <h3>
          <ClipboardList size={14} /> Volgende Stappen
        </h3>
        <ul>
          <li>Open je restaurant dashboard</li>
          <li>Ga naar Devices &rarr; Printers</li>
          <li>Wijs printers toe aan stations (bar, keuken, etc.)</li>
        </ul>
      </div>

      {/* Finish button */}
      <button className="finish-button" onClick={onFinish}>
        <CircleCheck size={16} />
        Start Printer Service
      </button>
    </div>
  )
}
