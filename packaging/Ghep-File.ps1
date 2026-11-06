$ErrorActionPreference = 'Stop'

function Get-Sha256([string]$Path) {
    $stream = [IO.File]::OpenRead($Path)
    $hasher = [Security.Cryptography.SHA256]::Create()
    try {
        return ([BitConverter]::ToString($hasher.ComputeHash($stream))).Replace('-', '').ToLowerInvariant()
    } finally {
        $stream.Dispose()
        $hasher.Dispose()
    }
}

function Assert-LeafName([string]$Name) {
    if ([string]::IsNullOrWhiteSpace($Name) -or $Name -ne [IO.Path]::GetFileName($Name) -or
        $Name.Contains('/') -or $Name.Contains('\') -or $Name -eq '.' -or $Name -eq '..') {
        throw "Asset name is invalid in downloads.json: '$Name'"
    }
}

$activeTemp = $null
try {
    $manifestPath = Join-Path $PSScriptRoot 'downloads.json'
    $manifest = @(Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json)
    if ($manifest.Count -eq 0) { throw 'downloads.json has no artifacts.' }

    $completed = 0
    foreach ($item in $manifest) {
        Assert-LeafName ([string]$item.name)
        $parts = @($item.parts)
        if ($parts.Count -eq 0) { throw "No parts listed for $($item.name)." }
        if ([long]$item.size -lt 1 -or [string]$item.sha256 -notmatch '^[0-9a-fA-F]{64}$') {
            throw "Invalid artifact metadata: $($item.name)."
        }

        $present = 0
        foreach ($part in $parts) {
            Assert-LeafName ([string]$part.name)
            if ([long]$part.size -lt 1 -or [string]$part.sha256 -notmatch '^[0-9a-fA-F]{64}$') {
                throw "Invalid part metadata: $($part.name)."
            }
            if (Test-Path -LiteralPath (Join-Path $PSScriptRoot $part.name) -PathType Leaf) { $present++ }
        }
        if ($present -eq 0) { continue }
        if ($present -ne $parts.Count) { throw "A downloaded part is missing for $($item.name)." }

        $sumSize = [long]0
        foreach ($part in $parts) {
            $path = Join-Path $PSScriptRoot $part.name
            $actualSize = (Get-Item -LiteralPath $path).Length
            if ($actualSize -ne [long]$part.size -or (Get-Sha256 $path) -ne ([string]$part.sha256).ToLowerInvariant()) {
                throw "Part failed size or SHA-256 verification: $($part.name)."
            }
            $sumSize += $actualSize
        }
        if ($sumSize -ne [long]$item.size) { throw "Part sizes do not match artifact size: $($item.name)." }

        $target = Join-Path $PSScriptRoot $item.name
        if (Test-Path -LiteralPath $target) {
            if ((Get-Item -LiteralPath $target).Length -eq [long]$item.size -and
                (Get-Sha256 $target) -eq ([string]$item.sha256).ToLowerInvariant()) {
                Write-Host "Verified file already exists: $target"
                $completed++
                continue
            }
            throw "Destination exists with different content: $target. Rename the existing file first."
        }

        $temp = "$target.partial"
        $output = $null
        try {
            $output = [IO.File]::Open($temp, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
            $activeTemp = $temp
            foreach ($part in $parts) {
                $inputFile = [IO.File]::OpenRead((Join-Path $PSScriptRoot $part.name))
                try { $inputFile.CopyTo($output) } finally { $inputFile.Dispose() }
            }
        } finally {
            if ($null -ne $output) { $output.Dispose() }
        }

        if ((Get-Item -LiteralPath $temp).Length -ne [long]$item.size -or
            (Get-Sha256 $temp) -ne ([string]$item.sha256).ToLowerInvariant()) {
            Remove-Item -LiteralPath $temp -Force
            $activeTemp = $null
            throw "Assembled file failed size or SHA-256 verification: $($item.name)."
        }
        [IO.File]::Move($temp, $target)
        $activeTemp = $null
        Write-Host "Assembled and verified: $target" -ForegroundColor Green
        $completed++
    }

    if ($completed -eq 0) { throw 'No Setup or Portable parts were found in this folder.' }
} catch {
    if ($activeTemp -and (Test-Path -LiteralPath $activeTemp)) {
        Remove-Item -LiteralPath $activeTemp -Force -ErrorAction SilentlyContinue
    }
    Write-Host $_ -ForegroundColor Red
    exit 1
}
