"""Prepare GitHub release assets below the 2 GiB per-file limit."""
import hashlib
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
DEST = ROOT / 'releases' / 'github'
CHUNK = 1800 * 1024 * 1024


def main():
    DEST.mkdir(parents=True, exist_ok=True)
    manifest = []
    for name in ('VietKaraStudio-1.0.0-Setup-x64.exe', 'VietKaraStudio-1.0.0-Portable-x64.zip'):
        source = ROOT / 'releases' / name
        whole = hashlib.sha256()
        parts = []
        with source.open('rb') as stream:
            index = 1
            while data := stream.read(CHUNK):
                part = DEST / f'{name}.{index:03}'
                part.write_bytes(data)
                whole.update(data)
                parts.append({'name': part.name, 'size': len(data), 'sha256': hashlib.sha256(data).hexdigest()})
                print(part.name, len(data), flush=True)
                index += 1
        manifest.append({'name': name, 'size': source.stat().st_size, 'sha256': whole.hexdigest(), 'parts': parts})
    (DEST / 'downloads.json').write_text(json.dumps(manifest, indent=2), encoding='utf-8')
    (DEST / 'SHA256SUMS.txt').write_text(''.join(f"{item['sha256']}  {item['name']}\n" for item in manifest), encoding='utf-8')
    (DEST / 'Ghep-File.ps1').write_text(r'''$ErrorActionPreference = 'Stop'
function Get-Sha256([string]$Path) {
    $stream = [IO.File]::OpenRead($Path)
    $hasher = [Security.Cryptography.SHA256]::Create()
    try { return ([BitConverter]::ToString($hasher.ComputeHash($stream))).Replace('-', '').ToLowerInvariant() }
    finally { $stream.Dispose(); $hasher.Dispose() }
}
try {
    $manifest = Get-Content -LiteralPath (Join-Path $PSScriptRoot 'downloads.json') -Raw | ConvertFrom-Json
    $completed = 0
    foreach ($item in $manifest) {
        $present = @($item.parts | Where-Object { Test-Path -LiteralPath (Join-Path $PSScriptRoot $_.name) })
        if ($present.Count -eq 0) { continue }
        if ($present.Count -ne $item.parts.Count) { throw "Thieu phan tai ve: $($item.name)" }
        foreach ($part in $item.parts) {
            $path = Join-Path $PSScriptRoot $part.name
            if ((Get-Item -LiteralPath $path).Length -ne $part.size -or (Get-Sha256 $path) -ne $part.sha256) { throw "File bi loi: $($part.name)" }
        }
        $target = Join-Path $PSScriptRoot $item.name
        if (Test-Path -LiteralPath $target) {
            if ((Get-Sha256 $target) -eq $item.sha256) { Write-Host "Da co file dung: $target"; $completed++; continue }
            throw "File dich da ton tai nhung khac noi dung: $target. Hay doi ten file cu."
        }
        $temp = "$target.partial"
        $output = [IO.File]::Open($temp, [IO.FileMode]::CreateNew)
        try {
            foreach ($part in $item.parts) {
                $inputFile = [IO.File]::OpenRead((Join-Path $PSScriptRoot $part.name))
                try { $inputFile.CopyTo($output) } finally { $inputFile.Dispose() }
            }
        } finally { $output.Dispose() }
        if ((Get-Sha256 $temp) -ne $item.sha256) { throw "SHA256 khong khop: $temp" }
        Move-Item -LiteralPath $temp -Destination $target
        Write-Host "Da ghep va kiem tra: $target" -ForegroundColor Green
        $completed++
    }
    if ($completed -eq 0) { throw 'Chua co cac phan .001 va .002 trong thu muc nay.' }
} catch { Write-Host $_ -ForegroundColor Red; exit 1 }
''', encoding='utf-8')
    (DEST / 'Ghep-File.cmd').write_text('@echo off\r\npowershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0Ghep-File.ps1"\r\npause\r\n', encoding='ascii')
    (DEST / 'HUONG-DAN.txt').write_text('VietKara Studio 1.0.0\n\nChon Setup hoac Portable. Tai ca hai phan .001 va .002 cua goi da chon.\nTai them downloads.json, Ghep-File.ps1, Ghep-File.cmd vao CUNG thu muc.\nChay Ghep-File.cmd de kiem tra SHA256 va ghep thanh file EXE/ZIP nguyen ven.\nCan khoang 6 GB trong tren o dia cho moi goi (cac phan + file ghep).\nSetup: chay EXE. Portable: giai nen ZIP, chay VietKaraStudio.exe.\nWindows 10/11 x64; can Edge WebView2. Model AI tai rieng trong Cai dat.\nBan phat hanh chua ky so; da thu tren may build, chua thu Windows VM sach.\n', encoding='utf-8')


if __name__ == '__main__':
    main()
