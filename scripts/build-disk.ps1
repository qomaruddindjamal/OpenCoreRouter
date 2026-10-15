# Build bootable Rinjani 1.0.0 DISK image (GPT + FAT32 ESP + Limine) for IS-01
# Pipeline: kernel ELF -> opcr-mkdisk (GPT+FAT32) -> limine bios-install -> verify -> VDI
[CmdletBinding()]
param(
    [string]$Profile = "debug",
    [long]$DiskMib = 2048,
    [string]$RawOut = "target/rinjani-1.0.0-x86_64-disk.raw",
    [string]$VdiOut = "target/rinjani-1.0.0-x86_64-disk.vdi"
)

$ErrorActionPreference = "Stop"
$workspaceRoot = "C:\OpenCR"
Set-Location $workspaceRoot

$kernelBin = "$workspaceRoot\target\x86_64-unknown-none\$Profile\asl-kernel"
$limineDir = "$workspaceRoot\tools\limine"
$vboxManage = "C:\Program Files\Oracle\VirtualBox\VBoxManage.exe"

Write-Host "=== BUILDING RINJANI 1.0.0 DISK IMAGE (IS-01) ==="

# 1. Kernel ELF
if (!(Test-Path $kernelBin)) {
    Write-Host "Compiling asl-kernel-bin for $Profile ..."
    $profileFlag = if ($Profile -eq "release") { "--release" } else { @() }
    cargo build -p asl-kernel-bin --target x86_64-unknown-none @profileFlag
    if (!(Test-Path $kernelBin)) { throw "kernel binary missing: $kernelBin" }
}

# 2. Limine provisioned?
if (!(Test-Path "$limineDir\limine.exe") -or !(Test-Path "$limineDir\limine-bios.sys")) {
    Write-Host "Limine missing. Provisioning..."
    & "$workspaceRoot\tools\provision-limine.ps1"
}

# 3. Build host-side mkdisk installer
Write-Host "Building opcr-mkdisk..."
cargo build --release --manifest-path "$workspaceRoot\scripts\mkdisk\Cargo.toml"
if ($LASTEXITCODE -ne 0) { throw "opcr-mkdisk build failed" }
$mkdisk = "$workspaceRoot\scripts\mkdisk\target\release\opcr-mkdisk.exe"

# 4. limine.conf content for on-disk boot
$limineConf = "timeout: 0`nverbose: yes`n`n/Rinjani 1.0.0 (ASL Core Kernel v1.0.0)`n    protocol: limine`n    path: boot():/boot/rinjani-kernel.elf`n"

# 5. Compose raw GPT+FAT32 image
Write-Host "Running opcr-mkdisk (DiskMib=$DiskMib)..."
& $mkdisk --kernel $kernelBin --limine-bios-sys "$limineDir\limine-bios.sys" --limine-conf-text $limineConf --out $RawOut --disk-mib $DiskMib
if (!(Test-Path $RawOut)) { throw "raw image not produced" }

# 6. Embed Limine MBR boot stage
Write-Host "limine.exe bios-install $RawOut ..."
& "$limineDir\limine.exe" bios-install $RawOut
if ($LASTEXITCODE -ne 0) { throw "limine bios-install failed" }

# 7. Verify boot sector integrity of the raw image
$fs = [System.IO.File]::OpenRead("$workspaceRoot\$RawOut")
$mbr = New-Object byte[] 512
$fs.Read($mbr, 0, 512) | Out-Null
$fs.Close()
if ($mbr[510] -ne 0x55 -or $mbr[511] -ne 0xAA) { throw "MBR signature lost after bios-install" }
if ($mbr[446 + 4] -ne 0xEE) { throw "Protective MBR partition entry (0xEE) lost after bios-install" }
$bootCodeNonZero = ($mbr[0..439] | Where-Object { $_ -ne 0 }).Count
Write-Host ("MBR OK: boot code non-zero bytes = {0}, PMBR type = 0xEE (intact)" -f $bootCodeNonZero)

# Header LBA1 check
$fs = [System.IO.File]::OpenRead("$workspaceRoot\$RawOut")
$hdr = New-Object byte[] 8
$fs.Seek(512, [System.IO.SeekOrigin]::Begin) | Out-Null
$fs.Read($hdr, 0, 8) | Out-Null
$fs.Close()
if ([System.Text.Encoding]::ASCII.GetString($hdr) -ne "EFI PART") { throw "GPT primary header missing at LBA1" }
Write-Host "GPT header 'EFI PART' present at LBA1."

# 8. Convert to VDI
if (Test-Path "$workspaceRoot\$VdiOut") { Remove-Item "$workspaceRoot\$VdiOut" -Force }
Write-Host "convertfromraw -> VDI ..."
& $vboxManage convertfromraw "$workspaceRoot\$RawOut" "$workspaceRoot\$VdiOut" --format VDI
if (!(Test-Path "$workspaceRoot\$VdiOut")) { throw "VDI conversion failed" }

$rawHash = (Get-FileHash -Path "$workspaceRoot\$RawOut" -Algorithm SHA256).Hash
$vdiSize = (Get-Item "$workspaceRoot\$VdiOut").Length
Write-Host "=== DISK IMAGE BUILD SUCCESSFUL ==="
Write-Host "RAW   : $RawOut"
Write-Host "RAW   : SHA256 $rawHash"
Write-Host "VDI   : $VdiOut ($vdiSize bytes)"
