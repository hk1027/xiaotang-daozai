param([int]$Seconds=5)
$ErrorActionPreference='Stop'
Add-Type -AssemblyName System.Runtime.WindowsRuntime
$awaitMethod=[System.WindowsRuntimeSystemExtensions].GetMethods() | Where-Object { $_.Name -eq 'AsTask' -and $_.IsGenericMethod -and $_.GetGenericArguments().Count -eq 1 -and $_.GetParameters().Count -eq 1 -and $_.GetParameters()[0].ParameterType.Name -eq 'IAsyncOperation`1' } | Select-Object -First 1
function Await-Result($Operation,[Type]$ResultType){
    $task=$awaitMethod.MakeGenericMethod($ResultType).Invoke($null,@($Operation))
    $task.Wait()
    $task.Result
}
$player=[Windows.Media.Playback.MediaPlayer,Windows.Media.Playback,ContentType=WindowsRuntime]::new()
try {
    $player.CommandManager.IsEnabled=$false
    $controls=$player.SystemMediaTransportControls
    $controls.IsEnabled=$true
    $controls.IsPlayEnabled=$true
    $controls.IsPauseEnabled=$true
    $controls.PlaybackStatus=[Windows.Media.MediaPlaybackStatus,Windows.Media,ContentType=WindowsRuntime]::Playing
    $display=$controls.DisplayUpdater
    $display.Type=[Windows.Media.MediaPlaybackType,Windows.Media,ContentType=WindowsRuntime]::Music
    $display.MusicProperties.Title='Daozai memory fixture 0'
    $display.MusicProperties.Artist='Memory fixture'
    $file=Await-Result ([Windows.Storage.StorageFile,Windows.Storage,ContentType=WindowsRuntime]::GetFileFromPathAsync((Join-Path (Split-Path $PSScriptRoot -Parent) 'assets\daozai-logo.png'))) ([Windows.Storage.StorageFile])
    $display.Thumbnail=[Windows.Storage.Streams.RandomAccessStreamReference,Windows.Storage.Streams,ContentType=WindowsRuntime]::CreateFromFile($file)
    $display.Update()
    $manager=Await-Result ([Windows.Media.Control.GlobalSystemMediaTransportControlsSessionManager,Windows.Media.Control,ContentType=WindowsRuntime]::RequestAsync()) ([Windows.Media.Control.GlobalSystemMediaTransportControlsSessionManager])
    foreach($session in $manager.GetSessions()){Write-Output ('session='+$session.SourceAppUserModelId)}
    $started=[System.Diagnostics.Stopwatch]::StartNew()
    $track=-1
    while($started.Elapsed.TotalSeconds -lt $Seconds){
        $next=[int][math]::Floor($started.Elapsed.TotalSeconds/2)
        if($next -ne $track){$track=$next;$display.MusicProperties.Title='Daozai memory fixture '+$track;$display.Update()}
        Start-Sleep -Milliseconds 100
    }
} finally {$player.Dispose()}
