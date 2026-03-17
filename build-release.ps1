$ErrorActionPreference = "Stop"

$ProjectRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
$IconIco = Join-Path $ProjectRoot "icon.ico"
$IconRc = Join-Path $ProjectRoot "icon.rc"
$IconRes = Join-Path $ProjectRoot "icon.res"
$BuildTemp = Join-Path $ProjectRoot ".tmp-build"

New-Item -ItemType Directory -Force $BuildTemp | Out-Null
$env:TEMP = $BuildTemp
$env:TMP = $BuildTemp

if (-not (Test-Path $IconIco)) {
    throw "Missing icon file: $IconIco"
}

if (-not (Test-Path $IconRc)) {
    @'
1 ICON "icon.ico"
'@ | Set-Content -Encoding ASCII $IconRc
}

function Get-RcExe {
    $sdkRoot = "C:\Program Files (x86)\Windows Kits\10\bin"
    if (-not (Test-Path $sdkRoot)) {
        return $null
    }

    $versions = Get-ChildItem $sdkRoot -Directory | Sort-Object Name -Descending
    foreach ($version in $versions) {
        foreach ($arch in @("x64", "x86")) {
            $candidate = Join-Path $version.FullName $arch
            $candidate = Join-Path $candidate "rc.exe"
            if (Test-Path $candidate) {
                return $candidate
            }
        }
    }

    return $null
}

$RcExe = Get-RcExe
if (-not $RcExe) {
    throw "Could not find rc.exe in the Windows SDK."
}

& $RcExe "/fo$IconRes" $IconRc
if ($LASTEXITCODE -ne 0 -or -not (Test-Path $IconRes)) {
    throw "Failed to compile icon resource."
}

Push-Location $ProjectRoot
try {
    cargo rustc --release --bin live-wall -- "-Clink-arg=$IconRes"
}
finally {
    Pop-Location
}
