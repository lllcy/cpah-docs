[CmdletBinding()]
param(
    [string]$CargoAbout = "cargo-about"
)

$ErrorActionPreference = "Stop"
$projectRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$packageJsonPath = Join-Path $projectRoot "package.json"
$cargoTomlPath = Join-Path $projectRoot "src-tauri\Cargo.toml"
$tauriConfigPath = Join-Path $projectRoot "src-tauri\tauri.conf.json"
$thirdPartyLicensesPath = Join-Path $projectRoot "THIRD_PARTY_LICENSES.md"

$packageVersion = (Get-Content -LiteralPath $packageJsonPath -Raw -Encoding utf8 | ConvertFrom-Json).version
$tauriVersion = (Get-Content -LiteralPath $tauriConfigPath -Raw -Encoding utf8 | ConvertFrom-Json).version
$cargoContent = Get-Content -LiteralPath $cargoTomlPath -Raw -Encoding utf8
$cargoMatch = [regex]::Match($cargoContent, '(?ms)^\[package\].*?^version\s*=\s*"([^"]+)"')
if (-not $cargoMatch.Success) {
    throw "Cannot read package.version from src-tauri/Cargo.toml"
}
$cargoVersion = $cargoMatch.Groups[1].Value

if (($packageVersion -ne $cargoVersion) -or ($packageVersion -ne $tauriVersion)) {
    throw "Version mismatch: package.json=$packageVersion, Cargo.toml=$cargoVersion, tauri.conf.json=$tauriVersion"
}

Push-Location $projectRoot
try {
    $licenseHashBefore = if (Test-Path -LiteralPath $thirdPartyLicensesPath -PathType Leaf) {
        (Get-FileHash -LiteralPath $thirdPartyLicensesPath -Algorithm SHA256).Hash
    } else {
        $null
    }
    & (Join-Path $PSScriptRoot "generate-third-party-licenses.ps1") -CargoAbout $CargoAbout
    $licenseHashAfter = (Get-FileHash -LiteralPath $thirdPartyLicensesPath -Algorithm SHA256).Hash
    if (-not $licenseHashBefore -or $licenseHashBefore -ne $licenseHashAfter) {
        throw "THIRD_PARTY_LICENSES.md was refreshed. Review and commit it, then run the release again."
    }

    cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
    if ($LASTEXITCODE -ne 0) { throw "Rust formatting check failed" }

    npm.cmd run build
    if ($LASTEXITCODE -ne 0) { throw "Frontend production build failed" }

    cargo test --manifest-path src-tauri/Cargo.toml --all-targets
    if ($LASTEXITCODE -ne 0) { throw "Rust tests failed" }

    cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
    if ($LASTEXITCODE -ne 0) { throw "Clippy checks failed" }

    npx.cmd tauri build --no-bundle
    if ($LASTEXITCODE -ne 0) { throw "Tauri release build failed" }

    $sourceExe = Join-Path $projectRoot "src-tauri\target\release\cpah-docs.exe"
    if (-not (Test-Path -LiteralPath $sourceExe -PathType Leaf)) {
        throw "Release EXE not found: $sourceExe"
    }

    $artifactDirectory = Join-Path $projectRoot "release"
    New-Item -ItemType Directory -Path $artifactDirectory -Force | Out-Null
    $artifactName = "CPAH-Docs-v$packageVersion-windows-x64.exe"
    $artifactPath = Join-Path $artifactDirectory $artifactName
    Copy-Item -LiteralPath $sourceExe -Destination $artifactPath -Force
    Copy-Item -LiteralPath (Join-Path $projectRoot "LICENSE") -Destination (Join-Path $artifactDirectory "LICENSE.txt") -Force
    Copy-Item -LiteralPath $thirdPartyLicensesPath -Destination (Join-Path $artifactDirectory "THIRD_PARTY_LICENSES.md") -Force

    $stream = [System.IO.File]::OpenRead($artifactPath)
    try {
        $sha256 = [System.Security.Cryptography.SHA256]::Create()
        try {
            $hashBytes = $sha256.ComputeHash($stream)
        }
        finally {
            $sha256.Dispose()
        }
    }
    finally {
        $stream.Dispose()
    }
    $hash = -join ($hashBytes | ForEach-Object { $_.ToString("x2") })
    Set-Content -LiteralPath (Join-Path $artifactDirectory "SHA256SUMS.txt") -Value "$hash  $artifactName" -Encoding ascii
    Write-Host "Release artifact: $artifactPath"
    Write-Host "SHA-256: $hash"
}
finally {
    Pop-Location
}
