<#
.SYNOPSIS
  Checks the Playwright driver that playwright-rs downloaded at build time against its publishers' hashes.

.DESCRIPTION
  The build script of playwright-rs downloads Node.js (nodejs.org) and playwright-core (registry.npmjs.org) over
  HTTPS and checks no hash; jobbot runs both every time it opens the browser. This compares the cached copy with
  what the publishers list:
    - node.exe against the "win-x64/node.exe" line of Node's SHASUMS256.txt (its version is read from the file's
      version resource, so the binary under check is never run);
    - every file of package\ against the playwright-core tarball, whose sha512 must equal npm's dist.integrity.
  It also reads Node's release index and says whether node.exe is older than the newest security release of its
  own major line.
  It downloads SHASUMS256.txt, Node's release index, npm's metadata and the tarball (about 3 MB) into a temporary
  folder, unpacks the tarball there with the system tar, compares, and deletes the folder. Nothing it downloads is
  run.

  Run it after the first build of a new playwright-rs version (the cache is keyed by driver version, so that build
  is the only one that downloads), and now and then for the security releases. Exit 0: every file matched and
  node.exe is current. 1: something differs, is missing or extra; do not use this driver. 2: every file matched,
  but node.exe is behind a security release (jobbot makes no HTTP requests from Node, the browser fetches the
  pages, so this waits for the next playwright-rs update).

.PARAMETER Cache
  The driver cache. Default %LOCALAPPDATA%\playwright-rust (or PLAYWRIGHT_DRIVER_CACHE_DIR\playwright-rust).

.PARAMETER DriverVersion
  The playwright-core version playwright-rs pins (PLAYWRIGHT_VERSION in its build.rs). Default 1.64.0, the one
  playwright-rs 0.20.1 uses: update it with the pin in Cargo.toml. Since 0.20 the driver's folder also names its
  Node version (playwright-<version>-win32_x64-node<node>); the newest folder of that driver version is checked.

.EXAMPLE
  powershell -ExecutionPolicy Bypass -File scripts\verify-playwright-driver.ps1
#>
param(
    [string]$Cache = '',
    [string]$DriverVersion = '1.64.0'
)
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

if (-not $Cache) {
    $base = if ($env:PLAYWRIGHT_DRIVER_CACHE_DIR) { $env:PLAYWRIGHT_DRIVER_CACHE_DIR } else { $env:LOCALAPPDATA }
    $Cache = Join-Path $base 'playwright-rust'
}
$folders = @(Get-ChildItem (Join-Path $Cache $DriverVersion) -Directory -ErrorAction SilentlyContinue |
    Where-Object { $_.Name -eq "playwright-$DriverVersion-win32_x64" -or $_.Name -like "playwright-$DriverVersion-win32_x64-node*" })
if ($folders.Count -eq 0) { throw "No driver $DriverVersion under $Cache (build once with that playwright-rs version first)." }
# One folder per Node pin; the newest is the one the last build assembled.
$driver = ($folders | Sort-Object LastWriteTime -Descending | Select-Object -First 1).FullName
Write-Host "driver folder: $driver" -ForegroundColor Cyan
$nodeExe = Join-Path $driver 'node.exe'
if (-not (Test-Path $nodeExe)) { throw "No node.exe in $driver." }

