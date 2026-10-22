<#
.SYNOPSIS
  Removes the scheduled task that scripts\schedule.ps1 created.

.DESCRIPTION
  Deletes "\jobbot\run-daily" (or the one you name) from Task Scheduler and, if the "\jobbot\" folder is left
  empty, deletes it too. It does not touch the repository, the database or the STOP file. If the task does not exist
  it says so and exits without an error.

  A run that is already in progress is not interrupted by this: to stop it right away use "jobbot stop".

.PARAMETER TaskName
  Task name. Default "run-daily".

.PARAMETER TaskPath
  Task folder in Task Scheduler. Default "\jobbot\".

.PARAMETER DryRun
  Only prints what it would remove.

.EXAMPLE
  powershell -ExecutionPolicy Bypass -File scripts\unschedule.ps1 -DryRun

.EXAMPLE
  powershell -ExecutionPolicy Bypass -File scripts\unschedule.ps1
#>
[CmdletBinding(SupportsShouldProcess = $true)]
param(
    [string]$TaskName = 'run-daily',
    [string]$TaskPath = '\jobbot\',
    [switch]$DryRun
)

$ErrorActionPreference = 'Stop'
if (-not $TaskPath.StartsWith('\')) { $TaskPath = '\' + $TaskPath }
if (-not $TaskPath.EndsWith('\')) { $TaskPath = $TaskPath + '\' }

$task = Get-ScheduledTask -TaskPath $TaskPath -TaskName $TaskName -ErrorAction SilentlyContinue
if (-not $task) {
    Write-Host ('There is no task {0}{1}: nothing to remove.' -f $TaskPath, $TaskName)
    return
}

Write-Host ('Would remove the task {0}{1} (state: {2}).' -f $TaskPath, $TaskName, $task.State)
if ($DryRun) {
    Write-Host 'DryRun: nothing was removed.'
    return
}

if ($PSCmdlet.ShouldProcess(('{0}{1}' -f $TaskPath, $TaskName), 'Unregister-ScheduledTask')) {
    Unregister-ScheduledTask -TaskPath $TaskPath -TaskName $TaskName -Confirm:$false
    Write-Host ('Task removed: {0}{1}' -f $TaskPath, $TaskName)

    # The folder the registration created stays behind in Task Scheduler once empty; remove it too (best effort).
    if ($TaskPath -ne '\') {
        try {
            $service = New-Object -ComObject 'Schedule.Service'
            $service.Connect()
            $folder = $service.GetFolder($TaskPath.TrimEnd('\'))
            if ($folder.GetTasks(1).Count -eq 0 -and $folder.GetFolders(0).Count -eq 0) {
                $parent = $service.GetFolder('\')
                $parent.DeleteFolder($TaskPath.Trim('\'), 0)
                Write-Host ('Empty folder removed: {0}' -f $TaskPath)
            }
        } catch {
            Write-Host ('The folder {0} stays (could not remove it: {1}).' -f $TaskPath, $_.Exception.Message)
        }
    }
}
