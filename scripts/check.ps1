<#
.SYNOPSIS
  The one command that says whether the tree is fit to commit: format, lints, tests, supply chain.
.PARAMETER Fast
  Skips the test run (format, lints and supply chain only).
.PARAMETER Strict
  A step skipped because its tool is not installed counts as a failure. Without a remote, this
  check is the only gate the code passes, so the owner's machine runs strict by default: either
  JOBBOT_CHECK_STRICT=1, or a file named check-strict in the private state folder (JOBBOT_HOME, or
  else %LOCALAPPDATA%\jobbot). A fresh clone has neither, so it runs what it can and says what it
  skipped.
#>
param([switch]$Fast, [switch]$Strict)
$ErrorActionPreference = 'Stop'
$here = if ($PSScriptRoot) { $PSScriptRoot } else { Split-Path -Parent $MyInvocation.MyCommand.Path }
Set-Location (Join-Path $here '..')

$stateRoot = if ($env:JOBBOT_HOME) { $env:JOBBOT_HOME } elseif ($env:LOCALAPPDATA) { Join-Path $env:LOCALAPPDATA 'jobbot' } else { $null }
$strictBy = if ($Strict) { '-Strict' }
    elseif ($env:JOBBOT_CHECK_STRICT -eq '1') { 'JOBBOT_CHECK_STRICT=1' }
    elseif ($stateRoot -and (Test-Path (Join-Path $stateRoot 'check-strict'))) { (Join-Path $stateRoot 'check-strict') }
    else { $null }
if ($strictBy) { Write-Host "strict: a skipped step fails (set by $strictBy)" -ForegroundColor Cyan }

$script:failed = @()
$script:skipped = @()

function Step([string]$Name, [scriptblock]$Body) {
    Write-Host "==> $Name" -ForegroundColor Cyan
    & $Body
    if ($LASTEXITCODE -ne 0) {
        $script:failed += $Name
        Write-Host "FAILED: $Name" -ForegroundColor Red
    }
}

function Optional-Step([string]$Tool, [string]$Name, [scriptblock]$Body) {
    if (Get-Command $Tool -ErrorAction SilentlyContinue) {
        Step $Name $Body
    }
    else {
        $script:skipped += $Name
        $script:missingTools += $Tool
        Write-Host "SKIPPED: $Name ($Tool is not installed)" -ForegroundColor Yellow
    }
}
$script:missingTools = @()

Step 'rustfmt' { cargo fmt --all -- --check }
Step 'clippy' { cargo clippy --locked --workspace --all-targets -- -D warnings }
if (-not $Fast) { Step 'tests' { cargo test --locked --workspace } }
Optional-Step 'cargo-deny' 'cargo deny' { cargo deny check }
Optional-Step 'cargo-audit' 'cargo audit' { cargo audit }
Optional-Step 'npm' 'npm audit (panel-src)' { npm audit --prefix panel-src --audit-level=high }

if ($strictBy -and $script:skipped.Count -gt 0) {
    $script:failed += ($script:skipped | ForEach-Object { "$_ (skipped)" })
}
if ($script:failed.Count -gt 0) {
    Write-Host ("Failed: " + ($script:failed -join ', ')) -ForegroundColor Red
    exit 1
}
if ($script:skipped.Count -gt 0) {
    $tools = ($script:missingTools | Select-Object -Unique) -join ', '
    Write-Host ("check: OK, but {0} skipped (install {1}): {2}" -f $script:skipped.Count, $tools, ($script:skipped -join ', ')) -ForegroundColor Yellow
    exit 0
}
Write-Host 'check: OK' -ForegroundColor Green
