param(
    [Parameter(Mandatory = $true)]
    [string]$Root,
    [Parameter(Mandatory = $true)]
    [string]$BackupRoot
)

$ErrorActionPreference = "Stop"
Add-Type @"
using System;
using System.Runtime.InteropServices;
public static class CpahNativeFile {
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    public static extern bool MoveFileEx(string existingName, string newName, uint flags);
}
"@
$resolvedRoot = [IO.Path]::GetFullPath((Resolve-Path -LiteralPath $Root).Path).TrimEnd("\")
$resolvedBackup = [IO.Path]::GetFullPath($BackupRoot).TrimEnd("\")
if ($resolvedBackup.StartsWith($resolvedRoot + "\", [StringComparison]::OrdinalIgnoreCase)) {
    throw "Backup directory must be outside the cleanup root"
}
if (Test-Path -LiteralPath $resolvedBackup) {
    throw "Backup directory already exists: $resolvedBackup"
}

$targets = @(rg -l "^(cpah_tags|cpah_categories)\s*:" -g "*.md" -- $resolvedRoot)
if ($targets.Count -eq 0) {
    [pscustomobject]@{
        Changed = 0
        Backup = "not-created"
        RemainingManagedFields = 0
    }
    exit 0
}

New-Item -ItemType Directory -Path $resolvedBackup | Out-Null
$utf8Strict = New-Object Text.UTF8Encoding($false, $true)
$utf8Bom = New-Object Text.UTF8Encoding($true)
$utf8NoBom = New-Object Text.UTF8Encoding($false)
$openingRegex = New-Object Text.RegularExpressions.Regex("\A---(?<nl>\r?\n)")
$closingRegex = New-Object Text.RegularExpressions.Regex("(?m)^(---|\.\.\.)\r?$")
$changed = 0

foreach ($path in $targets) {
    $fullPath = [IO.Path]::GetFullPath($path)
    if (-not $fullPath.StartsWith($resolvedRoot + "\", [StringComparison]::OrdinalIgnoreCase)) {
        throw "File escaped cleanup root: $fullPath"
    }

    $relativePath = $fullPath.Substring($resolvedRoot.Length).TrimStart("\")
    $backupPath = Join-Path $resolvedBackup $relativePath
    New-Item -ItemType Directory -Force -Path (Split-Path -Parent $backupPath) | Out-Null
    Copy-Item -LiteralPath $fullPath -Destination $backupPath

    $bytes = [IO.File]::ReadAllBytes($fullPath)
    $hasBom = $bytes.Length -ge 3 -and $bytes[0] -eq 0xEF -and $bytes[1] -eq 0xBB -and $bytes[2] -eq 0xBF
    $offset = if ($hasBom) { 3 } else { 0 }
    $text = $utf8Strict.GetString($bytes, $offset, $bytes.Length - $offset)
    $opening = $openingRegex.Match($text)
    if (-not $opening.Success) {
        continue
    }
    $closing = $closingRegex.Match($text, $opening.Length)
    if (-not $closing.Success) {
        continue
    }

    $newline = $opening.Groups["nl"].Value
    $yaml = $text.Substring($opening.Length, $closing.Index - $opening.Length)
    $lines = [Text.RegularExpressions.Regex]::Split($yaml, "\r?\n")
    $kept = New-Object Collections.Generic.List[string]
    $removing = $false
    foreach ($line in $lines) {
        if ($line -match "^cpah_(tags|categories)\s*:") {
            $removing = $true
            continue
        }
        if ($removing) {
            if ($line -match "^[ \t]+" -or $line -eq "") {
                continue
            }
            $removing = $false
        }
        $kept.Add($line)
    }

    $cleanYaml = [string]::Join($newline, $kept)
    if ($cleanYaml.Length -gt 0 -and -not $cleanYaml.EndsWith($newline)) {
        $cleanYaml += $newline
    }
    $updated = $text.Substring(0, $opening.Length) + $cleanYaml + $text.Substring($closing.Index)
    if ($updated -eq $text) {
        continue
    }

    $temporary = Join-Path (Split-Path -Parent $fullPath) ("." + [IO.Path]::GetFileName($fullPath) + "." + [guid]::NewGuid().ToString("N") + ".cleanup.tmp")
    try {
        $encoding = if ($hasBom) { $utf8Bom } else { $utf8NoBom }
        [IO.File]::WriteAllText($temporary, $updated, $encoding)
        if (-not [CpahNativeFile]::MoveFileEx($temporary, $fullPath, 9)) {
            $errorCode = [Runtime.InteropServices.Marshal]::GetLastWin32Error()
            throw "Atomic replace failed with Win32 error $errorCode"
        }
        $changed++
    }
    finally {
        if (Test-Path -LiteralPath $temporary) {
            Remove-Item -LiteralPath $temporary -Force
        }
    }
}

$remaining = @(rg -l "^(cpah_tags|cpah_categories)\s*:" -g "*.md" -- $resolvedRoot).Count
$standardTags = @(rg -l "^tags\s*:" -g "*.md" -- $resolvedRoot).Count
[pscustomobject]@{
    Changed = $changed
    Backup = $resolvedBackup
    RemainingManagedFields = $remaining
    PreservedStandardTags = $standardTags
}
