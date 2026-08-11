@echo off
REM Windows: double-click this file to start your local search engine.
cd /d "%~dp0"

where docker >nul 2>nul
if errorlevel 1 (
  echo Docker isn't installed yet.
  echo Install Docker Desktop ^(free^), then double-click this file again:
  echo   https://www.docker.com/products/docker-desktop/
  echo.
  pause
  exit /b 1
)

REM Give SearXNG a random secret on first run.
findstr /C:"REPLACE_ME" config\settings.yml >nul 2>nul
if not errorlevel 1 (
  for /f %%i in ('powershell -NoProfile -Command "[guid]::NewGuid().ToString('N')+[guid]::NewGuid().ToString('N')"') do set SECRET=%%i
  powershell -NoProfile -Command "(Get-Content config\settings.yml) -replace 'REPLACE_ME', '%SECRET%' | Set-Content config\settings.yml"
)

echo Starting local search...
docker compose up -d
if errorlevel 1 ( pause & exit /b 1 )

echo.
echo Local search is running at:  http://localhost:8888
echo In the app:  Manage key -^> SearXNG base URL = http://localhost:8888  (leave token blank), then Save.
echo It will auto-start whenever Docker is running. Enable Docker Desktop's "Start when you sign in" to have it ready after a reboot.
echo.
pause
