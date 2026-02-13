#Requires -Version 5.1
<#
.SYNOPSIS
    Cleans up the node-launchpad setup by stopping and removing all antnode services,
    then deleting associated directories and files.
.DESCRIPTION
    This script must be run with elevated (Administrator) privileges. It will:
    1. Stop and remove all antnode Windows services
    2. Remove the Node Launchpad MSIX package if installed
    3. Delete the antctl program data directory
    4. Delete the antnode logs directory
    5. Delete the node-launchpad executable
    6. Delete the autonomi app data directory
#>

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

# Check for elevated privileges
$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
$principal = New-Object Security.Principal.WindowsPrincipal($identity)
if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    Write-Error "This script must be run with elevated (Administrator) privileges. Please run PowerShell as Administrator and try again."
    exit 1
}

# Check if node-launchpad.exe is running
$launchpadProcess = Get-Process -Name "node-launchpad" -ErrorAction SilentlyContinue
if ($launchpadProcess) {
    Write-Error "node-launchpad.exe is currently running. Please close it before running this script."
    exit 1
}

# Paths
$registryPath = "C:\ProgramData\antctl\node_registry.json"
$antctlDir = "C:\ProgramData\antctl"
$antnodeLogsDir = "C:\ProgramData\antnode"
$launchpadExe = "C:\Users\Chris\AppData\Local\Microsoft\WindowsApps\node-launchpad.exe"
$autonomiDir = "C:\Users\Chris\AppData\Roaming\autonomi"

# Read node registry and stop/remove services
if (Test-Path $registryPath) {
    Write-Host "Reading node registry from $registryPath..."
    $registry = Get-Content $registryPath -Raw | ConvertFrom-Json
    $nodes = $registry.nodes

    if ($nodes -and $nodes.Count -gt 0) {
        Write-Host "Found $($nodes.Count) node(s) in registry."

        foreach ($node in $nodes) {
            $serviceName = $node.service_name
            Write-Host ""
            Write-Host "Processing service: $serviceName"

            # Check if the service exists
            $service = Get-Service -Name $serviceName -ErrorAction SilentlyContinue
            if (-not $service) {
                Write-Host "  Service '$serviceName' does not exist. Skipping."
                continue
            }

            # Stop the service if it is running
            if ($service.Status -eq "Running") {
                Write-Host "  Stopping service '$serviceName'..."
                Stop-Service -Name $serviceName -Force -ErrorAction SilentlyContinue

                # Wait for the service to stop (up to 30 seconds)
                $timeout = 30
                $elapsed = 0
                while ($elapsed -lt $timeout) {
                    $service = Get-Service -Name $serviceName -ErrorAction SilentlyContinue
                    if (-not $service -or $service.Status -eq "Stopped") {
                        break
                    }
                    Start-Sleep -Seconds 2
                    $elapsed += 2
                }

                if ($service -and $service.Status -ne "Stopped") {
                    Write-Warning "  Service '$serviceName' did not stop within $timeout seconds."
                }
            }

            # Check if the associated antnode process is still running
            $exePath = $node.antnode_path
            if ($exePath -and (Test-Path $exePath)) {
                $proc = Get-Process | Where-Object {
                    $_.Path -and $_.Path -eq $exePath
                } -ErrorAction SilentlyContinue
                if ($proc) {
                    Write-Host "  Waiting for antnode process to exit..."
                    $proc | ForEach-Object {
                        $_ | Wait-Process -Timeout 15 -ErrorAction SilentlyContinue
                        if (-not $_.HasExited) {
                            Write-Warning "  Forcefully stopping antnode process (PID $($_.Id))..."
                            $_ | Stop-Process -Force -ErrorAction SilentlyContinue
                        }
                    }
                }
            }

            # Delete the service using sc.exe
            Write-Host "  Deleting service '$serviceName'..."
            $scResult = sc.exe delete $serviceName 2>&1
            if ($LASTEXITCODE -eq 0) {
                Write-Host "  Service '$serviceName' deleted successfully."
            } else {
                Write-Warning "  Failed to delete service '$serviceName': $scResult"
            }
        }
    } else {
        Write-Host "No nodes found in registry."
    }
} else {
    Write-Host "Node registry not found at $registryPath. Skipping service cleanup."
}

Write-Host ""

# Remove Node Launchpad MSIX package if installed
$msixPackage = Get-AppxPackage -Name "Autonomi.NodeLaunchpad" -ErrorAction SilentlyContinue
if ($msixPackage) {
    Write-Host "Found Node Launchpad MSIX package (version $($msixPackage.Version)). Removing..."
    $msixPackage | Remove-AppxPackage -ErrorAction Stop
    Write-Host "Node Launchpad MSIX package removed."
} else {
    Write-Host "Node Launchpad MSIX package is not installed. Skipping."
}

Write-Host ""

# Delete directories and files
if (Test-Path $antctlDir) {
    Write-Host "Deleting $antctlDir..."
    Remove-Item -Path $antctlDir -Recurse -Force
    Write-Host "Deleted $antctlDir."
} else {
    Write-Host "$antctlDir does not exist. Skipping."
}

if (Test-Path $antnodeLogsDir) {
    Write-Host "Deleting $antnodeLogsDir..."
    Remove-Item -Path $antnodeLogsDir -Recurse -Force
    Write-Host "Deleted $antnodeLogsDir."
} else {
    Write-Host "$antnodeLogsDir does not exist. Skipping."
}

if (Test-Path $launchpadExe) {
    Write-Host "Deleting $launchpadExe..."
    Remove-Item -Path $launchpadExe -Force
    Write-Host "Deleted $launchpadExe."
} else {
    Write-Host "$launchpadExe does not exist. Skipping."
}

if (Test-Path $autonomiDir) {
    Write-Host "Deleting $autonomiDir..."
    Remove-Item -Path $autonomiDir -Recurse -Force
    Write-Host "Deleted $autonomiDir."
} else {
    Write-Host "$autonomiDir does not exist. Skipping."
}

Write-Host ""
Write-Host "Cleanup complete."
