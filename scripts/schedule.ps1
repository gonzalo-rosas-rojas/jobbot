<#
.SYNOPSIS
  Registers the scheduled task that runs "run-daily" every hour, without a window and without storing a password.

.DESCRIPTION
  Creates the task "\jobbot\run-daily" for the user running this script. The task is named after the program so
  it is easy to find and remove; change the name and folder with -TaskName and -TaskPath.

  Every hour the program itself decides: inside the run window it runs run-daily (discovery has its own 6-hour
  cap); outside it, or with STOP, it exits at once without touching anything. That is why the trigger does not need
  to know about schedules.

  How it runs:
    - LogonType Interactive: only while your Windows session is signed in (locked is fine). No password is stored,
      and the Claude session lives in your already-loaded profile. S4U would also run without a session, but then
      the profile and the credential are not where the program expects them, so it is not used.
    - RunLevel Limited: no administrator privileges.
    - Without a console window: the command is wrapped in "conhost.exe --headless", which starts the console
      without a window, so nothing flashes on screen every hour. -ShowWindow skips the wrapper (for debugging).
      The task itself is not hidden: Task Scheduler lists it under \jobbot\.
    - WorkingDirectory = the repository root, so the program finds profile\ and data\.
    - No inherited variables: a task does NOT inherit the variables of the window you created it from; its
      environment is built at start from the user's and the system's, and %USERPROFILE% can point to
      C:\Users\Default if the profile is not loaded (KB 2968540). So nothing depends on them: the executable goes with
      an absolute path (-Exe) and the Claude config directory with an absolute path in profile\claude.toml.
    - -StartWhenAvailable (if the PC was off, it runs when it is back), -MultipleInstances IgnoreNew (a long run is
      not stepped on by the next one) and an -ExecutionTimeLimit of 2 hours.

  The command it runs is scripts\run-daily.cmd, which calls the executable, rotates the log (data\logs\jobbot.log)
  and discards the log of the hours outside the window.

  This script never runs by itself or on install: you run it when you want the bot to work on its own. Before that,
  look at what it would do with -DryRun (or -WhatIf).

  A git worktree is NOT the place: the task would point at a temporary folder (its target\release, its
  run-daily.cmd, its profile\ and its data\) that is deleted when the branch is done. If this script is in a linked
  worktree (its ".git" is a file, not a folder), it warns in red and requires an explicit -Exe outside the
  worktree; with -DryRun it only shows the red warning. The safe place is the main checkout.

.PARAMETER Exe
  jobbot executable, absolute path. Default: target\release\jobbot.exe at the root of the repository holding this
  script (build it first with "cargo build --release"). From a linked worktree it is required and cannot be inside
  that worktree.

.PARAMETER TaskName
  Task name. Default "run-daily".

.PARAMETER TaskPath
  Task folder in Task Scheduler. Default "\jobbot\".

.PARAMETER EveryMinutes
  How many minutes between triggers. Default 60.

.PARAMETER ShowWindow
  Does not hide the console (to watch a run while debugging).

.PARAMETER DryRun
  Only prints what it would register. It does not touch Task Scheduler and does not require the executable to exist.

.EXAMPLE
  powershell -ExecutionPolicy Bypass -File scripts\schedule.ps1 -DryRun

.EXAMPLE
  powershell -ExecutionPolicy Bypass -File scripts\schedule.ps1