$work = Join-Path ([System.IO.Path]::GetTempPath()) ("playwright-verify-" + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $work | Out-Null
$problems = 0
try {
    # node.exe: its version from the file's metadata, its hash against Node's published list.
    $nodeVersion = (Get-Item $nodeExe).VersionInfo.ProductVersion
    $sums = (Invoke-WebRequest -UseBasicParsing "https://nodejs.org/dist/v$nodeVersion/SHASUMS256.txt").Content
    $line = ($sums -split "`n") | Where-Object { $_ -match '\s+win-x64/node\.exe\s*$' } | Select-Object -First 1
    $want = if ($line) { ($line -split '\s+')[0].ToLowerInvariant() } else { '' }
    $have = (Get-FileHash -Algorithm SHA256 $nodeExe).Hash.ToLowerInvariant()
    if ($want -and $want -eq $have) {
        Write-Host "node.exe $nodeVersion matches nodejs.org SHASUMS256 ($have)" -ForegroundColor Green
    }
    else {
        $problems++
        Write-Host "node.exe $nodeVersion does NOT match: published '$want', cached '$have'" -ForegroundColor Red
    }

    # How current: the newest security release of node.exe's own major line (the index lists newest first).
    $index = Invoke-WebRequest -UseBasicParsing 'https://nodejs.org/dist/index.json' |
        Select-Object -ExpandProperty Content | ConvertFrom-Json
    $major = $nodeVersion.Split('.')[0]
    $security = $index | Where-Object { $_.security -and $_.version -like "v$major.*" } | Select-Object -First 1
    $behind = $null
    if ($security -and ([version]$security.version.TrimStart('v') -gt [version]$nodeVersion)) {
        $behind = $security.version
        Write-Host ("node.exe {0} is older than {1} ({2}), the newest security release of Node {3}" -f `
            $nodeVersion, $behind, $security.date, $major) -ForegroundColor Yellow
    }
    else {
        Write-Host "node.exe $nodeVersion has every security release of Node $major" -ForegroundColor Green
    }

    # playwright-core: the tarball's sha512 against npm's integrity, then every file against the cache.
    $meta = Invoke-WebRequest -UseBasicParsing "https://registry.npmjs.org/playwright-core/$DriverVersion" |
        Select-Object -ExpandProperty Content | ConvertFrom-Json
    $tgz = Join-Path $work 'playwright-core.tgz'
    Invoke-WebRequest -UseBasicParsing $meta.dist.tarball -OutFile $tgz
    $hex = (Get-FileHash -Algorithm SHA512 $tgz).Hash
    $bytes = [byte[]]::new($hex.Length / 2)
    for ($i = 0; $i -lt $bytes.Length; $i++) { $bytes[$i] = [Convert]::ToByte($hex.Substring(2 * $i, 2), 16) }
    $integrity = 'sha512-' + [Convert]::ToBase64String($bytes)
    if ($integrity -ne $meta.dist.integrity) {
        $problems++
        Write-Host "the downloaded tarball does not match npm's integrity: stopping" -ForegroundColor Red
        exit 1
    }
    $unpacked = Join-Path $work 'unpacked'
    New-Item -ItemType Directory -Path $unpacked | Out-Null
    & "$env:SystemRoot\System32\tar.exe" -xzf $tgz -C $unpacked
    if ($LASTEXITCODE -ne 0) { throw "tar could not unpack $tgz" }

    $published = Join-Path $unpacked 'package'
    $cached = Join-Path $driver 'package'
    $relative = { param($root, $path) $path.Substring($root.Length).TrimStart('\') }
    $publishedFiles = Get-ChildItem $published -Recurse -File | ForEach-Object { & $relative $published $_.FullName }
    $cachedFiles = Get-ChildItem $cached -Recurse -File | ForEach-Object { & $relative $cached $_.FullName }
    $same = 0
    foreach ($file in $publishedFiles) {
        $mine = Join-Path $cached $file
        if (-not (Test-Path $mine)) { $problems++; Write-Host "missing in the cache: $file" -ForegroundColor Red; continue }
        if ((Get-FileHash $mine).Hash -eq (Get-FileHash (Join-Path $published $file)).Hash) { $same++ }
        else { $problems++; Write-Host "differs: $file" -ForegroundColor Red }
    }
    foreach ($file in ($cachedFiles | Where-Object { $publishedFiles -notcontains $_ })) {
        $problems++
        Write-Host "in the cache but not published: $file" -ForegroundColor Red
    }
    Write-Host ("playwright-core {0}: tarball matches npm's integrity; {1} of {2} files identical" -f `
        $DriverVersion, $same, $publishedFiles.Count) -ForegroundColor $(if ($problems -eq 0) { 'Green' } else { 'Yellow' })
}
finally {
    Remove-Item -Recurse -Force $work -ErrorAction SilentlyContinue
}
if ($problems -gt 0) { Write-Host "$problems problem(s): do not trust this driver" -ForegroundColor Red; exit 1 }
if ($behind) {
    Write-Host "driver: verified, but its node.exe is behind security release $behind (update playwright-rs)" -ForegroundColor Yellow
    exit 2
}
Write-Host 'driver: verified' -ForegroundColor Green
