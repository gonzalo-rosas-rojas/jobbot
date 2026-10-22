<#
.SYNOPSIS
  The one command that says whether the tree is fit to commit: format, lints, tests, supply chain.
.PARAMETER Fast
  Skips the test run (format, lints and supply chain only).
#>
param([switch]$Fast)
$ErrorActionPreference = 'Stop'
$here = if ($PSScriptRoot) { $PSScriptRoot } else { Split-Path -Parent $MyInvocation.MyCommand.Path }
Set-Location (Join-Path $here '..')

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
        Write-Host "SKIPPED: $Name ($Tool is not installed)" -ForegroundColor Yellow
    }
}

Step 'rustfmt' { cargo fmt --all -- --check }
Step 'clippy' { cargo clippy --locked --workspace --all-targets -- -D warnings }
if (-not $Fast) { Step 'tests' { cargo test --locked --workspace } }
Optional-Step 'cargo-deny' 'cargo deny' { cargo deny check }
Optional-Step 'cargo-audit' 'cargo audit' { cargo audit }

if ($script:skipped.Count -gt 0) { Write-Host ("Skipped: " + ($script:skipped -join ', ')) -ForegroundColor Yellow }
if ($script:failed.Count -gt 0) {
    Write-Host ("Failed: " + ($script:failed -join ', ')) -ForegroundColor Red
    exit 1
}
Write-Host 'check: OK' -ForegroundColor Green
