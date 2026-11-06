$ErrorActionPreference = 'Stop'

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$cargoCommand = Get-Command cargo -ErrorAction SilentlyContinue
$cargo = if ($cargoCommand) { $cargoCommand.Source } else {
    Join-Path $env:USERPROFILE '.cargo\bin\cargo.exe'
}
if (-not (Test-Path -LiteralPath $cargo)) { throw "Cargo not found: $cargo" }

$ffmpeg = $env:VIETKARA_FFMPEG
if (-not $ffmpeg) {
    $ffmpegCommand = Get-Command ffmpeg -ErrorAction SilentlyContinue
    if ($ffmpegCommand) { $ffmpeg = $ffmpegCommand.Source }
}
if (-not $ffmpeg -or -not (Test-Path -LiteralPath $ffmpeg)) {
    throw 'Set VIETKARA_FFMPEG to an FFmpeg build with libass enabled.'
}

$tempRoot = [System.IO.Path]::GetFullPath((Join-Path $env:TEMP ('vietkara-ass-parity-' + [guid]::NewGuid().ToString('N'))))
$tempBase = [System.IO.Path]::GetFullPath($env:TEMP).TrimEnd('\') + '\'
if (-not $tempRoot.StartsWith($tempBase, [System.StringComparison]::OrdinalIgnoreCase)) {
    throw 'Test directory resolved outside TEMP.'
}
New-Item -ItemType Directory -Path $tempRoot | Out-Null

$previousDump = [Environment]::GetEnvironmentVariable('VIETKARA_ASS_GOLDEN_DUMP', 'Process')
$previousLayoutDump = [Environment]::GetEnvironmentVariable('VIETKARA_ASS_LAYOUT_DUMP', 'Process')
try {
    $baseline = Join-Path $repoRoot 'crates\vietkara-render\tests\fixtures\python-c8dae55-simple-karaoke.ass'
    $rustAss = Join-Path $tempRoot 'rust.ass'
    [Environment]::SetEnvironmentVariable('VIETKARA_ASS_GOLDEN_DUMP', $rustAss, 'Process')

    Push-Location $repoRoot
    try {
        & $cargo test -p vietkara-render --test ass_python_golden -- --exact `
            simple_karaoke_ass_matches_python_baseline_c8dae55
        if ($LASTEXITCODE -ne 0) { throw "Rust ASS fixture generation failed: $LASTEXITCODE" }
    }
    finally { Pop-Location }

    if (-not (Test-Path -LiteralPath $rustAss)) { throw 'Rust ASS output was not generated.' }
    Copy-Item -LiteralPath $baseline -Destination (Join-Path $tempRoot 'baseline.ass')
    $expectedBytes = 1920 * 1080 * 3

    Push-Location $tempRoot
    try {
        foreach ($frame in 0..5) {
            foreach ($kind in @('baseline', 'rust')) {
                $filter = "subtitles=$kind.ass,select=eq(n\,$frame)"
                $raw = "$kind-$frame.rgb"
                & $ffmpeg -y -hide_banner -loglevel error `
                    -f lavfi -i 'color=c=0x101d2b:s=1920x1080:r=2:d=3' `
                    -vf $filter -fps_mode passthrough -frames:v 1 `
                    -pix_fmt rgb24 -f rawvideo $raw
                if ($LASTEXITCODE -ne 0) { throw "FFmpeg render failed: $kind frame $frame" }
                if ((Get-Item -LiteralPath $raw).Length -ne $expectedBytes) {
                    throw "Unexpected raw frame size: $kind frame $frame"
                }
            }
            $baselineHash = (Get-FileHash -Algorithm SHA256 "baseline-$frame.rgb").Hash
            $rustHash = (Get-FileHash -Algorithm SHA256 "rust-$frame.rgb").Hash
            if ($baselineHash -ne $rustHash) {
                throw "ASS raster differs from Python baseline at frame $frame (t=$($frame / 2)s)."
            }
        }
    }
    finally { Pop-Location }

    $layoutBaseline = Join-Path $repoRoot 'crates\vietkara-render\tests\fixtures\python-c8dae55-layout-fixed-width.ass'
    $layoutRust = Join-Path $tempRoot 'layout-rust.ass'
    [Environment]::SetEnvironmentVariable('VIETKARA_ASS_LAYOUT_DUMP', $layoutRust, 'Process')
    Push-Location $repoRoot
    try {
        & $cargo test -p vietkara-render --test ass_layout_python_golden -- --exact `
            complete_ass_events_match_python_layout_baseline_with_fixed_width_metrics
        if ($LASTEXITCODE -ne 0) { throw "Rust layout ASS generation failed: $LASTEXITCODE" }
    }
    finally { Pop-Location }
    if (-not (Test-Path -LiteralPath $layoutRust)) { throw 'Rust layout ASS output was not generated.' }
    Copy-Item -LiteralPath $layoutBaseline -Destination (Join-Path $tempRoot 'layout-baseline.ass')

    Push-Location $tempRoot
    try {
        foreach ($frame in 0..5) {
            foreach ($kind in @('layout-baseline', 'layout-rust')) {
                $filter = "subtitles=$kind.ass,select=eq(n\,$frame)"
                $raw = "$kind-$frame.rgb"
                & $ffmpeg -y -hide_banner -loglevel error `
                    -f lavfi -i 'color=c=0x101d2b:s=1920x1080:r=2:d=3' `
                    -vf $filter -fps_mode passthrough -frames:v 1 `
                    -pix_fmt rgb24 -f rawvideo $raw
                if ($LASTEXITCODE -ne 0) { throw "FFmpeg layout render failed: $kind frame $frame" }
                if ((Get-Item -LiteralPath $raw).Length -ne $expectedBytes) {
                    throw "Unexpected layout frame size: $kind frame $frame"
                }
            }
            $baselineHash = (Get-FileHash -Algorithm SHA256 "layout-baseline-$frame.rgb").Hash
            $rustHash = (Get-FileHash -Algorithm SHA256 "layout-rust-$frame.rgb").Hash
            if ($baselineHash -ne $rustHash) {
                throw "Full-layout ASS raster differs from Python baseline at frame $frame (t=$($frame / 2)s)."
            }
        }
    }
    finally { Pop-Location }

    Write-Output 'ASS raster parity passed: simple serializer and full fixed-metric layout each match six Python baseline frames.'
}
finally {
    [Environment]::SetEnvironmentVariable('VIETKARA_ASS_GOLDEN_DUMP', $previousDump, 'Process')
    [Environment]::SetEnvironmentVariable('VIETKARA_ASS_LAYOUT_DUMP', $previousLayoutDump, 'Process')
    Remove-Item -LiteralPath $tempRoot -Recurse -Force -ErrorAction SilentlyContinue
}
