$ErrorActionPreference = 'Stop'
$project = Split-Path $PSScriptRoot -Parent
$target = (New-Item -Path (Join-Path $project 'target') -ItemType Directory -Force).FullName
$root = Join-Path $target ("diagnostics-test-' " + [Guid]::NewGuid().ToString('N'))
$logs = Join-Path $root 'data\diagnostics'
$packages = Join-Path $root 'out'
New-Item -ItemType Directory -Path $logs -Force | Out-Null
try {
    $fakePath = Join-Path $env:USERPROFILE 'DaozaiFixture\player.exe'
    $json = @{ Path = $fakePath } | ConvertTo-Json -Compress
    [IO.File]::WriteAllText((Join-Path $logs 'resources.log'), $json)
    [IO.File]::WriteAllText((Join-Path $logs 'frames.log'), 'frame-current')
    [IO.File]::WriteAllText((Join-Path $logs 'frames.previous.log'), 'frame-previous')
    & (Join-Path $project 'scripts\collect-diagnostics.ps1') -DataDirectory (Split-Path $logs) -OutputDirectory $packages | Out-Null
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $files = @(Get-ChildItem -LiteralPath $packages -Filter '*.zip')
    if ($files.Count -ne 1) { throw '收集一次应只产出一份诊断包' }
    $zip = [IO.Compression.ZipFile]::OpenRead($files[0].FullName)
    try {
        $reader = [IO.StreamReader]::new($zip.GetEntry('resources.log').Open())
        try { $masked = ($reader.ReadToEnd() | ConvertFrom-Json).Path } finally { $reader.Dispose() }
        if ($masked -ne '%USERPROFILE%\DaozaiFixture\player.exe') { throw '诊断包中的 JSON 路径没有隐去用户目录' }
        foreach ($name in @('frames.log','frames.previous.log')) {
            if (!$zip.GetEntry($name)) { throw "诊断包缺少 $name" }
        }
    } finally { $zip.Dispose() }
    Write-Output '通过：诊断归档、转义路径脱敏、当前与轮转慢帧日志'
} finally {
    foreach ($dir in @($packages,$logs)) {
        if (Test-Path -LiteralPath $dir) {
            Get-ChildItem -LiteralPath $dir -File | Remove-Item -Force
            [IO.Directory]::Delete($dir)
        }
    }
    [IO.Directory]::Delete((Split-Path $logs))
    [IO.Directory]::Delete($root)
}
