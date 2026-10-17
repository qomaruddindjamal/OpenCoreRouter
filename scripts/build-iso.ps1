# Build Bootable ISO for Rinjani 1.0.0 (Powered by ASL Core Kernel v1.0.0)
#
# Profil default = release, sama seperti build-disk.ps1. Limine membaca SELURUH
# berkas ELF ke high memory sebelum memetakan segmen, jadi ukuran berkas
# menentukan RAM minimum tamu: ELF debug 3 703 992 B memicu PANIC 'High memory
# allocator: Out of memory' di freadall pada guest 4 MiB, ELF release 707 168 B
# muat. Debug tetap tersedia lewat -Profile debug untuk tamu ber-RAM besar.
[CmdletBinding()]
param(
    [string]$Profile = "release",
    [string]$IsoOut = "target/rinjani-1.0.0-x86_64.iso",
    [long]$ElfMaxBytes = 1048576
)

$ErrorActionPreference = "Stop"

$workspaceRoot = "C:\OpenCR"
$kernelBin = "$workspaceRoot\target\x86_64-unknown-none\$Profile\asl-kernel"
$limineDir = "$workspaceRoot\tools\limine"
$isoRoot = "$workspaceRoot\target\iso_root"

Write-Host "=== BUILDING RINJANI 1.0.0 BOOTABLE ISO ==="

# 1. Pastikan kernel binary ada (profil yang sama dengan $Profile)
if (!(Test-Path $kernelBin)) {
    Write-Host "Kernel binary not found. Compiling asl-kernel-bin ($Profile)..."
    if ($Profile -eq "release") {
        cargo build -p asl-kernel-bin --release --target x86_64-unknown-none
    } else {
        cargo build -p asl-kernel-bin --target x86_64-unknown-none
    }
    if (!(Test-Path $kernelBin)) {
        throw "Failed to produce kernel binary at $kernelBin"
    }
}

# 1b. Gerbang tapak boot: berkas ELF harus termuat di high memory tamu.
$elfBytes = (Get-Item $kernelBin).Length
if ($elfBytes -gt $ElfMaxBytes) {
    throw "kernel ELF $elfBytes B > batas $ElfMaxBytes B; guest 4 MiB tidak akan termuat (pakai -Profile release)"
}
# Jumlah memsz segmen PT_LOAD = citra residen kernel di RAM.
$fs = [System.IO.File]::OpenRead($kernelBin)
try {
    $hdr = New-Object byte[] 64
    $fs.Read($hdr, 0, 64) | Out-Null
    if ($hdr[0] -ne 0x7F -or $hdr[1] -ne 0x45 -or $hdr[2] -ne 0x4C -or $hdr[3] -ne 0x46) { throw "bukan ELF" }
    if ($hdr[4] -ne 2) { throw "bukan ELF64" }
    $phoff = [System.BitConverter]::ToInt64($hdr, 32)
    $phentsize = [System.BitConverter]::ToInt16($hdr, 54)
    $phnum = [System.BitConverter]::ToInt16($hdr, 56)
    $resident = [long]0
    $loads = 0
    for ($i = 0; $i -lt $phnum; $i++) {
        $ph = New-Object byte[] $phentsize
        $fs.Seek($phoff + $i * $phentsize, [System.IO.SeekOrigin]::Begin) | Out-Null
        $fs.Read($ph, 0, $phentsize) | Out-Null
        if ([System.BitConverter]::ToInt32($ph, 0) -eq 1) {
            $loads++
            $resident += [System.BitConverter]::ToInt64($ph, 40)
        }
    }
} finally { $fs.Close() }
Write-Host ("BOOT FOOTPRINT: ELF {0} B, PT_LOAD {1} segmen, residen {2} B (batas ELF {3} B)" -f $elfBytes, $loads, $resident, $ElfMaxBytes)

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
