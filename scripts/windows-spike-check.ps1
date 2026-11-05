#Requires -Version 5.1
<#
  VietKara Studio — Windows GPUI spike, automated checks.
  Runbook: docs/windows-spike.md (plan RUST_REWRITE_PLAN.md §4.1, §9 step 3).
  Usage (PowerShell, from repo root):
    powershell -ExecutionPolicy Bypass -File scripts\windows-spike-check.ps1
  Writes a log to windows-spike-results.log (gitignored via *.log).
  Exit code: 0 only if every automated step passes. A failed
  `cargo check -p vietkara-app` is a spike finding, not a reason to skip
  recording it — the log keeps the full output.
#>
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$LogFile = Join-Path $PWD 'windows-spike-results.log'
$Failed = @()

function Write-Log {
    param([string]$Message)
    $line = "$(Get-Date -Format 'yyyy-MM-dd HH:mm:ss') $Message"
    Write-Host $line
    Add-Content -Path $LogFile -Value $line
}

function Invoke-Step {
    param([string]$Name, [string[]]$Command)
    Write-Log "=== $Name : $($Command -join ' ')"
    $previousErrorActionPreference = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    & $Command[0] $Command[1..($Command.Count - 1)] 2>&1 |
        ForEach-Object { Write-Log "    $_" }
    $exitCode = $LASTEXITCODE
    $ErrorActionPreference = $previousErrorActionPreference
    if ($exitCode -ne 0) {
        Write-Log "FAIL: $Name - exit code $exitCode"
        $script:Failed += $Name
        return
    }
    Write-Log "PASS: $Name"
}

"VietKara Studio Windows spike - $(Get-Date -Format 'yyyy-MM-dd HH:mm:ss')" |
    Set-Content -Path $LogFile

Write-Log '--- Environment ---'
foreach ($tool in @('rustc', 'cargo', 'git')) {
    $found = Get-Command $tool -ErrorAction SilentlyContinue
    if ($null -eq $found) { Write-Log "MISSING: $tool"; $Failed += "missing $tool" }
    else { Write-Log "$tool : $($found.Source)" }
}
if ($Failed.Count -eq 0) {
    Invoke-Step 'rustc version' @('rustc', '--version')
    Invoke-Step 'cargo version' @('cargo', '--version')
    Invoke-Step 'git branch' @('git', 'branch', '--show-current')
    Invoke-Step 'git log' @('git', 'log', '--oneline', '-1')
    Invoke-Step 'git status' @('git', 'status', '--short', '--branch')
}

Write-Log '--- Media tools (required by vietkara-media tests) ---'
foreach ($tool in @('ffmpeg', 'ffprobe')) {
    $found = Get-Command $tool -ErrorAction SilentlyContinue
    if ($null -eq $found) { Write-Log "MISSING: $tool (media tests require it)"; $Failed += "missing $tool" }
    else { Invoke-Step "$tool version" @($tool, '-version') }
}
if ($Failed.Count -gt 0) {
    Write-Log ("FAILED REQUIRED ENVIRONMENT: " + ($Failed -join '; '))
    Write-Log 'Automated checks were not run because required tools are missing or failed.'
    exit 1
}

Write-Log '--- Automated checks (Linux baseline 2026-11-05: 62/62 pass, 6 headless crates) ---'
Invoke-Step 'cargo fmt --check' @('cargo', 'fmt', '--check')
Invoke-Step 'cargo test headless crates' @('cargo', 'test', '-p', 'vietkara-domain', '-p', 'vietkara-storage', '-p', 'vietkara-jobs', '-p', 'vietkara-media', '-p', 'vietkara-render', '-p', 'vietkara-ai-bridge')
Invoke-Step 'cargo clippy headless crates' @('cargo', 'clippy', '-p', 'vietkara-domain', '-p', 'vietkara-storage', '-p', 'vietkara-jobs', '-p', 'vietkara-media', '-p', 'vietkara-render', '-p', 'vietkara-ai-bridge', '--all-targets', '--', '-D', 'warnings')
Write-Log '--- GPUI Windows build check: vietkara-app (GPUI 0.2.2) ---'
Invoke-Step 'cargo check vietkara-app' @('cargo', 'check', '-p', 'vietkara-app')
Write-Log '--- Full workspace checks (include vietkara-app/GPUI) ---'
Invoke-Step 'dialog spike round-trip tests' @('cargo', 'test', '-p', 'vietkara-app', '--example', 'dialog_spike')
Invoke-Step 'cargo test workspace' @('cargo', 'test', '--workspace')
Invoke-Step 'cargo clippy workspace' @('cargo', 'clippy', '--workspace', '--all-targets', '--', '-D', 'warnings')

Write-Log '--- Summary ---'
if ($Failed.Count -eq 0) {
    Write-Log 'ALL AUTOMATED CHECKS PASSED. Continue with the manual checks in docs/windows-spike.md section 4.'
    exit 0
}
Write-Log ("FAILED STEPS: " + ($Failed -join '; '))
Write-Log 'Record the failures in docs/windows-spike.md §5 before deciding go/no-go on GPUI.'
exit 1
