# ====================================================================
# Photobooth RS - Skrip Penerbitan Automatik (Publish Release)
# ====================================================================
# Skrip ini mengkompilasi kod versi release, mengemas kini pakej
# portable, mengira SHA256, dan menjana metadata version.json
# untuk diedarkan kepada rakan/pengguna secara automatik.
# ====================================================================

$ErrorActionPreference = "Stop"

Write-Host "`n🚀 [1/5] Mengesan versi projek dari Cargo.toml..." -ForegroundColor Cyan
$cargoToml = Get-Content "Cargo.toml" -Raw
if ($cargoToml -match 'version\s*=\s*"([^"]+)"') {
    $version = $matches[1]
    Write-Host "    Versi dikesan: v$version" -ForegroundColor Green
} else {
    $version = "0.1.0"
    Write-Host "    Gagal membaca versi, menggunakan lalai: v$version" -ForegroundColor Yellow
}

Write-Host "`n🔨 [2/5] Mengkompilasi Production Release Build (cargo build --release)..." -ForegroundColor Cyan
$msvcrtBin = "C:\Users\it-01995\AppData\Local\Microsoft\WinGet\Packages\BrechtSanders.WinLibs.POSIX.MSVCRT_Microsoft.Winget.Source_8wekyb3d8bbwe\mingw64\bin"
$env:Path = "$msvcrtBin;$HOME\.cargo\bin;$env:Path"

cargo build --release
if ($LASTEXITCODE -ne 0) {
    Write-Host "❌ Kompilasi gagal!" -ForegroundColor Red
    exit 1
}

$releaseExe = ".\target\release\photobooth-rs.exe"
if (-not (Test-Path $releaseExe)) {
    Write-Host "❌ Fail binari release tidak dijumpai: $releaseExe" -ForegroundColor Red
    exit 1
}

$exeSizeMB = [math]::Round((Get-Item $releaseExe).Length / 1MB, 2)
Write-Host "    Kompilasi berjaya! Saiz binari: $exeSizeMB MB" -ForegroundColor Green

Write-Host "`n🔒 [3/5] Mengira kod cincangan integriti (SHA-256)..." -ForegroundColor Cyan
$hash = (Get-FileHash -Path $releaseExe -Algorithm SHA256).Hash
Write-Host "    SHA-256: $hash" -ForegroundColor Gray

Write-Host "`n📦 [4/5] Membina pakej edaran mudah alih (Photobooth-RS-Portable)..." -ForegroundColor Cyan
$distDir = ".\dist\Photobooth-RS-Portable"
if (Test-Path $distDir) { Remove-Item -Recurse -Force $distDir }
New-Item -ItemType Directory -Path "$distDir\assets\templates" -Force | Out-Null
New-Item -ItemType Directory -Path "$distDir\assets\icons" -Force | Out-Null
New-Item -ItemType Directory -Path "$distDir\output\raw" -Force | Out-Null
New-Item -ItemType Directory -Path "$distDir\output\composites" -Force | Out-Null
New-Item -ItemType Directory -Path "$distDir\output\mock_prints" -Force | Out-Null

Copy-Item $releaseExe "$distDir\Photobooth-RS.exe"
$cscPath = "C:\Windows\Microsoft.NET\Framework\v4.0.30319\csc.exe"
if ((Test-Path $cscPath) -and (Test-Path ".\tools\canon_bridge\CanonBridge.cs") -and (Test-Path ".\tools\canon_bridge\lib\Canon.Eos.Framework.dll") -and (Test-Path ".\tools\canon_bridge\lib\CameraControl.Devices.dll")) {
    Write-Host "    Mengkompilasi CanonBridge.exe (x86 EDSDK Live View)..." -ForegroundColor Cyan
    & $cscPath /nologo /target:winexe /platform:x86 /optimize+ /out:"$distDir\CanonBridge.exe" /reference:".\tools\canon_bridge\lib\Canon.Eos.Framework.dll" /reference:".\tools\canon_bridge\lib\CameraControl.Devices.dll" ".\tools\canon_bridge\CanonBridge.cs"
    if ($LASTEXITCODE -ne 0) { throw "CanonBridge.cs compilation failed" }
    Copy-Item ".\tools\canon_bridge\lib\*.dll" "$distDir\" -Force
}
Copy-Item ".\assets\templates\*" "$distDir\assets\templates\" -Recurse
Copy-Item ".\assets\icons\app_icon.ico" "$distDir\assets\icons\"
if (Test-Path ".\config.json") { Copy-Item ".\config.json" "$distDir\config.json" }
if (Test-Path ".\dist\Photobooth-RS-Portable\PANDUAN_PENGGUNAAN.txt") {
    Copy-Item ".\dist\Photobooth-RS-Portable\PANDUAN_PENGGUNAAN.txt" "$distDir\PANDUAN_PENGGUNAAN.txt"
}

# Update ZIP and raw release binary
New-Item -ItemType Directory -Path ".\releases" -Force | Out-Null
Copy-Item $releaseExe ".\releases\Photobooth-RS.exe"
$zipPath = ".\dist\Photobooth-RS-Portable.zip"
if (Test-Path $zipPath) { Remove-Item -Force $zipPath }
Compress-Archive -Path "$distDir\*" -DestinationPath $zipPath -CompressionLevel Optimal
$zipSizeMB = [math]::Round((Get-Item $zipPath).Length / 1MB, 2)
Write-Host "    Pakej ZIP berjaya dibina: $zipPath ($zipSizeMB MB)" -ForegroundColor Green

Write-Host "`n🌐 [5/5] Menjana metadata kemas kini version.json..." -ForegroundColor Cyan
$publishedAt = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ")
$versionJson = @{
    version = $version
    release_notes = "Kemas kini Photobooth RS v$version - Pemacu kamera laptop, layout picker, dan in-app auto-update."
    download_url = "https://github.com/ImranNaufal/photobooth/raw/main/releases/Photobooth-RS.exe"
    sha256 = $hash
    published_at = $publishedAt
} | ConvertTo-Json -Depth 4

$versionJsonPath = ".\dist\version.json"
$resolvedPath = if (Test-Path $versionJsonPath) { (Resolve-Path $versionJsonPath).Path } else { [System.IO.Path]::GetFullPath($versionJsonPath) }
[System.IO.File]::WriteAllText($resolvedPath, $versionJson, [System.Text.UTF8Encoding]::new($false))
Write-Host "    version.json berjaya dijana di: $versionJsonPath (UTF-8 No-BOM)" -ForegroundColor Green

Write-Host "`n====================================================================" -ForegroundColor Green
Write-Host "✨ PENERBITAN RELEASE SELESAI!" -ForegroundColor Green
Write-Host "====================================================================" -ForegroundColor Green
Write-Host "Untuk membolehkan kawan anda menerima update ini secara automatik:"
Write-Host "1. Upload fail 'dist\Photobooth-RS-Portable\Photobooth-RS.exe' dan 'dist\version.json'"
Write-Host "   ke repo GitHub atau cloud hosting anda."
Write-Host "2. Apabila kawan anda buka app mereka (atau tekan 'Check Updates' di Admin),"
Write-Host "   mereka akan dapat popup kemas kini dan butang 1-Klik!"
Write-Host "====================================================================`n"
