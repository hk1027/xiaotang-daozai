# 岛仔：汽水音乐歌词自查（只读）
# 只读取版本号、文件大小、缓存文件头 64 字节和岛仔歌词日志中的汽水记录；
# 不读取歌词正文、登录信息或音频，也不修改任何文件。
# 用法：右键 → 使用 PowerShell 运行，或在 PowerShell 中执行：
#   powershell -NoProfile -ExecutionPolicy Bypass -File .\check-soda-lyrics.ps1
$ErrorActionPreference = 'Continue'
$out = New-Object System.Collections.Generic.List[string]
function Say($text) { $out.Add([string]$text); Write-Host $text }

Say "== 基本信息 =="
Say ("时间: " + (Get-Date -Format 'yyyy-MM-dd HH:mm:ss'))
Say ("Windows: " + [Environment]::OSVersion.VersionString)
Say ("APPDATA: " + $env:APPDATA)
Say ("LOCALAPPDATA: " + $env:LOCALAPPDATA)

Say ""
Say "== 汽水程序 =="
$soda = Get-Process SodaMusic -ErrorAction SilentlyContinue | Where-Object Path | Select-Object -First 1
if ($soda) {
    $info = (Get-Item $soda.Path).VersionInfo
    Say ("运行路径: " + $soda.Path)
    Say ("程序版本: " + $info.ProductVersion + " (" + $info.FileVersion + ")")
    $root = Split-Path (Split-Path $soda.Path)
    foreach ($name in 'launcher_config.json', 'Packages\config.json') {
        $file = Join-Path $root $name
        if (Test-Path $file) { Say ("${name}: " + ((Get-Content $file -Raw) -replace '\s+', ' ')) }
    }
    $packages = Join-Path $root 'Packages'
    if (Test-Path $packages) { Say ("界面更新包: " + ((Get-ChildItem $packages -Directory | Select-Object -ExpandProperty Name) -join ', ')) }
} else {
    Say "汽水未运行（请先打开汽水并播放一首歌再运行本脚本）"
}

Say ""
Say "== 汽水歌词缓存 =="
$db = Join-Path $env:APPDATA 'SodaMusic\LunaCacheV2\entries.db'
if (Test-Path $db) {
    $item = Get-Item $db
    Say ("entries.db: " + [math]::Round($item.Length / 1MB, 2) + " MB，修改于 " + $item.LastWriteTime)
    if ($item.Length -gt 32MB) { Say "!! 超过 32 MB：当前版本岛仔会放弃读取本地歌词" }
    try {
        $bytes = [byte[]]::new(64)
        $stream = [IO.File]::Open($db, 'Open', 'Read', 'ReadWrite,Delete')
        [void]$stream.Read($bytes, 0, 64); $stream.Close()
        $magic = [BitConverter]::ToUInt32($bytes, 24); $ver = [BitConverter]::ToUInt32($bytes, 28); $page = [BitConverter]::ToUInt32($bytes, 48)
        Say ("文件头: magic={0:x8} 版本={1} 页大小={2}" -f $magic, $ver, $page)
        if (("{0:x8}" -f $magic) -ne 'beefc0de' -or $ver -ne 2) { Say "!! 文件格式与岛仔已适配的格式不同" }
    } catch { Say ("!! 无法打开缓存文件: " + $_.Exception.Message) }
} else {
    Say ("!! 默认位置没有缓存文件: " + $db)
}
Say "其他位置的 LunaCacheV2 / entries.db："
$found = @()
foreach ($base in @($env:APPDATA, $env:LOCALAPPDATA) | Where-Object { $_ }) {
    $found += Get-ChildItem $base -Filter 'entries.db' -Recurse -Depth 4 -ErrorAction SilentlyContinue |
        Where-Object { $_.FullName -match 'Soda|Luna' }
}
if ($found) { foreach ($f in $found) { Say ("  " + $f.FullName + "  " + [math]::Round($f.Length / 1MB, 2) + " MB  " + $f.LastWriteTime) } } else { Say "  （未找到）" }
$luna = Join-Path $env:APPDATA 'SodaMusic'
if (Test-Path $luna) { Say ("SodaMusic 目录内容: " + ((Get-ChildItem $luna | Select-Object -ExpandProperty Name) -join ', ')) }

Say ""
Say "== 岛仔 =="
$daozai = Get-Process daozai -ErrorAction SilentlyContinue | Where-Object Path | Select-Object -First 1
if ($daozai) { Say ("运行路径: " + $daozai.Path + "  版本: " + (Get-Item $daozai.Path).VersionInfo.ProductVersion) } else { Say "岛仔未运行或无法读取路径（可能以管理员身份运行）" }
$log = Join-Path $env:APPDATA 'Daozai\diagnostics\lyrics.log'
if (Test-Path $log) {
    Say "最近的汽水歌词记录（最多 15 条）："
    Get-Content $log -Encoding UTF8 | Where-Object { $_ -match '汽水|soda' } | Select-Object -Last 15 | ForEach-Object { Say ("  " + $_) }
} else { Say ("!! 没有歌词日志: " + $log) }

$report = Join-Path ([Environment]::GetFolderPath('Desktop')) ('汽水歌词自查-' + (Get-Date -Format 'yyyyMMdd-HHmmss') + '.txt')
$out | Set-Content -Path $report -Encoding UTF8
Write-Host ""
Write-Host ("结果已保存到桌面: " + $report + "，请把这个文件发给我们。")
