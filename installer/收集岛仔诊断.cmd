@echo off
rem Portable package: the diagnostics script is embedded in daozai.exe.
start "" "%~dp0daozai.exe" --collect-diagnostics
