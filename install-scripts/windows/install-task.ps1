# Windows Task Scheduler Installation Script
# Installs and starts the Eatsome Printer Service as a scheduled task

# Requires: PowerShell 5.1+ (Windows 10/11)
# Run as: User (NOT administrator)

param(
    [switch]$Uninstall,
    [switch]$Force
)

$TaskName = "Eatsome Printer Service"
$TaskPath = "\Eatsome\"
$TaskXml = ".\EatsomePrinterService.xml"

function Test-TaskExists {
    try {
        $task = Get-ScheduledTask -TaskName $TaskName -TaskPath $TaskPath -ErrorAction Stop
        return $true
    } catch {
        return $false
    }
}

function Install-Task {
    Write-Host "🚀 Installing Eatsome Printer Service..." -ForegroundColor Cyan

    # Check if XML exists
    if (-not (Test-Path $TaskXml)) {
        Write-Host "❌ Error: $TaskXml not found!" -ForegroundColor Red
        Write-Host "   Run this script from the install-scripts/windows directory" -ForegroundColor Yellow
        exit 1
    }

    # Check if task already exists
    if (Test-TaskExists) {
        if ($Force) {
            Write-Host "⚠️  Task already exists - removing..." -ForegroundColor Yellow
            Unregister-ScheduledTask -TaskName $TaskName -TaskPath $TaskPath -Confirm:$false
        } else {
            Write-Host "❌ Error: Task already exists!" -ForegroundColor Red
            Write-Host "   Use -Force to reinstall" -ForegroundColor Yellow
            exit 1
        }
    }

    # Create task folder if it doesn't exist
    try {
        $null = Get-ScheduledTask -TaskPath $TaskPath -ErrorAction Stop
    } catch {
        Write-Host "📁 Creating task folder: $TaskPath" -ForegroundColor Gray
        # Folder creation happens automatically when task is registered
    }

    # Register task
    Write-Host "📄 Registering scheduled task..." -ForegroundColor Gray
    try {
        Register-ScheduledTask -Xml (Get-Content $TaskXml | Out-String) `
            -TaskName $TaskName `
            -TaskPath $TaskPath `
            -Force:$Force
        Write-Host "✅ Task registered successfully!" -ForegroundColor Green
    } catch {
        Write-Host "❌ Failed to register task: $_" -ForegroundColor Red
        exit 1
    }

    # Start task immediately
    Write-Host "▶️  Starting service..." -ForegroundColor Gray
    try {
        Start-ScheduledTask -TaskName $TaskName -TaskPath $TaskPath
        Start-Sleep -Seconds 2

        # Verify running
        $task = Get-ScheduledTask -TaskName $TaskName -TaskPath $TaskPath
        if ($task.State -eq "Running") {
            Write-Host "✅ Eatsome Printer Service is now running!" -ForegroundColor Green
            Write-Host "📊 Service will start automatically on login." -ForegroundColor Cyan
        } else {
            Write-Host "⚠️  Service registered but not running (State: $($task.State))" -ForegroundColor Yellow
            Write-Host "   Check Task Scheduler for details" -ForegroundColor Gray
        }
    } catch {
        Write-Host "❌ Failed to start task: $_" -ForegroundColor Red
        Write-Host "   Task is registered but not running" -ForegroundColor Yellow
    }

    Write-Host ""
    Write-Host "📝 Management commands:" -ForegroundColor Cyan
    Write-Host "  View status:  Get-ScheduledTask -TaskName '$TaskName' -TaskPath '$TaskPath'" -ForegroundColor Gray
    Write-Host "  Start:        Start-ScheduledTask -TaskName '$TaskName' -TaskPath '$TaskPath'" -ForegroundColor Gray
    Write-Host "  Stop:         Stop-ScheduledTask -TaskName '$TaskName' -TaskPath '$TaskPath'" -ForegroundColor Gray
    Write-Host "  Uninstall:    .\install-task.ps1 -Uninstall" -ForegroundColor Gray
}

function Uninstall-Task {
    Write-Host "🗑️  Uninstalling Eatsome Printer Service..." -ForegroundColor Cyan

    if (-not (Test-TaskExists)) {
        Write-Host "⚠️  Task not found - nothing to uninstall" -ForegroundColor Yellow
        exit 0
    }

    # Stop if running
    try {
        Stop-ScheduledTask -TaskName $TaskName -TaskPath $TaskPath -ErrorAction SilentlyContinue
        Write-Host "⏹️  Service stopped" -ForegroundColor Gray
    } catch {
        # Ignore errors if already stopped
    }

    # Unregister task
    try {
        Unregister-ScheduledTask -TaskName $TaskName -TaskPath $TaskPath -Confirm:$false
        Write-Host "✅ Task uninstalled successfully!" -ForegroundColor Green
    } catch {
        Write-Host "❌ Failed to uninstall task: $_" -ForegroundColor Red
        exit 1
    }
}

# Main
if ($Uninstall) {
    Uninstall-Task
} else {
    Install-Task
}
