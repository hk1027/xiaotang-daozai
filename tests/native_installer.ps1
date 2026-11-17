param([Parameter(Mandatory=$true)][string]$NsisCompiler)
$ErrorActionPreference = 'Stop'
$project = Split-Path $PSScriptRoot -Parent
$id = 'DaozaiSetupTest' + [Guid]::NewGuid().ToString('N')
$name = '岛仔安装测试-' + $id.Substring(15,8)
$root = Join-Path $project ('target\' + $id)
$installed = Join-Path $root 'installed files'
$key = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\' + $id
$data = Join-Path $env:APPDATA $id
New-Item -ItemType Directory -Path $root -Force | Out-Null
$sentinel = Join-Path $data 'settings.txt'
$source = Join-Path $root 'fixture.cs'
'class Fixture { static void Main() { } }' | Set-Content -LiteralPath $source
$fixture = Join-Path $root 'fixture.exe'
& (Join-Path $env:WINDIR 'Microsoft.NET\Framework64\v4.0.30319\csc.exe') /nologo /target:winexe "/out:$fixture" $source
if ($LASTEXITCODE -ne 0) { throw '测试程序编译失败' }
$setup = Join-Path $root 'Daozai-TestInstaller.exe'
& $NsisCompiler /V2 /WX "/DAPP_ID=$id" "/DAPP_NAME=$name" /DAPP_VERSION=9.8.7 "/DPROJECT_DIR=$project" "/DSOURCE_EXE=$fixture" "/DOUTPUT_FILE=$setup" (Join-Path $project 'installer\setup.nsi')
if ($LASTEXITCODE -ne 0) { throw '测试安装器编译失败' }
New-Item -ItemType Directory -Path $data -Force | Out-Null
Set-Content -LiteralPath $sentinel -Value 'keep-user-data'
function Run-Setup {
    param([string[]]$Options = @(), [switch]$UseRegisteredPath)
    $arguments = @('/S') + $Options
    if (-not $UseRegisteredPath) { $arguments += "/D=$installed" }
    $process = Start-Process -FilePath $setup -ArgumentList $arguments -WindowStyle Hidden -PassThru
    if (-not $process.WaitForExit(30000) -or $process.ExitCode -ne 0) { throw '安装失败或超时' }
}
function Assert-Installed {
    param([bool]$Desktop = $true, [bool]$AutoStart = $false)
    $entry = Get-ItemProperty -LiteralPath $key
    if ($entry.InstallLocation -ne $installed -or $entry.DisplayVersion -ne '9.8.7') { throw '安装注册信息不正确' }
    if ($entry.UninstallString -ne ('"' + (Join-Path $installed 'Uninstall.exe') + '"')) { throw '卸载命令不正确' }
    if (-not ($entry.EstimatedSize -gt 0 -and $entry.EstimatedSize -lt 100000)) { throw "登记大小不正确：$($entry.EstimatedSize) KB" }
    if ((Get-FileHash (Join-Path $installed 'daozai.exe')).Hash -ne (Get-FileHash $fixture).Hash) { throw '程序复制不正确' }
    $shell = New-Object -ComObject WScript.Shell
    $desktopLink = Join-Path $shell.SpecialFolders.Item('Desktop') "$name.lnk"
    if ((Test-Path -LiteralPath $desktopLink) -ne $Desktop) { throw '桌面快捷方式选项未生效' }
    $paths = @((Join-Path $env:APPDATA "Microsoft\Windows\Start Menu\Programs\$name.lnk"))
    if ($Desktop) { $paths += $desktopLink }
    foreach ($path in $paths) {
        if (-not (Test-Path -LiteralPath $path)) { throw "快捷方式不存在：$path" }
        # WScript 在英文系统上读取中文快捷方式文件名时可能返回空目标；复制原始文件后检查同一份内容。
        $probe = Join-Path $root 'shortcut-probe.lnk'
        Copy-Item -LiteralPath $path -Destination $probe -Force
        $actual = $shell.CreateShortcut($probe).TargetPath
        $expected = Join-Path $installed 'daozai.exe'
        if (-not (Test-Path -LiteralPath $actual) -or (Get-Item -LiteralPath $actual).FullName -ne (Get-Item -LiteralPath $expected).FullName) { throw "快捷方式目标不正确：$actual；预期：$expected" }
    }
    $run = (Get-ItemProperty -LiteralPath 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run' -ErrorAction SilentlyContinue).$id
    if ($AutoStart) {
        if ($run -ne ('"' + (Join-Path $installed 'daozai.exe') + '"')) { throw '开机自启选项或路径不正确' }
    } elseif ($run) { throw '未勾选自启却设置了开机启动' }
    if ((Get-Content -LiteralPath $sentinel -Raw).Trim() -ne 'keep-user-data') { throw '用户数据被修改' }
}
try {
    Run-Setup
    Assert-Installed
    Write-Output '通过：首次安装、含空格路径、快捷方式和卸载注册'
    Set-Content -LiteralPath (Join-Path $installed 'daozai.exe') -Value 'previous-version'
    Set-Content -LiteralPath (Join-Path $installed 'uninstall.ps1') -Value 'previous-uninstaller'
    Set-ItemProperty -LiteralPath $key -Name UninstallString -Value 'powershell.exe legacy-uninstall.ps1'
    Run-Setup
    Assert-Installed
    if (Test-Path -LiteralPath (Join-Path $installed 'uninstall.ps1')) { throw '旧卸载脚本未移除' }
    Write-Output '通过：覆盖升级、旧脚本安装迁移、保留数据'
    Run-Setup -Options @('/DESKTOP=0','/AUTOSTART=1')
    Assert-Installed -Desktop $false -AutoStart $true
    Write-Output '通过：取消桌面快捷方式、开启自启'
    Run-Setup -UseRegisteredPath
    Assert-Installed -Desktop $false -AutoStart $true
    Write-Output '通过：升级保留现有路径和选项'
    Run-Setup -Options @('/DESKTOP=1','/AUTOSTART=0')
    Assert-Installed
    Write-Output '通过：恢复桌面快捷方式、关闭自启'
    $mutex = [Threading.Mutex]::new($false, "Local\$id.SingleInstance")
    try {
        $process = Start-Process -FilePath $setup -ArgumentList @('/S',"/D=$installed") -WindowStyle Hidden -PassThru
        if (-not $process.WaitForExit(30000) -or $process.ExitCode -eq 0) { throw '运行中的程序未阻止覆盖安装' }
        Assert-Installed
        Write-Output '通过：运行中拒绝覆盖程序'
    } finally { $mutex.Dispose() }
    Run-Setup -Options @('/AUTOSTART=1')
    Assert-Installed -AutoStart $true
    Set-Content -LiteralPath (Join-Path $installed 'user-file.txt') -Value 'keep-extra-file'
    # 使用原安装路径运行原生卸载器，只操作本测试的独立产品标识。
    $process = Start-Process -FilePath (Join-Path $installed 'Uninstall.exe') -ArgumentList @('/S',"_?=$installed") -WindowStyle Hidden -PassThru
    if (-not $process.WaitForExit(30000) -or $process.ExitCode -ne 0) { throw '卸载失败或超时' }
    if ((Test-Path -LiteralPath $key) -or (Test-Path -LiteralPath (Join-Path $installed 'daozai.exe'))) { throw '卸载残留程序或注册信息' }
    if ((Get-ItemProperty -LiteralPath 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run' -ErrorAction SilentlyContinue).$id) { throw '卸载残留自启项' }
    if (-not (Test-Path -LiteralPath (Join-Path $installed 'user-file.txt'))) { throw '卸载误删额外文件' }
    if ((Get-Content -LiteralPath $sentinel -Raw).Trim() -ne 'keep-user-data') { throw '卸载误删用户数据' }
    Write-Output '通过：原生卸载、保留用户数据和额外文件'
    # 选了装着其他软件的共享文件夹：自动装进其中的专用子文件夹，登记大小只算岛仔自己的文件。
    $shared = Join-Path $root 'shared apps'
    New-Item -ItemType Directory -Path (Join-Path $shared 'Other App') -Force | Out-Null
    [IO.File]::WriteAllBytes((Join-Path $shared 'Other App\big.bin'), (New-Object byte[] (40MB)))
    Set-Content -LiteralPath (Join-Path $shared 'other.txt') -Value 'other-software'
    $installed = Join-Path $shared $id
    $process = Start-Process -FilePath $setup -ArgumentList @('/S',"/D=$shared") -WindowStyle Hidden -PassThru
    if (-not $process.WaitForExit(30000) -or $process.ExitCode -ne 0) { throw '共享文件夹安装失败或超时' }
    Assert-Installed
    if (Test-Path -LiteralPath (Join-Path $shared 'daozai.exe')) { throw '程序被直接装进了共享文件夹' }
    if ((Get-ItemProperty -LiteralPath $key).EstimatedSize -ge 40000) { throw '登记大小把共享文件夹里的其他软件算了进去' }
    Write-Output '通过：共享文件夹自动建立专用子文件夹、登记大小只算岛仔'
    Run-Setup -UseRegisteredPath
    Assert-Installed
    Write-Output '通过：专用子文件夹原地升级'
    $process = Start-Process -FilePath (Join-Path $installed 'Uninstall.exe') -ArgumentList @('/S',"_?=$installed") -WindowStyle Hidden -PassThru
    if (-not $process.WaitForExit(30000) -or $process.ExitCode -ne 0) { throw '共享文件夹卸载失败或超时' }
    if (-not (Test-Path -LiteralPath (Join-Path $shared 'other.txt')) -or -not (Test-Path -LiteralPath (Join-Path $shared 'Other App\big.bin'))) { throw '卸载误删共享文件夹里的其他软件' }
    Write-Output '通过：卸载不影响共享文件夹里的其他软件'
} finally {
    # 清理本测试的独立安装，避免失败时残留桌面快捷方式和自启项。
    if (Test-Path -LiteralPath $key) {
        $cleanupDirectory = (Get-ItemProperty -LiteralPath $key).InstallLocation
        $defaultDirectory = Join-Path $env:LOCALAPPDATA "Programs\$id"
        if ($cleanupDirectory -ne $installed -and $cleanupDirectory -ne $defaultDirectory -and $cleanupDirectory -ne (Join-Path $root "shared apps\$id")) { throw '测试清理目录超出预期范围' }
        $cleanup = Start-Process -FilePath (Join-Path $cleanupDirectory 'Uninstall.exe') -ArgumentList @('/S',"_?=$cleanupDirectory") -WindowStyle Hidden -PassThru
        $null = $cleanup.WaitForExit(30000)
    }
    if (-not (Test-Path -LiteralPath $key)) {
        Remove-Item -LiteralPath $sentinel
        Remove-Item -LiteralPath $data
    }
}
