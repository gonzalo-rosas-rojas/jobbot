@echo off
rem Lo ejecuta el Programador de tareas cada hora (ver scripts\schedule.ps1), que le pasa la ruta absoluta del
rem ejecutable como primer argumento; sin argumento usa target\release\jobbot.exe de esta carpeta.
rem Codigo 4 = fuera de la ventana de ejecucion: es un exito silencioso y no escribe nada en el registro.
cd /d "%~dp0.."
set "JOBBOT_EXE=%~1"
if "%JOBBOT_EXE%"=="" set "JOBBOT_EXE=target\release\jobbot.exe"
if not exist data\logs mkdir data\logs
rem Rota el registro cuando pasa de 5 MB, para que no crezca sin limite.
if exist data\logs\jobbot.log for %%A in (data\logs\jobbot.log) do if %%~zA GTR 5000000 move /y data\logs\jobbot.log data\logs\jobbot.old.log >nul
"%JOBBOT_EXE%" run-daily > data\logs\last-run.tmp 2>&1
set JOBBOT_CODE=%ERRORLEVEL%
if "%JOBBOT_CODE%"=="4" (
    del data\logs\last-run.tmp
    exit /b 0
)
type data\logs\last-run.tmp >> data\logs\jobbot.log
del data\logs\last-run.tmp
exit /b %JOBBOT_CODE%
