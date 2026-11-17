@echo off
setlocal
chcp 65001 >nul
rem 岛仔：编译当前任务分支的本机测试版
cd /d "%~dp0"
if not exist "target\claude-skin\." mkdir "target\claude-skin" || exit /b 1
set "LOG=%~dp0target\claude-skin\build.log"
set CARGO=cargo
set "CARGO_BUILD_JOBS=1"
where cargo >nul 2>nul || set CARGO=%USERPROFILE%\.cargo\bin\cargo.exe
echo [%date% %time%] 开始编译 > "%LOG%"
"%CARGO%" --version >> "%LOG%" 2>&1
"%CARGO%" +stable-x86_64-pc-windows-msvc build --locked --release --bin daozai --target x86_64-pc-windows-msvc >> "%LOG%" 2>&1
set "BUILD_EXIT=%errorlevel%"
echo EXIT=%BUILD_EXIT% >> "%LOG%"
echo [%date% %time%] 结束 >> "%LOG%"
rustup show active-toolchain >> "%LOG%" 2>&1
echo.
if not "%BUILD_EXIT%"=="0" (
    echo 编译失败，请查看 "%LOG%"。
    exit /b %BUILD_EXIT%
)
echo 编译成功，日志位于 "%LOG%"。
exit /b 0
