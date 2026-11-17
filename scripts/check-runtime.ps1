param([Parameter(Mandatory=$true)][string]$Executable)
$ErrorActionPreference = 'Stop'
$stream = [IO.File]::OpenRead([IO.Path]::GetFullPath($Executable))
$reader = [IO.BinaryReader]::new($stream)
try {
    if ($reader.ReadUInt16() -ne 0x5a4d) { throw '不是有效的 Windows 程序' }
    $stream.Position = 0x3c
    $pe = $reader.ReadUInt32()
    $stream.Position = $pe
    if ($reader.ReadUInt32() -ne 0x4550 -or $reader.ReadUInt16() -ne 0x8664) { throw '发布程序必须是 Windows x64' }
    $sectionCount = $reader.ReadUInt16()
    $stream.Position = $pe + 20
    $optionalSize = $reader.ReadUInt16()
    $optional = $pe + 24
    $stream.Position = $optional
    if ($reader.ReadUInt16() -ne 0x20b) { throw '发布程序必须使用 PE32+ 格式' }
    $sections = @()
    for ($i = 0; $i -lt $sectionCount; $i++) {
        $stream.Position = $optional + $optionalSize + 40 * $i + 8
        $virtualSize = $reader.ReadUInt32()
        $address = $reader.ReadUInt32()
        $rawSize = $reader.ReadUInt32()
        $sections += @{Address=$address; Size=[Math]::Max($virtualSize,$rawSize); Offset=$reader.ReadUInt32()}
    }
    function Resolve-Rva([uint32]$Address) {
        foreach ($section in $sections) {
            if ($Address -ge $section.Address -and $Address -lt $section.Address + $section.Size) {
                return $section.Offset + $Address - $section.Address
            }
        }
        throw '程序的导入表地址无效'
    }
    $dependencies = @()
    # 普通导入和延迟导入都必须检查，避免在首次调用功能时才缺少运行库。
    foreach ($table in @(@{Index=1; Stride=20; NameOffset=12}, @{Index=13; Stride=32; NameOffset=4})) {
        $stream.Position = $optional + 112 + 8 * $table.Index
        $address = $reader.ReadUInt32()
        $size = $reader.ReadUInt32()
        if ($address -eq 0) { continue }
        $offset = Resolve-Rva $address
        for ($i = 0; $i + $table.Stride -le $size; $i += $table.Stride) {
            $stream.Position = $offset + $i + $table.NameOffset
            $nameAddress = $reader.ReadUInt32()
            if ($nameAddress -eq 0) { break }
            $stream.Position = Resolve-Rva $nameAddress
            $name = ''
            while (($byte = $reader.ReadByte()) -ne 0) { $name += [char]$byte }
            $dependencies += $name
        }
    }
    $runtime = @($dependencies | Where-Object { $_ -match '^(vcruntime|msvcp|msvcr|api-ms-win-crt-).*\.dll$' })
    if ($runtime.Count) { throw ('发布程序仍依赖外部 C/C++ 运行库：' + ($runtime -join ', ')) }
    Write-Output '运行库检查通过：Windows x64，无外部 C/C++ 运行库依赖。'
} finally { $reader.Dispose(); $stream.Dispose() }
