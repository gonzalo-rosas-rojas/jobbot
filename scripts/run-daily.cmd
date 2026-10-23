@echo off
rem Task Scheduler runs this every hour (see scripts\schedule.ps1) and passes the absolute path of the
rem executable as the first argument; without one it uses target\release\jobbot.exe at the repository root.
rem Exit code 4 = outside the run window: a silent success that writes nothing to the log.
cd /d "%~dp0.."
set "JOBBOT_EXE=%~1"
if "%JOBBOT_EXE%"=="" set "JOBBOT_EXE=target\release\jobbot.exe"
if not exist data\logs mkdir data\logs
rem Rotate the log once it passes 5 MB, so it does not grow without limit.
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
