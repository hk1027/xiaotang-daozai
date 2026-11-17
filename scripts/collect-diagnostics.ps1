param(
    [string]$OutputDirectory = [Environment]::GetFolderPath('Desktop'),
    [string]$DataDirectory = $(if ($env:DAOZAI_DATA_DIR) { $env:DAOZAI_DATA_DIR } else { Join-Path $env:APPDATA 'Daozai' }),
    [switch]$RevealArchive
)
$ErrorActionPreference = 'Stop'
$collectionRoot = Join-Path ([IO.Path]::GetTempPath()) ('daozai-diagnostics-' + [guid]::NewGuid().ToString('N'))
$utf8 = New-Object System.Text.UTF8Encoding($true)
$missing = New-Object 'System.Collections.Generic.List[string]'
function Save-Text([string]$Name, [string]$Value) {
    foreach ($key in @('LOCALAPPDATA', 'APPDATA', 'USERPROFILE')) {
        $privatePath = [Environment]::GetEnvironmentVariable($key)
        if ($privatePath) {
            $placeholder = '%' + $key + '%'
            $Value = $Value -replace [regex]::Escape($privatePath.Replace('\','\\')), $placeholder
            $Value = $Value -replace [regex]::Escape($privatePath), $placeholder
        }
    }
    [IO.File]::WriteAllText((Join-Path $collectionRoot $Name), $Value, $utf8)
}
New-Item -ItemType Directory -Path $collectionRoot | Out-Null
try {
    $logs = @('last-error.log', 'last-graphics.log', 'last-startup.log', 'resources.log',
        'resources.previous.log', 'last-resource-anomaly.log', 'resource-guard.log',
        'weather-network.log', 'weather-network.previous.log',
        'messenger-events.log', 'messenger-events.previous.log', 'wechat-events.log', 'wechat-tray.log',
        'calendar.log', 'calendar.previous.log', 'lyrics.log', 'lyrics.previous.log',
        'music-gaps.log', 'music-gaps.previous.log', 'frames.log', 'frames.previous.log', 'auto-restart.log')
    foreach ($name in $logs + @('glass-diagnostics.txt')) {
        $source = if ($name -eq 'glass-diagnostics.txt') { Join-Path $DataDirectory $name } else { Join-Path (Join-Path $DataDirectory 'diagnostics') $name }
        if (!(Test-Path -LiteralPath $source -PathType Leaf)) { $missing.Add($name); continue }
        try { Save-Text $name ([IO.File]::ReadAllText($source)) }
        catch { $missing.Add($name + '：无法读取') }
    }
    $info = [ordered]@{ CollectedAt = (Get-Date).ToString('o'); MissingLogs = $missing.ToArray() }
    try {
        $os = Get-CimInstance Win32_OperatingSystem -OperationTimeoutSec 10
        $info.System = [ordered]@{ Name = [string]$os.Caption; Version = [string]$os.Version; Build = [string]$os.BuildNumber; BootedAt = $os.LastBootUpTime.ToString('o') }
        $info.Graphics = @(Get-CimInstance Win32_VideoController -OperationTimeoutSec 10 | ForEach-Object {
            [pscustomobject]@{ Name = [string]$_.Name; Driver = [string]$_.DriverVersion; DriverDate = [string]$_.DriverDate }
        })
    } catch { $info.SystemQuery = '无法读取系统或显卡信息' }
    try {
        Add-Type -AssemblyName System.Windows.Forms
        $info.Screens = @([System.Windows.Forms.Screen]::AllScreens | ForEach-Object {
            [pscustomobject]@{ Device = $_.DeviceName; Primary = $_.Primary; X = $_.Bounds.X; Y = $_.Bounds.Y; Width = $_.Bounds.Width; Height = $_.Bounds.Height }
        })
    } catch { $info.ScreensQuery = '无法读取显示器信息' }
    $settingsPath = Join-Path $DataDirectory 'settings.txt'
    if (Test-Path -LiteralPath $settingsPath) {
        $info.DisplaySettings = @([IO.File]::ReadAllLines($settingsPath) | Where-Object { $_ -match '^(skin|island_shape|dock|glass_blur|island_size|island_offset|settings_theme|minimal|pet)=' })
        # 功能开关（只有开 / 关和页面顺序，不含账号、密码或内容）：用于判断「开关没开」一类问题。
        $info.FeatureSettings = @([IO.File]::ReadAllLines($settingsPath) | Where-Object { $_ -match '^(notifications|wechat_notifications|wechat_tray_open|wecom_notifications|feishu_notifications|dingtalk_notifications|read_notifications|qq_only|visible_pages|page_order|lyrics_enabled|island_lyrics|render_compat|calendar_auto_sync|mail_enabled)=' })
    }
    try {
        Add-Type -TypeDefinition 'public static class DaozaiDiagnosticGui { [System.Runtime.InteropServices.DllImport("user32.dll")] public static extern uint GetGuiResources(System.IntPtr process, uint flags); }'
        $info.Processes = @(Get-Process -Name daozai -ErrorAction SilentlyContinue | ForEach-Object {
            $processInfo = [ordered]@{ Id = $_.Id; StartedAt = $_.StartTime.ToString('o'); Handles = $_.HandleCount; Threads = $_.Threads.Count; PrivateBytes = $_.PrivateMemorySize64; WorkingBytes = $_.WorkingSet64 }
            try {
                $processInfo.GdiObjects = [DaozaiDiagnosticGui]::GetGuiResources($_.Handle, 0)
                $processInfo.UserObjects = [DaozaiDiagnosticGui]::GetGuiResources($_.Handle, 1)
                $processInfo.Version = $_.MainModule.FileVersionInfo.FileVersion
                $processInfo.Sha256 = (Get-FileHash -LiteralPath $_.Path -Algorithm SHA256).Hash
            } catch { $processInfo.Details = '无法读取部分进程信息' }
            [pscustomobject]$processInfo
        })
    } catch { $info.ProcessQuery = '无法读取进程信息' }
    try {
        $faultQuery = "*[System[(EventID=1000 or EventID=1001 or EventID=1002) and TimeCreated[timediff(@SystemTime) <= 604800000]] and EventData[Data[@Name='AppName']='daozai.exe' or Data[@Name='P1']='daozai.exe']]"
        $info.ApplicationFaults = @(Get-WinEvent -LogName Application -FilterXPath $faultQuery -MaxEvents 30 -ErrorAction SilentlyContinue | ForEach-Object {
            $eventData = @{}
            foreach ($node in ([xml]$_.ToXml()).Event.EventData.Data) { if ($node.Name) { $eventData[[string]$node.Name] = $node.InnerText } }
            if ($eventData.AppName -ine 'daozai.exe' -and $eventData.P1 -ine 'daozai.exe') { return }
            $fault = [ordered]@{ At = $_.TimeCreated.ToString('o'); EventId = $_.Id; Provider = $_.ProviderName }
            foreach ($key in @('AppName', 'AppVersion', 'ModuleName', 'ModuleVersion', 'ExceptionCode', 'FaultingOffset', 'EventName', 'P1', 'P2', 'P3', 'P4', 'P5', 'P6', 'P7', 'P8')) {
                if ($eventData.ContainsKey($key)) { $fault[$key] = $eventData[$key] }
            }
            [pscustomobject]$fault
        })
        $info.DisplayResets = @(Get-WinEvent -FilterHashtable @{ LogName = 'System'; Id = 4101; StartTime = (Get-Date).AddDays(-7) } -MaxEvents 20 -ErrorAction SilentlyContinue | ForEach-Object {
            [pscustomobject]@{ At = $_.TimeCreated.ToString('o'); Provider = $_.ProviderName; EventId = $_.Id }
        })
    } catch { $info.EventQuery = '无法读取 Windows 事件记录' }
    # 消息客户端窗口快照：只记窗口类名、可见状态、样式和大小，不读窗口标题（标题里可能有聊天对象）。
    try {
        Add-Type -TypeDefinition @'
using System; using System.Text; using System.Collections.Generic; using System.Runtime.InteropServices;
public static class DaozaiDiagnosticWindows {
    delegate bool Visit(IntPtr window, IntPtr data);
    [DllImport("user32.dll")] static extern bool EnumWindows(Visit visit, IntPtr data);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr window, out uint pid);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetClassName(IntPtr window, StringBuilder name, int size);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr window);
    [DllImport("user32.dll")] static extern bool IsIconic(IntPtr window);
    [DllImport("user32.dll")] static extern int GetWindowLong(IntPtr window, int index);
    [DllImport("user32.dll")] static extern IntPtr GetWindow(IntPtr window, uint command);
    [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr window, out Rect rect);
    struct Rect { public int Left, Top, Right, Bottom; }
    public static List<string> Of(HashSet<uint> pids) {
        var rows = new List<string>();
        EnumWindows((window, data) => {
            uint pid; GetWindowThreadProcessId(window, out pid);
            if (!pids.Contains(pid) || rows.Count >= 400) return true;
            var name = new StringBuilder(128); GetClassName(window, name, 128);
            Rect r; GetWindowRect(window, out r);
            rows.Add(String.Format("pid={0} window=0x{1:x} class={2} visible={3} minimized={4} owned={5} style=0x{6:X8} exstyle=0x{7:X8} size={8}x{9}",
                pid, (long)window, name, IsWindowVisible(window) ? 1 : 0, IsIconic(window) ? 1 : 0, GetWindow(window, 4) != IntPtr.Zero ? 1 : 0,
                GetWindowLong(window, -16), GetWindowLong(window, -20), r.Right - r.Left, r.Bottom - r.Top));
            return true;
        }, IntPtr.Zero);
        return rows;
    }
}
'@
        $clients = @(Get-Process -Name Weixin, WeChat, WXWork, Feishu, DingTalk, QQ -ErrorAction SilentlyContinue)
        $info.MessengerProcesses = @($clients | ForEach-Object {
            $item = [ordered]@{ Name = $_.ProcessName; Id = $_.Id }
            try {
                $item.Path = $_.Path; $item.Version = $_.MainModule.FileVersionInfo.FileVersion; $item.Company = $_.MainModule.FileVersionInfo.CompanyName
                $item.Parent = (Get-CimInstance Win32_Process -Filter "ProcessId=$($_.Id)" -OperationTimeoutSec 5).ParentProcessId
            } catch { $item.Details = '无法读取部分进程信息' }
            [pscustomobject]$item
        })
        $pids = New-Object 'System.Collections.Generic.HashSet[uint32]'
        foreach ($client in $clients) { [void]$pids.Add([uint32]$client.Id) }
        $info.MessengerWindows = @([DaozaiDiagnosticWindows]::Of($pids))
    } catch { $info.MessengerQuery = '无法读取消息客户端窗口信息' }
    # 系统媒体会话：Windows 当前看到哪些播放器、播放状态与类型；只记「有没有」歌名，不记歌名本身。
    try {
        Add-Type -AssemblyName System.Runtime.WindowsRuntime
        $null = [Windows.Media.Control.GlobalSystemMediaTransportControlsSessionManager, Windows.Media.Control, ContentType = WindowsRuntime]
        $asTask = ([System.WindowsRuntimeSystemExtensions].GetMethods() | Where-Object { $_.Name -eq 'AsTask' -and $_.GetParameters().Count -eq 1 -and $_.GetParameters()[0].ParameterType.Name -eq 'IAsyncOperation`1' })[0]
        function Wait-WinRt($operation, [Type]$type) { $task = $asTask.MakeGenericMethod($type).Invoke($null, @($operation)); if ($task.Wait(5000)) { $task.Result } }
        $manager = Wait-WinRt ([Windows.Media.Control.GlobalSystemMediaTransportControlsSessionManager]::RequestAsync()) ([Windows.Media.Control.GlobalSystemMediaTransportControlsSessionManager])
        $info.MediaSessions = @($manager.GetSessions() | ForEach-Object {
            $properties = Wait-WinRt ($_.TryGetMediaPropertiesAsync()) ([Windows.Media.Control.GlobalSystemMediaTransportControlsSessionMediaProperties])
            $playback = $_.GetPlaybackInfo()
            $timeline = $_.GetTimelineProperties()
            [pscustomobject]@{ Source = $_.SourceAppUserModelId; Status = [string]$playback.PlaybackStatus; Type = [string]$playback.PlaybackType
                HasTitle = [bool]($properties -and $properties.Title); HasArtist = [bool]($properties -and $properties.Artist)
                HasAlbum = [bool]($properties -and $properties.AlbumTitle); HasThumbnail = [bool]($properties -and $properties.Thumbnail)
                HasDuration = [bool]($timeline -and $timeline.EndTime.Ticks -gt 0) }
        })
    } catch { $info.MediaQuery = '无法读取系统媒体会话' }
    # 正在运行的音乐播放器与版本：播放器在放歌却没出现在上面的媒体会话里，就是「播放器没有向系统提供播放信息」。
    try {
        $info.MusicPlayers = @(Get-Process -Name cloudmusic, QQMusic, KuGou, SodaMusic, kwmusic, bodian_pc, Spotify, AppleMusic -ErrorAction SilentlyContinue |
            Group-Object ProcessName | ForEach-Object {
                $first = $_.Group | Select-Object -First 1
                $item = [ordered]@{ Name = $_.Name; Processes = $_.Count }
                try { $item.Version = $first.MainModule.FileVersionInfo.FileVersion; $item.Path = $first.Path } catch { $item.Details = '无法读取版本' }
                [pscustomobject]$item
            })
    } catch { $info.MusicPlayerQuery = '无法读取音乐播放器进程' }
    Save-Text 'environment.json' ($info | ConvertTo-Json -Depth 7)
    New-Item -ItemType Directory -Force -Path $OutputDirectory | Out-Null
    $archive = Join-Path $OutputDirectory ('岛仔诊断-' + (Get-Date -Format 'yyyyMMdd-HHmmss') + '-' + [guid]::NewGuid().ToString('N').Substring(0, 6) + '.zip')
    Compress-Archive -Path (Join-Path $collectionRoot '*') -DestinationPath $archive
    Write-Output ('诊断包已保存：' + $archive)
    Write-Output '请将诊断包发给开发者，并说明异常出现时间、重启岛仔能否恢复、是否发生在睡眠唤醒后。'
    Write-Output '音乐没有歌名、封面或歌词时，请在歌曲播放中运行诊断，并说明使用的播放模式（歌单、电台、FM、漫游等）。'
    Write-Output '诊断包含岛仔日志、功能开关、系统与显卡信息、消息窗口类型和播放器状态。音乐日志可能含有歌名和歌手。诊断包由你决定是否分享。'
    if ($RevealArchive) {
        Read-Host '按回车键查看诊断包' | Out-Null
        Start-Process -FilePath (Join-Path $env:WINDIR 'explorer.exe') -ArgumentList ('/select,"' + $archive + '"')
    }
} finally {
    Get-ChildItem -LiteralPath $collectionRoot -File | ForEach-Object { Remove-Item -LiteralPath $_.FullName -Force }
    Remove-Item -LiteralPath $collectionRoot -Force
}
