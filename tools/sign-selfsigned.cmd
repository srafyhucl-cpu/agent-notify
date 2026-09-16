@echo off
rem Wrapper so both Inno Setup and the build scripts can call the signer as
rem "<tool> sign <file>" (Windows cannot execute .ps1 directly). Keep ASCII-only.
setlocal
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0sign-selfsigned.ps1" %*
exit /b %ERRORLEVEL%
