# Build Bootable ISO for Rinjani 1.0.0 (Powered by ASL Core Kernel v1.0.0)
[CmdletBinding()]
param(
    [string]$Profile = "debug",
    [string]$IsoOut = "target/rinjani-1.0.0-x86_64.iso"
)

$ErrorActionPreference = "Stop"

$workspaceRoot = "C:\OpenCR"
$kernelBin = "$workspaceRoot\target\x86_64-unknown-none\$Profile\asl-kernel"
$limineDir = "$workspaceRoot\tools\limine"
$isoRoot = "$workspaceRoot\target\iso_root"

Write-Host "=== BUILDING RINJANI 1.0.0 BOOTABLE ISO ==="

# 1. Pastikan kernel binary ada
if (!(Test-Path $kernelBin)) {
    Write-Host "Kernel binary not found. Compiling asl-kernel-bin..."
    cargo build -p asl-kernel-bin --target x86_64-unknown-none
    if (!(Test-Path $kernelBin)) {
        throw "Failed to produce kernel binary at $kernelBin"
    }
}

# 2. Pastikan Limine binaries tersedia
if (!(Test-Path "$limineDir\limine.exe")) {
    Write-Host "Limine binaries missing. Running provision-limine.ps1..."
    & "$workspaceRoot\tools\provision-limine.ps1"
}

# 3. Siapkan direktori iso_root
if (Test-Path $isoRoot) {
    Remove-Item -Path $isoRoot -Recurse -Force
}
New-Item -ItemType Directory -Path "$isoRoot\boot\limine" -Force | Out-Null

# 4. Salin Kernel ke /boot/rinjani-kernel.elf
Copy-Item -Path $kernelBin -Destination "$isoRoot\boot\rinjani-kernel.elf" -Force

# 5. Buat limine.conf
$limineConf = @"
timeout: 0
verbose: yes

/Rinjani 1.0.0 (ASL Core Kernel v1.0.0)
    protocol: limine
    path: boot():/boot/rinjani-kernel.elf
"@

$limineConf | Out-File -FilePath "$isoRoot\boot\limine\limine.conf" -Encoding ascii -NoNewline
$limineConf | Out-File -FilePath "$isoRoot\limine.conf" -Encoding ascii -NoNewline

# 6. Salin file bootloader Limine ke iso_root
Copy-Item -Path "$limineDir\limine-bios.sys" -Destination "$isoRoot\boot\limine\" -Force
Copy-Item -Path "$limineDir\limine-bios-cd.bin" -Destination "$isoRoot\boot\limine\" -Force
Copy-Item -Path "$limineDir\limine-uefi-cd.bin" -Destination "$isoRoot\boot\limine\" -Force

# 7. Jalankan xorriso untuk membuat ISO hybrid
$targetDir = Split-Path -Parent $IsoOut
if ($targetDir -and !(Test-Path $targetDir)) {
    New-Item -ItemType Directory -Path $targetDir -Force | Out-Null
}

$xorrisoArgs = @(
    "-as", "mkisofs",
    "-b", "boot/limine/limine-bios-cd.bin",
    "-no-emul-boot",
    "-boot-load-size", "4",
    "-boot-info-table",
    "--efi-boot", "boot/limine/limine-uefi-cd.bin",
    "-efi-boot-part",
    "--efi-boot-image",
    "--protective-msdos-label",
    "-V", "RINJANI",
    "-o", $IsoOut,
    "target/iso_root"
)

Write-Host "Running xorriso..."
& xorriso $xorrisoArgs

if (!(Test-Path $IsoOut)) {
    throw "Failed to create ISO at $IsoOut"
}

# 8. Tanamkan MBR boot code via limine.exe bios-install
Write-Host "Running limine.exe bios-install..."
& "$limineDir\limine.exe" bios-install $IsoOut

$isoSize = (Get-Item $IsoOut).Length
$isoHash = (Get-FileHash -Path $IsoOut -Algorithm SHA256).Hash

Write-Host "=== ISO BUILD SUCCESSFUL ==="
Write-Host "Output: $IsoOut"
Write-Host "Size  : $isoSize bytes"
Write-Host "SHA256: $isoHash"
