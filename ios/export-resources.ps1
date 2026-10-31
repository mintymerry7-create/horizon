# Zips the game files the Horizon launcher prepared on this Windows PC, for the iOS app (docs/ios.md):
# put horizon-resources.zip in Files > On My iPhone > Horizon and tap it there to unzip it.
#   powershell -ExecutionPolicy Bypass -File ios\export-resources.ps1 [-Out <zip>]
param([string]$Out = "$HOME\Desktop\horizon-resources.zip")
$ErrorActionPreference = 'Stop'
$root = if ($env:HORIZON_LAUNCHER_HOME) { $env:HORIZON_LAUNCHER_HOME } else { Join-Path $env:APPDATA 'horizon-launcher' }
$manifest = Get-ChildItem -Path (Join-Path $root 'versions') -Recurse -Force -Filter 'horizon-resources.json' -ErrorAction SilentlyContinue |
    Where-Object { $_.DirectoryName -match '\.local\horizon-client\' } |
    Sort-Object LastWriteTime -Descending | Select-Object -First 1
if (-not $manifest) {
    Write-Error "No prepared game files under $root\versions: press Play once in the Horizon launcher first."
}
$dir = $manifest.Directory
Write-Host "Zipping $($dir.FullName) ..."
if (Test-Path $Out) { Remove-Item $Out }
# Windows' tar writes zips (-a picks the format from the name) far faster than Compress-Archive.
tar -a -c -f $Out -C $dir.Parent.FullName $dir.Name
if ($LASTEXITCODE -ne 0) { Write-Error "tar failed ($LASTEXITCODE)" }
Write-Host "Done: $Out ($([math]::Round((Get-Item $Out).Length / 1MB)) MB)"
