@echo off
powershell.exe -NoProfile -File "%~dp0steward.ps1" -Action Update
pause
