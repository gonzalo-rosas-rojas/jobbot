<#
.SYNOPSIS
  Registra la tarea programada que corre "run-daily" cada hora, sin ventana y sin guardar claves.

.DESCRIPTION
  Crea la tarea "\Personal\jobbot run-daily" para el usuario que corre este script. El nombre dice lo que es y la tarea
  se ve en la lista del Programador: una tarea que esconde lo que corre parece malware a quien revise el equipo.

  Cada hora el propio programa decide: dentro de la ventana de ejecucion corre run-daily (el descubrimiento tiene su
  propio tope de 6 horas); fuera de ella, o con STOP, sale al instante sin tocar nada. Por eso el disparador no tiene
  que saber de horarios.

  Como corre:
    - LogonType Interactive: solo con tu sesion de Windows iniciada (bloqueada vale). No se guarda ninguna clave, y la
      sesion de Claude vive en tu perfil ya cargado. S4U correria tambien sin sesion, pero entonces el perfil y la
      credencial no estan donde el programa los espera: no se usa.
    - RunLevel Limited: sin privilegios de administrador.
    - Sin consola: el comando va envuelto en "conhost.exe --headless", que arranca la consola sin ventana, para que no
      salte una ventana cada hora. La tarea no lleva la propiedad "Hidden" (que solo la esconderia de la lista). Con
      -ShowWindow se omite el envoltorio (para depurar).
    - WorkingDirectory = la raiz del repositorio, para que el programa encuentre profile\ y data\.
    - Sin variables heredadas: una tarea NO hereda las variables de la ventana donde la creaste; su entorno se arma al
      arrancar con las del usuario y del sistema, y %USERPROFILE% puede apuntar a C:\Users\Default si el perfil no esta
      cargado (KB 2968540). Por eso nada depende de ellas: el ejecutable va con ruta absoluta (-Exe) y el directorio de
      configuracion de Claude con ruta absoluta en profile\claude.toml.
    - -StartWhenAvailable (si el PC estaba apagado, corre al volver), -MultipleInstances IgnoreNew (una corrida larga
      no se pisa con la siguiente) y -ExecutionTimeLimit de 2 horas.

  El comando que ejecuta es scripts\run-daily.cmd, que llama al ejecutable, rota el registro (data\logs\jobbot.log) y
  descarta el registro de las horas fuera de ventana.

  Este script NO se corre solo ni al instalar nada: tu lo ejecutas, cuando quieras que el bot trabaje solo. Antes,
  mira lo que haria con -DryRun (o -WhatIf).

  Un worktree de git NO es el lugar: la tarea quedaria apuntando a una carpeta temporal (su target\release, su
  run-daily.cmd, su profile\ y su data\) que se borra cuando termina la rama. Si este script esta en un worktree
  enlazado (su ".git" es un archivo, no una carpeta), avisa en rojo y exige -Exe explicito, fuera del worktree; con
  -DryRun solo muestra el aviso rojo. Lo seguro es correrlo desde el checkout principal.

.PARAMETER Exe
  Ejecutable de jobbot, ruta absoluta. Por defecto, target\release\jobbot.exe en la raiz del repositorio donde esta este
  script (compilalo antes con "cargo build --release"). Desde un worktree enlazado es obligatorio y no puede estar
  dentro de ese worktree.

.PARAMETER TaskName
  Nombre de la tarea. Por defecto "jobbot run-daily".

.PARAMETER TaskPath
  Carpeta de la tarea en el Programador. Por defecto "\Personal\".

.PARAMETER EveryMinutes
  Cada cuantos minutos se dispara. Por defecto 60.

.PARAMETER ShowWindow
  No esconde la consola (para ver una corrida mientras depuras).

.PARAMETER DryRun
  Solo imprime lo que registraria. No toca el Programador de tareas ni exige que exista el ejecutable.

.EXAMPLE
  powershell -ExecutionPolicy Bypass -File scripts\schedule.ps1 -DryRun

.EXAMPLE
  powershell -ExecutionPolicy Bypass -File scripts\schedule.ps1
#>
[CmdletBinding(SupportsShouldProcess = $true)]
param(
    [string]$Exe = '',
    [string]$TaskName = 'jobbot run-daily',
    [string]$TaskPath = '\Personal\',
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
Write-Host 'Lo que se registraria:'
Write-Host ('  Tarea:        {0}{1}' -f $TaskPath, $TaskName)
Write-Host ('  Usuario:      {0}  (LogonType Interactive, RunLevel Limited; sin clave guardada)' -f $userId)
Write-Host ('  Programa:     {0} {1}' -f $execute, $argument)
Write-Host ('  Carpeta:      {0}' -f $root)
Write-Host ('  Disparador:   cada {0} min desde {1:yyyy-MM-dd HH:mm}' -f $EveryMinutes, $start)
Write-Host '  Ajustes:      visible en la lista, sin ventana, StartWhenAvailable, una sola instancia (IgnoreNew), limite de 2 h, corre con bateria'
Write-Host ('  Ejecutable:   {0}  ({1})' -f $Exe, $(if (Test-Path -LiteralPath $Exe) { 'existe' } else { 'NO existe todavia' }))
Write-Host ''

if ($inWorktree) {
    Write-Host 'AVISO: este script esta en un worktree de git, no en el checkout principal.' -ForegroundColor Red
    Write-Host ('  La tarea quedaria apuntando a {0}: su run-daily.cmd, su profile\ y su data\ (la carpeta de trabajo)' -f $root) -ForegroundColor Red
    Write-Host '  y se rompe el dia que se borre el worktree. Registrala desde el checkout principal.' -ForegroundColor Red
    if (-not $exeGiven) {
        Write-Host '  Sin -Exe explicito, un registro real se niega (con -DryRun solo se muestra este aviso).' -ForegroundColor Red
    } elseif ($exeInsideWorktree) {
        Write-Host '  El -Exe dado esta dentro del worktree: un registro real se niega.' -ForegroundColor Red
    }
    Write-Host ''
}

if ($DryRun) {
    Write-Host 'DryRun: no se registro nada.'
    return
}

if ($inWorktree -and (-not $exeGiven -or $exeInsideWorktree)) {
    throw "Esto es un worktree de git ($root): pasa -Exe con la ruta absoluta de un jobbot.exe fuera de el, o corre el script desde el checkout principal."
}

if (-not (Test-Path -LiteralPath $Exe)) {
    throw "Falta $Exe. Ejecuta primero: cargo build --release"
}
if (-not (Test-Path -LiteralPath $wrapper)) {
    throw "Falta $wrapper."
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
    Write-Host ('Tarea registrada: {0}{1}' -f $TaskPath, $TaskName)
    Get-ScheduledTask -TaskPath $TaskPath -TaskName $TaskName | Format-List TaskPath, TaskName, State
    Write-Host ''
    Write-Host ('Probarla ahora:     Start-ScheduledTask -TaskPath "{0}" -TaskName "{1}"' -f $TaskPath, $TaskName)
    Write-Host 'Ver la ultima:      jobbot status   (muestra cuando corrio, como termino y que cuenta de Claude uso)'
    Write-Host 'Detener todo:       jobbot stop     (o crea el archivo STOP)'
    Write-Host 'Quitarla:           powershell -ExecutionPolicy Bypass -File scripts\unschedule.ps1'
}
