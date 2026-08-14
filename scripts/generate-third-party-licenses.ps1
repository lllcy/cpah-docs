[CmdletBinding()]
param(
    [string]$CargoAbout = "cargo-about"
)

$ErrorActionPreference = "Stop"
$projectRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$rustOutput = Join-Path $env:TEMP ("cpah-docs-rust-licenses-" + [guid]::NewGuid().ToString("N") + ".md")

Push-Location $projectRoot
try {
    & $CargoAbout generate `
        --manifest-path "src-tauri/Cargo.toml" `
        --config "about.toml" `
        --locked `
        --fail `
        --output-file $rustOutput `
        "about.hbs"
    if ($LASTEXITCODE -ne 0) {
        throw "cargo-about failed. Install cargo-about 0.9.1 or pass -CargoAbout with its executable path."
    }

    $npmJson = npm.cmd query ".prod" | Out-String
    if ($LASTEXITCODE -ne 0) { throw "npm dependency query failed" }
    $packages = $npmJson | ConvertFrom-Json | Where-Object { $_.location }

    $npmLines = [System.Collections.Generic.List[string]]::new()
    $npmLines.Add("## npm 运行时依赖")
    $npmLines.Add("")
    $npmLines.Add("依赖名称和许可证来自锁定的 npm 依赖树；相同的许可证正文只收录一次。")
    $npmLines.Add("")
    $licenseTexts = @{}
    foreach ($package in ($packages | Sort-Object name, version)) {
        $license = if ($package.license) { [string]$package.license } else { "未声明" }
        $npmLines.Add("- ``$($package.name)@$($package.version)`` — $license")

        $licenseFile = Get-ChildItem -LiteralPath $package.path -File -ErrorAction SilentlyContinue |
            Where-Object { $_.Name -match '^(LICENSE|LICENCE|COPYING|NOTICE)(\..*)?$' } |
            Sort-Object Name |
            Select-Object -First 1
        if ($licenseFile) {
            $text = (Get-Content -LiteralPath $licenseFile.FullName -Raw -Encoding utf8).Trim()
            if ($text) {
                $hashBytes = [System.Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes($text))
                $hash = [Convert]::ToHexString($hashBytes).ToLowerInvariant()
                if (-not $licenseTexts.ContainsKey($hash)) {
                    $licenseTexts[$hash] = [ordered]@{ Text = $text; Packages = [System.Collections.Generic.List[string]]::new() }
                }
                $licenseTexts[$hash].Packages.Add("$($package.name)@$($package.version)")
            }
        }
    }

    foreach ($entry in ($licenseTexts.GetEnumerator() | Sort-Object { $_.Value.Packages[0] })) {
        $names = $entry.Value.Packages -join ", "
        $escaped = [System.Net.WebUtility]::HtmlEncode($entry.Value.Text)
        $npmLines.Add("")
        $npmLines.Add("<details>")
        $npmLines.Add("<summary>$([System.Net.WebUtility]::HtmlEncode($names))</summary>")
        $npmLines.Add("")
        $npmLines.Add("<pre>$escaped</pre>")
        $npmLines.Add("</details>")
    }

    $header = @(
        "# 第三方软件许可证",
        "",
        "CPAH Docs 自身采用 MIT 许可证。下列组件由各自作者提供，并继续受其许可证约束。",
        "",
        "> 此文件由 ``scripts/generate-third-party-licenses.ps1`` 生成，请勿手工编辑。",
        ""
    )
    $content = ($header -join "`n") + "`n" + (Get-Content -LiteralPath $rustOutput -Raw -Encoding utf8) + "`n" + ($npmLines -join "`n")
    $content = [regex]::Replace($content, "[ `t]+(?=`r?$)", "", [Text.RegularExpressions.RegexOptions]::Multiline).TrimEnd() + "`n"
    [IO.File]::WriteAllText(
        (Join-Path $projectRoot "THIRD_PARTY_LICENSES.md"),
        $content,
        [Text.UTF8Encoding]::new($false)
    )
}
finally {
    Pop-Location
    Remove-Item -LiteralPath $rustOutput -Force -ErrorAction SilentlyContinue
}
