<#
.SYNOPSIS
    Binbot Render Automated Check & Wake-up Script (PowerShell)

.DESCRIPTION
    Checks if the Render backend is sleeping. If already active, it immediately executes
    the specified command without delay. If sleeping, it pings the endpoint to trigger
    Render's container boot, waits until ready, and then executes the command.

.EXAMPLE
    .\scripts\wakeup.ps1 -BackendUrl "https://binbot.onrender.com"
    .\scripts\wakeup.ps1 -BackendUrl "https://binbot.onrender.com" -Command "cargo test"
#>

param (
    [Parameter(Position=0)]
    [string]$BackendUrl = "https://binbot.onrender.com",

    [Parameter(Position=1, ValueFromRemainingArguments=$true)]
    [string[]]$Command
)

$ErrorActionPreference = "Stop"
$HealthUrl = "$($BackendUrl.TrimEnd('/'))/healthz"

Write-Host "[CHECK] Checking Render backend status at: $HealthUrl..." -ForegroundColor Cyan

$isAwake = $false

# 1. Initial fast check (2 second timeout)
try {
    $request = [System.Net.HttpWebRequest]::Create($HealthUrl)
    $request.Timeout = 2000
    $request.Method = "GET"
    $response = $request.GetResponse()
    if ($response.StatusCode -eq [System.Net.HttpStatusCode]::OK) {
        $isAwake = $true
    }
    $response.Close()
} catch {
    $isAwake = $false
}

if ($isAwake) {
    Write-Host "[OK] Backend is active and running! Proceeding immediately." -ForegroundColor Green
} else {
    Write-Host "[SLEEPING] Backend is sleeping or starting up." -ForegroundColor Yellow
    Write-Host "[WAKEUP] Sending wake-up ping to trigger Render instance boot..." -ForegroundColor Cyan

    # Trigger wake-up call asynchronously
    [System.Net.HttpWebRequest]::Create($HealthUrl).BeginGetResponse($null, $null) | Out-Null

    $maxRetries = 30
    $retryCount = 0

    while ($retryCount -lt $maxRetries) {
        Start-Sleep -Seconds 2
        $retryCount++

        try {
            $req = [System.Net.HttpWebRequest]::Create($HealthUrl)
            $req.Timeout = 3000
            $req.Method = "GET"
            $res = $req.GetResponse()
            if ($res.StatusCode -eq [System.Net.HttpStatusCode]::OK) {
                $isAwake = $true
                $res.Close()
                Write-Host "[READY] Render backend successfully woke up in ~$($retryCount * 2)s!" -ForegroundColor Green
                break
            }
            $res.Close()
        } catch {
            Write-Host "   [WAIT] Waiting for Render instance... ($retryCount/$maxRetries)" -ForegroundColor Gray
        }
    }

    if (-not $isAwake) {
        Write-Host "[TIMEOUT] Render backend did not wake up within 60 seconds." -ForegroundColor Red
        exit 1
    }
}

# 2. Execute target command if provided
if ($Command -and $Command.Count -gt 0) {
    $cmdString = $Command -join " "
    Write-Host "[RUN] Executing target command: $cmdString" -ForegroundColor Magenta
    Invoke-Expression $cmdString
}
