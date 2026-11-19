@echo off
setlocal DisableDelayedExpansion
set "ST_PROCESSOR=%PROCESSOR_ARCHITECTURE%"
if defined PROCESSOR_ARCHITEW6432 set "ST_PROCESSOR=%PROCESSOR_ARCHITEW6432%"
if /I "%ST_PROCESSOR%"=="AMD64" goto x64
if /I "%ST_PROCESSOR%"=="ARM64" goto arm64
echo SoundTime requires 64-bit Windows on Intel/AMD or ARM. 1>&2
exit /b 1
:x64
set "ST_ARCH=x64"
goto run
:arm64
set "ST_ARCH=arm64"
:run
set "ST_ENGINE=%~dp0bin\windows-%ST_ARCH%\soundtime.exe"
if exist "%ST_ENGINE%" goto launch
echo SoundTime: extract the complete archive, including bin\windows-%ST_ARCH%\soundtime.exe. 1>&2
exit /b 1
:launch
"%ST_ENGINE%" %*
exit /b %ERRORLEVEL%
