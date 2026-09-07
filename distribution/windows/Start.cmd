@echo off
powershell.exe -NoProfile -File "%~dp0steward.ps1" -Action Start
if errorlevel 1 pause
