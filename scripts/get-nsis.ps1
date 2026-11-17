param([Parameter(Mandatory=$true)][string]$Directory)
$ErrorActionPreference = 'Stop'
$archive = Join-Path $Directory 'nsis-3.13.zip'
Invoke-WebRequest 'https://downloads.sourceforge.net/project/nsis/NSIS%203/3.13/nsis-3.13.zip' -OutFile $archive
if ((Get-FileHash $archive -Algorithm SHA256).Hash -ne 'ba63dffc4410ee89193e1cb5a41989991bd77c61068da17e3156d136b7b0b3d8') {
  # SourceForge 有时先返回下载页，再通过页面跳转提供压缩包。
  $redirect = [regex]::Match((Get-Content $archive -Raw), 'content="5; url=([^"]+)"')
  if (-not $redirect.Success) { throw 'NSIS 下载响应异常' }
  $url = [Net.WebUtility]::HtmlDecode($redirect.Groups[1].Value)
  $uri = [uri]$url
  if ($uri.Scheme -ne 'https' -or $uri.Host -ne 'downloads.sourceforge.net' -or $uri.AbsolutePath -ne '/project/nsis/NSIS%203/3.13/nsis-3.13.zip') { throw 'NSIS 下载跳转地址异常' }
  Invoke-WebRequest $url -OutFile $archive
}
if ((Get-FileHash $archive -Algorithm SHA256).Hash -ne 'ba63dffc4410ee89193e1cb5a41989991bd77c61068da17e3156d136b7b0b3d8') { throw 'NSIS 校验失败' }
Expand-Archive -LiteralPath $archive -DestinationPath $Directory
$compiler = Join-Path $Directory 'nsis-3.13\makensis.exe'
Write-Output $compiler
