# 只保存 Claude Code 通过标准输入提供的额度，不读取登录令牌或发送网络请求。
$ErrorActionPreference = 'Stop'
try {
    $data = [Console]::In.ReadToEnd() | ConvertFrom-Json
    $identityPath = Join-Path $PSScriptRoot '.claude.json'
    if (!(Test-Path -LiteralPath $identityPath) -and (Split-Path $PSScriptRoot -Leaf) -eq '.claude') {
        $identityPath = $PSScriptRoot + '.json'
    }
    $identity = Get-Content -LiteralPath $identityPath -Raw -Encoding UTF8 | ConvertFrom-Json
    $account = $identity.oauthAccount
    $limits = [ordered]@{}
    foreach ($key in @('five_hour', 'seven_day')) {
        $window = $data.rate_limits.$key
        if ($null -ne $window -and $window.used_percentage -is [ValueType] -and $window.used_percentage -isnot [bool]) {
            $used = [double]$window.used_percentage
            if ($used -ge 0 -and $used -le 100) {
                $limits[$key] = @{ used_percentage = $used; resets_at = $window.resets_at }
            }
        }
    }
    if ($limits.Count -gt 0 -and $account.accountUuid -and $account.organizationUuid) {
        $record = @{ version = 1; account = $account.accountUuid; organization = $account.organizationUuid; at = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds(); rate_limits = $limits }
        $path = Join-Path $PSScriptRoot 'daozai-usage.json'
        $temporary = $path + '.' + [Guid]::NewGuid().ToString('N') + '.tmp'
        try {
            [IO.File]::WriteAllText($temporary, ($record | ConvertTo-Json -Depth 5 -Compress), [Text.UTF8Encoding]::new($false))
            Move-Item -LiteralPath $temporary -Destination $path -Force
        } finally {
            if (Test-Path -LiteralPath $temporary) { Remove-Item -LiteralPath $temporary }
        }
    }
    $parts = @($data.model.display_name)
    if ($limits.Contains('five_hour')) { $parts += '5h: ' + $limits['five_hour'].used_percentage + '%' }
    if ($limits.Contains('seven_day')) { $parts += '7d: ' + $limits['seven_day'].used_percentage + '%' }
    [Console]::WriteLine(($parts | Where-Object { $_ }) -join ' | ')
} catch {
    # 缺少本地数据时保持安静，不触发登录或网络回退。
    exit 0
}
