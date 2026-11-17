@echo off
setlocal
chcp 65001 >nul
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\collect-diagnostics.ps1"
if errorlevel 1 echo 诊断收集失败，请将此窗口截图发给开发者。
pause
