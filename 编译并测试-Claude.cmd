@echo off
setlocal
chcp 65001 >nul
rem 岛仔：编译正式程序并运行完整回归，结果写入 target\claude-check\build.log
cd /d "%~dp0"
if not exist "%~dp0target\claude-check" mkdir "%~dp0target\claude-check" || exit /b 1
set LOG=%~dp0target\claude-check\build.log
set CARGO=cargo
set "CARGO_BUILD_JOBS=1"
where cargo >nul 2>nul || set CARGO=%USERPROFILE%\.cargo\bin\cargo.exe
echo [%date% %time%] 开始编译 > "%LOG%"
"%CARGO%" --version >> "%LOG%" 2>&1
echo ===== 编译正式程序 ===== >> "%LOG%"
"%CARGO%" +stable-x86_64-pc-windows-msvc build --locked --release --bin daozai --target x86_64-pc-windows-msvc >> "%LOG%" 2>&1
set "BUILD_EXIT=%errorlevel%"
echo BUILD_EXIT=%BUILD_EXIT% >> "%LOG%"
echo ===== 运行测试（含 Windows 专属测试） ===== >> "%LOG%"
"%CARGO%" +stable-x86_64-pc-windows-msvc test --locked --all-targets --features record --target x86_64-pc-windows-msvc -- --test-threads=1 >> "%LOG%" 2>&1
set "TEST_EXIT=%errorlevel%"
echo TEST_EXIT=%TEST_EXIT% >> "%LOG%"
echo [%date% %time%] 结束 >> "%LOG%"
echo.
if not "%BUILD_EXIT%"=="0" (echo 编译失败，日志位于 "%LOG%"。 & exit /b %BUILD_EXIT%)
if not "%TEST_EXIT%"=="0" (echo 测试失败，日志位于 "%LOG%"。 & exit /b %TEST_EXIT%)
echo 编译和测试通过，日志位于 "%LOG%"。
pause
exit /b 0