#>
[CmdletBinding(SupportsShouldProcess = $true)]
param(
    [string]$Exe = '',
    [string]$TaskName = 'run-daily',
    [string]$TaskPath = '\jobbot\',
    [ValidateRange(5, 1440)][int]$EveryMinutes = 60,
    [switch]$ShowWindow,
    [switch]$DryRun
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$wrapper = Join-Path $PSScriptRoot 'run-daily.cmd'
# The default is computed here and not in param(): $PSScriptRoot is empty there under "powershell -File".
$exeGiven = -not [string]::IsNullOrWhiteSpace($Exe)
if (-not $exeGiven) { $Exe = Join-Path $root 'target\release\jobbot.exe' }
$Exe = [System.IO.Path]::GetFullPath($Exe)
# A linked git worktree has ".git" as a FILE ("gitdir: ..."); the main checkout has it as a folder.
$inWorktree = Test-Path -LiteralPath (Join-Path $root '.git') -PathType Leaf
$rootPrefix = ([System.IO.Path]::GetFullPath($root)).TrimEnd('\') + '\'
$exeInsideWorktree = $Exe.StartsWith($rootPrefix, [System.StringComparison]::OrdinalIgnoreCase)
if (-not $TaskPath.StartsWith('\')) { $TaskPath = '\' + $TaskPath }
if (-not $TaskPath.EndsWith('\')) { $TaskPath = $TaskPath + '\' }
$userId = '{0}\{1}' -f $env:USERDOMAIN, $env:USERNAME

# "cmd.exe /c call "a" "b"" keeps both quoted paths whole (spaces in either). The "call" is deliberate: under
# "conhost --headless" a command line that starts with a quote (/c ""a" "b"") is mangled and nothing runs (tried on
# Windows 11 26200). The wrapper takes the exe as its argument, so the task never relies on a relative path or on
# the working directory to find it.
$inner = 'cmd.exe /c call "{0}" "{1}"' -f $wrapper, $Exe
if ($ShowWindow) {
    $execute = 'cmd.exe'
    $argument = '/c call "{0}" "{1}"' -f $wrapper, $Exe
} else {
    $execute = 'conhost.exe'
    $argument = '--headless ' + $inner
}

# Five minutes past the hour (so the run at 17:05 already falls inside a window that opens at 17:00), from the next
# hour on, for ten years.
$now = Get-Date
$start = $now.Date.AddHours($now.Hour + 1).AddMinutes(5)

Write-Host ''
Write-Host 'What would be registered:'
Write-Host ('  Task:         {0}{1}' -f $TaskPath, $TaskName)
Write-Host ('  User:         {0}  (LogonType Interactive, RunLevel Limited; no stored password)' -f $userId)
Write-Host ('  Program:      {0} {1}' -f $execute, $argument)
Write-Host ('  Folder:       {0}' -f $root)
Write-Host ('  Trigger:      every {0} min from {1:yyyy-MM-dd HH:mm}' -f $EveryMinutes, $start)
Write-Host '  Settings:     StartWhenAvailable, single instance (IgnoreNew), 2 h limit, runs on battery'
Write-Host ('  Executable:   {0}  ({1})' -f $Exe, $(if (Test-Path -LiteralPath $Exe) { 'exists' } else { 'does NOT exist yet' }))
Write-Host ''

if ($inWorktree) {
    Write-Host 'WARNING: this script is in a git worktree, not in the main checkout.' -ForegroundColor Red
    Write-Host ('  The task would point at {0}: its run-daily.cmd, its profile\ and its data\ (the working folder)' -f $root) -ForegroundColor Red
    Write-Host '  and would break the day the worktree is deleted. Register it from the main checkout.' -ForegroundColor Red
    if (-not $exeGiven) {
        Write-Host '  Without an explicit -Exe, a real registration is refused (with -DryRun only this warning is shown).' -ForegroundColor Red
    } elseif ($exeInsideWorktree) {
        Write-Host '  The given -Exe is inside the worktree: a real registration is refused.' -ForegroundColor Red
    }
    Write-Host ''
}

if ($DryRun) {
    Write-Host 'DryRun: nothing was registered.'
    return
}

if ($inWorktree -and (-not $exeGiven -or $exeInsideWorktree)) {
    throw "This is a git worktree ($root): pass -Exe with the absolute path of a jobbot.exe outside it, or run the script from the main checkout."
}

if (-not (Test-Path -LiteralPath $Exe)) {
    throw "Missing $Exe. Run first: cargo build --release"
}
if (-not (Test-Path -LiteralPath $wrapper)) {
    throw "Missing $wrapper."
}

$action = New-ScheduledTaskAction -Execute $execute -Argument $argument -WorkingDirectory $root
$trigger = New-ScheduledTaskTrigger -Once -At $start `
    -RepetitionInterval (New-TimeSpan -Minutes $EveryMinutes) -RepetitionDuration (New-TimeSpan -Days 3650)
$settings = New-ScheduledTaskSettingsSet -StartWhenAvailable -MultipleInstances IgnoreNew `
    -ExecutionTimeLimit (New-TimeSpan -Hours 2) -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries
$principal = New-ScheduledTaskPrincipal -UserId $userId -LogonType Interactive -RunLevel Limited

if ($PSCmdlet.ShouldProcess(('{0}{1}' -f $TaskPath, $TaskName), 'Register-ScheduledTask')) {
    Register-ScheduledTask -TaskName $TaskName -TaskPath $TaskPath -Action $action -Trigger $trigger `
        -Settings $settings -Principal $principal -Force | Out-Null
    Write-Host ('Task registered: {0}{1}' -f $TaskPath, $TaskName)
    Get-ScheduledTask -TaskPath $TaskPath -TaskName $TaskName | Format-List TaskPath, TaskName, State
    Write-Host ''
    Write-Host ('Try it now:         Start-ScheduledTask -TaskPath "{0}" -TaskName "{1}"' -f $TaskPath, $TaskName)
    Write-Host 'See the last run:   jobbot status   (shows when it ran, how it ended and which Claude account it used)'
    Write-Host 'Stop everything:    jobbot stop     (or create the STOP file)'
    Write-Host 'Remove it:          powershell -ExecutionPolicy Bypass -File scripts\unschedule.ps1'
}
