param([Parameter(Mandatory=$true)][string]$WorkPath, [Parameter(Mandatory=$true)][string]$TargetExe, [int]$ParentProcessId)
$ErrorActionPreference = 'Stop'
# 更新只使用当前 PowerShell 自带模块，避免继承其他终端版本的模块路径。
$env:PSModulePath = [IO.Path]::Combine($PSHOME, 'Modules')
$cacheRoot = [IO.Path]::GetFullPath((Join-Path $env:LOCALAPPDATA 'Daozai\Updates')).TrimEnd('\')
$workRoot = [IO.Path]::GetFullPath($WorkPath).TrimEnd('\')
$target = [IO.Path]::GetFullPath($TargetExe)
$targetDir = Split-Path $target -Parent
$staged = Join-Path $targetDir ('daozai-update-' + [Guid]::NewGuid().ToString('N') + '.exe')
$backup = Join-Path $workRoot 'previous.exe'
$replaced = $false
$exited = $false
function Assert-PlainPath([string]$PathToCheck) {
    $ancestor = [IO.Path]::GetFullPath($PathToCheck)
    while ($ancestor) {
        if ((Test-Path -LiteralPath $ancestor) -and ((Get-Item -LiteralPath $ancestor -Force).Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw '更新目录不能包含目录链接。' }
        $ancestor = Split-Path $ancestor -Parent
    }
}
try {
    if (-not $workRoot.StartsWith($cacheRoot + '\', [StringComparison]::OrdinalIgnoreCase)) { throw '更新缓存目录无效。' }
    Assert-PlainPath $workRoot
    Assert-PlainPath $target
    if (-not (Test-Path -LiteralPath $target -PathType Leaf)) { throw '找不到当前程序。' }
    $version = (Get-Content -LiteralPath (Join-Path $workRoot 'version.txt') -Raw).Trim()
    if ($version -notmatch '^\d+\.\d+\.\d+$') { throw '更新版本号无效。' }
    $archive = Join-Path $workRoot 'update.zip'
    $expectedHash = (Get-Content -LiteralPath (Join-Path $workRoot 'hash.txt') -Raw).Trim()
    if ($expectedHash -notmatch '^[a-fA-F0-9]{64}$' -or (Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash -ne $expectedHash) { throw '更新包校验失败。' }
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $zip = [IO.Compression.ZipFile]::OpenRead($archive)
    try {
        $name = 'Daozai-v' + $version + '-windows-x64/daozai.exe'
        $entries = @($zip.Entries | Where-Object {$_.FullName.Replace('\','/') -ceq $name})
        if ($entries.Count -ne 1 -or $entries[0].Length -gt 200MB) { throw '更新包中缺少唯一的岛仔程序。' }
        [IO.Compression.ZipFileExtensions]::ExtractToFile($entries[0], $staged)
    } finally { $zip.Dispose() }
    $actual = [Diagnostics.FileVersionInfo]::GetVersionInfo($staged).FileVersion
    if ($actual -ne $version -and $actual -ne "$version.0") { throw '安装包版本不匹配。' }
    Set-Content -LiteralPath (Join-Path $workRoot 'ready') -Value 'ok'
    if ($ParentProcessId -gt 0) {
        $parent = Get-Process -Id $ParentProcessId -ErrorAction SilentlyContinue
        if ($parent -and -not $parent.WaitForExit(60000)) { throw '岛仔尚未退出，请稍后重试。' }
    }
    $exited = $true
    # 缓存可能启用 EFS；先复制备份，避免原子替换时跨加密边界移动旧程序。
    [IO.File]::Copy($target, $backup, $true)
    [IO.File]::Replace($staged, $target, [NullString]::Value)
    $replaced = $true
    Start-Process -FilePath $target -WorkingDirectory $targetDir -WindowStyle Hidden
    $key = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Daozai'
    $registered = Get-ItemProperty -LiteralPath $key -ErrorAction SilentlyContinue
    if ($registered -and [string]::Equals($registered.InstallLocation.TrimEnd('\'), $targetDir, [StringComparison]::OrdinalIgnoreCase)) {
        Set-ItemProperty -LiteralPath $key -Name DisplayVersion -Value $version -ErrorAction SilentlyContinue
        # 只统计岛仔自己的文件，软件管家不会把共享安装文件夹的大小算到岛仔头上。
        $own = @('daozai.exe','Uninstall.exe','LICENSE','THIRD_PARTY_NOTICES.txt') | ForEach-Object { Join-Path $targetDir $_ } | Where-Object { Test-Path -LiteralPath $_ -PathType Leaf }
        $sizeKb = [int][Math]::Ceiling((($own | ForEach-Object { (Get-Item -LiteralPath $_).Length }) | Measure-Object -Sum).Sum / 1KB)
        if ($sizeKb -gt 0) { Set-ItemProperty -LiteralPath $key -Name EstimatedSize -Value $sizeKb -Type DWord -ErrorAction SilentlyContinue }
        # 开始菜单「收集岛仔诊断」：旧版安装没有，或指向已不再附带的 cmd；统一改为调用程序内嵌的诊断收集。失败不影响更新。
        try {
            $link = Join-Path ([Environment]::GetFolderPath('Programs')) '收集岛仔诊断.lnk'
            $shortcut = (New-Object -ComObject WScript.Shell).CreateShortcut($link)
            $shortcut.TargetPath = $target; $shortcut.Arguments = '--collect-diagnostics'
            $shortcut.WorkingDirectory = $targetDir; $shortcut.IconLocation = "$target,0"; $shortcut.Save()
        } catch { }
        foreach ($legacy in @('收集岛仔诊断.cmd', 'scripts\collect-diagnostics.ps1')) {
            try { $path = Join-Path $targetDir $legacy; if (Test-Path -LiteralPath $path -PathType Leaf) { Assert-PlainPath $path; Remove-Item -LiteralPath $path -Force } } catch { }
        }
        try { $scripts = Join-Path $targetDir 'scripts'; if ((Test-Path -LiteralPath $scripts) -and -not (Get-ChildItem -LiteralPath $scripts -Force)) { Remove-Item -LiteralPath $scripts -Force } } catch { }
    }
    # 只清理已经确认位于专用缓存内的本次更新目录。
    if ($workRoot.StartsWith($cacheRoot + '\', [StringComparison]::OrdinalIgnoreCase)) {
        try { Assert-PlainPath $workRoot; Remove-Item -LiteralPath $workRoot -Recurse -Force } catch { }
    }
} catch {
    $message = $_.Exception.Message
    if ($replaced -and (Test-Path -LiteralPath $backup)) { [IO.File]::Copy($backup, $target, $true) }
    if (Test-Path -LiteralPath $staged) { Remove-Item -LiteralPath $staged -Force }
    if (-not $exited) {
        if ($workRoot.StartsWith($cacheRoot + '\', [StringComparison]::OrdinalIgnoreCase)) {
            [IO.File]::WriteAllText((Join-Path $workRoot 'error.txt'), $message)
        }
        exit 1
    }
    if ($exited) { Start-Process -FilePath $target -WorkingDirectory $targetDir -WindowStyle Hidden }
    Add-Type -AssemblyName System.Windows.Forms
    [System.Windows.Forms.MessageBox]::Show("更新未完成：$message`r`n原版本已保留，可稍后重试或通过浏览器下载。", '岛仔更新') | Out-Null
    exit 1
}
