$ErrorActionPreference = "Stop"

$ProjectRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
$CargoToml = Join-Path $ProjectRoot "Cargo.toml"

if (-not (Test-Path $CargoToml)) {
    throw "Missing Cargo.toml: $CargoToml"
}

$VersionMatch = Select-String -Path $CargoToml -Pattern '^version\s*=\s*"([^"]+)"$' | Select-Object -First 1
if (-not $VersionMatch) {
    throw "Could not determine package version from Cargo.toml."
}

$Version = $VersionMatch.Matches[0].Groups[1].Value
$PackageName = "Live Wall v$Version"
$DistRoot = Join-Path $ProjectRoot "dist"
$PackageRoot = Join-Path $DistRoot $PackageName
$ZipPath = Join-Path $DistRoot "$PackageName-windows-x64.zip"
$BuildScript = Join-Path $ProjectRoot "build-release.ps1"
$ReleaseExe = Join-Path $ProjectRoot "target\release\live-wall.exe"
$WallpaperHtml = Join-Path $ProjectRoot "app\assets\wallpaper.html"
$PlaylistSource = Join-Path $ProjectRoot "PLAYLIST"
$MpvRuntime = Join-Path $ProjectRoot "mpv-x86_64-v3-20260307-git-f9190e5"

if (-not (Test-Path $BuildScript)) {
    throw "Missing build script: $BuildScript"
}

if (-not (Test-Path $WallpaperHtml)) {
    throw "Missing wallpaper asset: $WallpaperHtml"
}

if (-not (Test-Path $MpvRuntime)) {
    throw "Missing mpv runtime folder: $MpvRuntime"
}

& $BuildScript

if (-not (Test-Path $ReleaseExe)) {
    throw "Release executable was not produced: $ReleaseExe"
}

New-Item -ItemType Directory -Force $DistRoot | Out-Null
if (Test-Path $PackageRoot) {
    Remove-Item -Recurse -Force $PackageRoot
}
New-Item -ItemType Directory -Force $PackageRoot | Out-Null

Copy-Item $ReleaseExe (Join-Path $PackageRoot "live-wall.exe")
Copy-Item (Join-Path $ProjectRoot "tray_icon.ico") $PackageRoot
Copy-Item (Join-Path $ProjectRoot "icon.ico") $PackageRoot
Copy-Item $WallpaperHtml (Join-Path $PackageRoot "wallpaper.html")

$PlaylistTarget = Join-Path $PackageRoot "PLAYLIST"
New-Item -ItemType Directory -Force $PlaylistTarget | Out-Null
if (Test-Path $PlaylistSource) {
    Copy-Item (Join-Path $PlaylistSource "*") $PlaylistTarget -Recurse -Force
}

Copy-Item $MpvRuntime $PackageRoot -Recurse -Force

if (Test-Path $ZipPath) {
    Remove-Item -Force $ZipPath
}
Compress-Archive -Path (Join-Path $PackageRoot "*") -DestinationPath $ZipPath

Write-Host "Created package:"
Write-Host "  $PackageRoot"
Write-Host "Created zip:"
Write-Host "  $ZipPath"
