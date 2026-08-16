[CmdletBinding()]
param(
    [string]$CargoAbout = "cargo-about"
)

$ErrorActionPreference = "Stop"
$projectRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path

Push-Location $projectRoot
try {
    & node (Join-Path $PSScriptRoot "generate-third-party-licenses.mjs") --cargo-about $CargoAbout
    if ($LASTEXITCODE -ne 0) {
        throw "Third-party license generation failed. Install cargo-about 0.9.1 or pass -CargoAbout with its executable path."
    }
}
finally {
    Pop-Location
}
