<#
.SYNOPSIS
  Quita la tarea programada que creo scripts\schedule.ps1.

.DESCRIPTION
  Elimina "\Personal\jobbot run-daily" (o la que indiques) del Programador de tareas y, si la carpeta "\Personal\" queda
  vacia, la borra tambien. No toca el repositorio, la base de datos ni el archivo STOP. Si la tarea no existe, lo dice y
  termina sin error.

  Una corrida que ya estuviera en marcha no se interrumpe sola: para frenarla de inmediato usa "jobbot stop".

.PARAMETER TaskName
  Nombre de la tarea. Por defecto "jobbot run-daily". Una tarea registrada antes con el nombre anterior se quita con
  -TaskName 'Index Refresh'.

.PARAMETER TaskPath
  Carpeta de la tarea en el Programador. Por defecto "\Personal\".

.PARAMETER DryRun
  Solo imprime lo que quitaria.

.EXAMPLE
  powershell -ExecutionPolicy Bypass -File scripts\unschedule.ps1 -DryRun

.EXAMPLE
  powershell -ExecutionPolicy Bypass -File scripts\unschedule.ps1
#>
[CmdletBinding(SupportsShouldProcess = $true)]
param(
    [string]$TaskName = 'jobbot run-daily',
    [string]$TaskPath = '\Personal\',
    [switch]$DryRun
)

$ErrorActionPreference = 'Stop'
if (-not $TaskPath.StartsWith('\')) { $TaskPath = '\' + $TaskPath }
if (-not $TaskPath.EndsWith('\')) { $TaskPath = $TaskPath + '\' }

$task = Get-ScheduledTask -TaskPath $TaskPath -TaskName $TaskName -ErrorAction SilentlyContinue
if (-not $task) {
    Write-Host ('No hay ninguna tarea {0}{1}: nada que quitar.' -f $TaskPath, $TaskName)
    return
}

Write-Host ('Se quitaria la tarea {0}{1} (estado: {2}).' -f $TaskPath, $TaskName, $task.State)
if ($DryRun) {
    Write-Host 'DryRun: no se quito nada.'
    return
}

if ($PSCmdlet.ShouldProcess(('{0}{1}' -f $TaskPath, $TaskName), 'Unregister-ScheduledTask')) {
    Unregister-ScheduledTask -TaskPath $TaskPath -TaskName $TaskName -Confirm:$false
    Write-Host ('Tarea quitada: {0}{1}' -f $TaskPath, $TaskName)

    # The folder the registration created stays behind in Task Scheduler once empty; remove it too (best effort).
    if ($TaskPath -ne '\') {
        try {
            $service = New-Object -ComObject 'Schedule.Service'
            $service.Connect()
            $folder = $service.GetFolder($TaskPath.TrimEnd('\'))
            if ($folder.GetTasks(1).Count -eq 0 -and $folder.GetFolders(0).Count -eq 0) {
                $parent = $service.GetFolder('\')
                $parent.DeleteFolder($TaskPath.Trim('\'), 0)
                Write-Host ('Carpeta vacia quitada: {0}' -f $TaskPath)
            }
        } catch {
            Write-Host ('La carpeta {0} se queda (no se pudo quitar: {1}).' -f $TaskPath, $_.Exception.Message)
        }
    }
}
