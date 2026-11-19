@echo off
setlocal
cd /d "%~dp0" || exit /b 1
where py >nul 2>&1
if errorlevel 1 goto python
py -3 launcher.py %*
exit /b %errorlevel%
:python
python launcher.py %*
exit /b %errorlevel%
