@echo off
setlocal
cd /d "%~dp0" || exit /b 1
py -3 -c "import sys; sys.exit(sys.version_info < (3, 9))" >nul 2>&1
if not errorlevel 1 (
    py -3 launcher.py %*
    exit /b
)
python -c "import sys; sys.exit(sys.version_info < (3, 9))" >nul 2>&1
if not errorlevel 1 (
    python launcher.py %*
    exit /b
)
echo Cette copie des sources demande Python 3.9 ou une version plus recente.
echo Vous pouvez aussi utiliser le programme SoundTime Installer des distributions du projet.
pause
exit /b 1
