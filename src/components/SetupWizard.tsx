import { useState } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { Check } from 'lucide-react'
import ConnectStep from './wizard/ConnectStep'
import SuccessStep from './wizard/SuccessStep'
import './SetupWizard.css'

type WizardStep = 'connect' | 'success'

interface SetupWizardProps {
  onComplete: () => void
}

export interface DiscoveredPrinter {
  id: string
  name: string
  connection_type: string
  address: string
  vendor: string
  capabilities?: any
}

export default function SetupWizard({ onComplete }: SetupWizardProps) {
  const [currentStep, setCurrentStep] = useState<WizardStep>('connect')
  const [restaurantId, setRestaurantId] = useState<string>('')
  const [discoveredPrinters, setDiscoveredPrinters] = useState<DiscoveredPrinter[]>([])

  const handleConnectComplete = async (token: string, resId: string) => {
    setRestaurantId(resId)

    try {
      // Save auth to config
      const config = await invoke<any>('get_config')
      config.auth_token = token // printer_service_secret for pairing verification
      config.restaurant_id = resId // restaurant_code like "W434N" — resolved to UUID by backend
      await invoke('save_config', { config })

      // Auto-discover printers
      const printers = await invoke<DiscoveredPrinter[]>('discover_printers')
      setDiscoveredPrinters(printers)

      // Save printers to config
      config.printers = printers.map((p) => ({
        id: p.id,
        name: p.name,
        connection_type: p.connection_type.toLowerCase(), // Rust enum expects lowercase
        address: p.address,
        protocol: 'escpos',
        station: null, // Option<String> in Rust - null instead of empty string
        is_primary: false,
        capabilities: {
          // Always use default capabilities to ensure all required fields exist
          cutter: true,
          drawer: false,
          qrcode: true,
          max_width: 48,
        },
      }))

      await invoke('save_config', { config })

      // Move to success step
      setCurrentStep('success')
    } catch (error) {
      console.error('Setup failed:', error)
      throw error
    }
  }

  return (
    <div className="modern-wizard">
      {/* Progress indicator */}
      <div className="wizard-progress">
        <div className={`progress-dot ${currentStep === 'connect' ? 'active' : 'completed'}`}>
          {currentStep === 'success' ? <Check size={14} /> : '1'}
        </div>
        <div className="progress-line"></div>
        <div className={`progress-dot ${currentStep === 'success' ? 'active' : ''}`}>2</div>
      </div>

      {/* Content */}
      <div className="wizard-container">
        {currentStep === 'connect' && <ConnectStep onComplete={handleConnectComplete} />}
        {currentStep === 'success' && (
          <SuccessStep
            restaurantId={restaurantId}
            printerCount={discoveredPrinters.length}
            onFinish={onComplete}
          />
        )}
      </div>
    </div>
  )
}
