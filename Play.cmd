@echo off
setlocal
cd /d "%~dp0"
rem Build first: cargo rebuilds only what changed, so Play.cmd always starts
rem the current sources (an up-to-date build returns at once).
powershell -NoProfile -ExecutionPolicy Bypass -File tools\Build-Client.ps1
if errorlevel 1 (
  echo.
  echo The client build failed; see the messages above.
  pause
  exit /b 1
)
start "DSOR" "%~dp0target\fast\dsor-client.exe" %*
