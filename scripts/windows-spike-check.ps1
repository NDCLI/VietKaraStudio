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
    try {
        & $Command[0] $Command[1..($Command.Count - 1)] 2>&1 |
            ForEach-Object { Write-Log "    $_" }
        if ($LASTEXITCODE -ne 0) {
            throw "exit code $LASTEXITCODE"
        }
        Write-Log "PASS: $Name"
    }
    catch {
        Write-Log "FAIL: $Name — $_"
        $script:Failed += $Name
    }
}

"VietKara Studio Windows spike — $(Get-Date -Format 'yyyy-MM-dd HH:mm:ss')" |
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

Write-Log '--- Optional tools (warn only) ---'
foreach ($tool in @('ffmpeg', 'ffprobe')) {
    $found = Get-Command $tool -ErrorAction SilentlyContinue
    if ($null -eq $found) { Write-Log "OPTIONAL MISSING: $tool (probe step skipped)" }
    else { Invoke-Step "$tool version" @($tool, '-version') }
}

Write-Log '--- Automated checks (Linux baseline 2026-11-05: 62/62 pass, 6 headless crates) ---'
Invoke-Step 'cargo fmt --check' @('cargo', 'fmt', '--check')
Invoke-Step 'cargo test headless crates' @('cargo', 'test', '-p', 'vietkara-domain', '-p', 'vietkara-storage', '-p', 'vietkara-jobs', '-p', 'vietkara-media', '-p', 'vietkara-render', '-p', 'vietkara-ai-bridge')
Invoke-Step 'cargo clippy headless crates' @('cargo', 'clippy', '-p', 'vietkara-domain', '-p', 'vietkara-storage', '-p', 'vietkara-jobs', '-p', 'vietkara-media', '-p', 'vietkara-render', '-p', 'vietkara-ai-bridge', '--all-targets', '--', '-D', 'warnings')
Write-Log '--- Unproven step: vietkara-app (GPUI 0.2.2) has never built on any host ---'
Invoke-Step 'cargo check vietkara-app' @('cargo', 'check', '-p', 'vietkara-app')

Write-Log '--- Summary ---'
if ($Failed.Count -eq 0) {
    Write-Log 'ALL AUTOMATED CHECKS PASSED. Continue with the manual checks in docs/windows-spike.md §4.'
    exit 0
}
Write-Log ("FAILED STEPS: " + ($Failed -join '; '))
Write-Log 'Record the failures in docs/windows-spike.md §5 before deciding go/no-go on GPUI.'
exit 1
