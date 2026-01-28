import { useState } from 'react'
import { invoke } from '@tauri-apps/api/core'
import WelcomeStep from './wizard/WelcomeStep'
import AuthenticationStep from './wizard/AuthenticationStep'
import DiscoveryStep from './wizard/DiscoveryStep'
import AssignmentStep from './wizard/AssignmentStep'
import CompleteStep from './wizard/CompleteStep'
import './SetupWizard.css'

type WizardStep = 'welcome' | 'auth' | 'discovery' | 'assignment' | 'complete'

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

export interface PrinterAssignment {
  printer: DiscoveredPrinter
  station: string
  isPrimary: boolean
}

export default function SetupWizard({ onComplete }: SetupWizardProps) {
  const [currentStep, setCurrentStep] = useState<WizardStep>('welcome')
  const [_authToken, setAuthToken] = useState<string>('')
  const [restaurantId, setRestaurantId] = useState<string>('')
  const [discoveredPrinters, setDiscoveredPrinters] = useState<DiscoveredPrinter[]>([])
  const [assignments, setAssignments] = useState<PrinterAssignment[]>([])

  const handleWelcomeNext = () => {
    setCurrentStep('auth')
  }

  const handleAuthComplete = async (token: string, resId: string) => {
    setAuthToken(token)
    setRestaurantId(resId)

    // Save authentication to config
    try {
      const config = await invoke<any>('get_config')
      config.auth_token = token
      config.restaurant_id = resId
      await invoke('save_config', { config })
      setCurrentStep('discovery')
    } catch (error) {
      console.error('Failed to save auth config:', error)
      alert(`Failed to save configuration: ${error}`)
    }
  }

  const handleDiscoveryComplete = (printers: DiscoveredPrinter[]) => {
    setDiscoveredPrinters(printers)
    setCurrentStep('assignment')
  }

  const handleAssignmentComplete = async (assignments: PrinterAssignment[]) => {
    setAssignments(assignments)

    // Save printer assignments to config
    try {
      const config = await invoke<any>('get_config')

      // Convert assignments to printer configs
      config.printers = assignments.map((a) => ({
        id: a.printer.id,
        name: a.printer.name,
        connection_type: a.printer.connection_type,
        address: a.printer.address,
        protocol: 'escpos',
        station: a.station,
        is_primary: a.isPrimary,
        capabilities: a.printer.capabilities || {
          cutter: true,
          drawer: false,
          qrcode: true,
          max_width: 48,
        },
      }))

      await invoke('save_config', { config })
      setCurrentStep('complete')
    } catch (error) {
      console.error('Failed to save printer assignments:', error)
      alert(`Failed to save printer configuration: ${error}`)
    }
  }

  const handleComplete = () => {
    onComplete()
  }

  return (
    <div className="setup-wizard">
      <div className="wizard-header">
        <h1>Eatsome Printer Service Setup</h1>
        <div className="step-indicator">
          <div className={`step ${currentStep === 'welcome' ? 'active' : 'completed'}`}>1</div>
          <div
            className={`step ${currentStep === 'auth' ? 'active' : currentStep === 'discovery' || currentStep === 'assignment' || currentStep === 'complete' ? 'completed' : ''}`}
          >
            2
          </div>
          <div
            className={`step ${currentStep === 'discovery' ? 'active' : currentStep === 'assignment' || currentStep === 'complete' ? 'completed' : ''}`}
          >
            3
          </div>
          <div
            className={`step ${currentStep === 'assignment' ? 'active' : currentStep === 'complete' ? 'completed' : ''}`}
          >
            4
          </div>
          <div className={`step ${currentStep === 'complete' ? 'active' : ''}`}>5</div>
        </div>
      </div>

      <div className="wizard-content">
        {currentStep === 'welcome' && <WelcomeStep onNext={handleWelcomeNext} />}
        {currentStep === 'auth' && <AuthenticationStep onComplete={handleAuthComplete} />}
        {currentStep === 'discovery' && <DiscoveryStep onComplete={handleDiscoveryComplete} />}
        {currentStep === 'assignment' && (
          <AssignmentStep printers={discoveredPrinters} onComplete={handleAssignmentComplete} />
        )}
        {currentStep === 'complete' && (
          <CompleteStep
            restaurantId={restaurantId}
            printerCount={assignments.length}
            onFinish={handleComplete}
          />
        )}
      </div>
    </div>
  )
}
