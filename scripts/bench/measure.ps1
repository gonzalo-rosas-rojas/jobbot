<#
.SYNOPSIS
  Times the read-only commands of a built jobbot against a COPY of a real database.
.DESCRIPTION
  Copies the profile and the database of -SourceRoot into a temporary project (jobbot never opens
  the originals), runs each command -Runs times and prints a Markdown table: the cold first run,
  then p50, p95, min and max wall time in milliseconds and the best-effort peak working set.
  The temporary copy, which holds personal data, is deleted when the script ends.
#>
param(
    [Parameter(Mandatory = $true)][string]$SourceRoot,
    [string]$Exe = '',
    [int]$Runs = 30,
    [string[]]$Commands = @('status', 'today', 'top')
)
$ErrorActionPreference = 'Stop'
# $PSScriptRoot is not always filled in inside a param() default, so the default is resolved here.
$here = if ($PSScriptRoot) { $PSScriptRoot } else { Split-Path -Parent $MyInvocation.MyCommand.Path }
if (-not $Exe) { $Exe = Join-Path $here '..\..\target\release\jobbot.exe' }

function Get-Percentile([double[]]$Sorted, [double]$P) {
    $rank = [math]::Ceiling($P * $Sorted.Count) - 1
    $Sorted[[math]::Max(0, [int]$rank)]
}

$exe = (Resolve-Path $Exe).Path
$work = Join-Path ([IO.Path]::GetTempPath()) ('jobbot-bench-' + [guid]::NewGuid().ToString('N').Substring(0, 8))
New-Item -ItemType Directory -Path $work, "$work\data" | Out-Null
try {
    Copy-Item (Join-Path $SourceRoot 'profile') "$work\profile" -Recurse
    foreach ($name in 'jobbot.db', 'jobbot.db-wal', 'prep.db', 'prep.db-wal') {
        $from = Join-Path $SourceRoot "data\$name"
        if (Test-Path $from) { Copy-Item $from "$work\data\$name" }
    }

    $rows = foreach ($command in $Commands) {
        $times = @(); $peak = 0; $cold = 0
        for ($i = 0; $i -le $Runs; $i++) {
            $sw = [Diagnostics.Stopwatch]::StartNew()
            $p = Start-Process -FilePath $exe -ArgumentList $command -WorkingDirectory $work -NoNewWindow -PassThru `
                -RedirectStandardOutput "$work\out.txt" -RedirectStandardError "$work\err.txt"
            # Touching the handle keeps it open, so ExitCode and the times survive the exit.
            $null = $p.Handle
            while (-not $p.HasExited) {
                try { $p.Refresh(); $peak = [math]::Max($peak, $p.PeakWorkingSet64) } catch { }
                Start-Sleep -Milliseconds 2
            }
            $p.WaitForExit()
            $sw.Stop()
            if ($p.ExitCode -ne 0) {
                throw "jobbot $command exited with $($p.ExitCode): $(Get-Content "$work\err.txt" -Raw)"
            }
            # The process's own clock excludes the cost of Start-Process; fall back to the stopwatch.
            try { $ms = ($p.ExitTime - $p.StartTime).TotalMilliseconds } catch { $ms = $sw.Elapsed.TotalMilliseconds }
            if ($i -eq 0) { $cold = $ms } else { $times += $ms }
        }
        $sorted = $times | Sort-Object
        [pscustomobject]@{
            Command = $command
            Cold    = [math]::Round($cold)
            P50     = [math]::Round((Get-Percentile $sorted 0.50))
            P95     = [math]::Round((Get-Percentile $sorted 0.95))
            Min     = [math]::Round($sorted[0])
            Max     = [math]::Round($sorted[-1])
            PeakMB  = [math]::Round($peak / 1MB, 1)
        }
    }

    '| command | cold (ms) | p50 (ms) | p95 (ms) | min (ms) | max (ms) | peak memory (MB) |'
    '|---|---|---|---|---|---|---|'
    foreach ($r in $rows) {
        "| jobbot $($r.Command) | $($r.Cold) | $($r.P50) | $($r.P95) | $($r.Min) | $($r.Max) | $($r.PeakMB) |"
    }
}
finally {
    Remove-Item $work -Recurse -Force -ErrorAction SilentlyContinue
}
