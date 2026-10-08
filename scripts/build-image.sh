#!/bin/bash
# =============================================================================
# Open Core Router — Image Builder
# =============================================================================
# Builds a custom OpenWrt 25.12.5 x86-64 image with OCR stack pre-installed.
#
# Based on: openwrt-25.12.5-x86-64-generic-ext4-combined-efi.img
#   Partition 1: EFI (16 MB,  LBA 512–33279,   GRUB2 EFI bootloader)
#   Partition 2: Root FS (104 MB, LBA 33280–246271, EXT4)
#   Kernel: Linux 6.6.x
#   Init:   procd
#   PkgMgr: apk (Alpine Package Keeper)
#
# Usage:
#   ./scripts/build-image.sh [--clean] [--iso] [--vm]
#
# Output:
#   build/output/Open-Core-Router.1.0.1.img     (raw disk image)
#   build/output/Open-Core-Router.1.0.1.iso     (ISO for CD/VM boot)
#
# Requirements (run inside Docker — see scripts/Dockerfile.build):
#   - Docker Desktop (Windows) or Docker Engine (Linux)
#   - 4 GB free disk space
#   - Internet access (downloads Image Builder + packages)
#
# =============================================================================
set -euo pipefail
IFS=$'\n\t'

# ── Config ────────────────────────────────────────────────────────────────────
OPENWRT_VERSION="25.12.5"
OPENWRT_TARGET="x86"
OPENWRT_SUBTARGET="64"
OPENWRT_ARCH="x86_64"
IMAGEBUILDER_URL="https://downloads.openwrt.org/releases/${OPENWRT_VERSION}/targets/${OPENWRT_TARGET}/${OPENWRT_SUBTARGET}/openwrt-imagebuilder-${OPENWRT_VERSION}-${OPENWRT_TARGET}-${OPENWRT_SUBTARGET}.Linux-x86_64.tar.zst"

BUILD_DIR="$(cd "$(dirname "$0")/.." && pwd)/build"
OUTPUT_DIR="${BUILD_DIR}/output"
IMAGEBUILDER_DIR="${BUILD_DIR}/imagebuilder"
OVERLAY_DIR="${BUILD_DIR}/overlay"
SCRIPTS_DIR="$(cd "$(dirname "$0")" && pwd)"

OCR_NAME="Open-Core-Router"
OCR_VERSION="1.0.1"
OUTPUT_IMG="${OUTPUT_DIR}/${OCR_NAME}.${OCR_VERSION}.img"
OUTPUT_ISO="${OUTPUT_DIR}/${OCR_NAME}.${OCR_VERSION}.iso"
OUTPUT_QCOW2="${OUTPUT_DIR}/${OCR_NAME}.${OCR_VERSION}.qcow2"
OUTPUT_VMDK="${OUTPUT_DIR}/${OCR_NAME}.${OCR_VERSION}.vmdk"

# ── Flags ─────────────────────────────────────────────────────────────────────
DO_CLEAN=0
DO_ISO=1
DO_VM=0

for arg in "$@"; do
  case $arg in
    --clean) DO_CLEAN=1 ;;
    --no-iso) DO_ISO=0 ;;
    --vm)    DO_VM=1 ;;
  esac
done

# ── OCR packages to install ───────────────────────────────────────────────────
# All available in OpenWrt 25.12.5 apk repository for x86_64
OCR_PACKAGES="
  base-files
  libc
  busybox
  procd
  ubus
  ubox
  uci
  rpcd
  rpcd-mod-rpcsys
  netifd
  firewall4
  nftables
  kmod-nft-core
  kmod-nft-nat
  kmod-nft-bridge
  kmod-nft-offload
  ip-full
  ipset
  iptables-nft
  kmod-e1000e
  kmod-e1000
  kmod-r8169
  kmod-igb
  kmod-ixgbe
  kmod-virtio-net
  kmod-vmxnet3
  kmod-tun
  kmod-wireguard
  kmod-nf-conntrack
  kmod-nf-conntrack6
  kmod-sched-cake
  kmod-sched-core
  tc-full
  dnsmasq-full
  uhttpd
  uhttpd-mod-ubus
  luci
  luci-base
  luci-mod-admin-full
  luci-theme-bootstrap
  frr
  frr-zebra
  frr-staticd
  frr-ospfd
  frr-bgpd
  frr-ripd
  frr-vtysh
  wireguard-tools
  openvpn-openssl
  strongswan
  sqm-scripts
  kmod-sched-cake
  luci-app-sqm
  php83
  php83-fpm
  php83-curl
  php83-json
  php83-mbstring
  php83-openssl
  php83-pdo
  php83-pdo-sqlite
  php83-session
  php83-xml
  php83-zip
  lighttpd
  lighttpd-mod-fastcgi
  lighttpd-mod-access
  lighttpd-mod-redirect
  libsqlite3
  curl
  wget-ssl
  openssl-util
  tcpdump
  htop
  bash
  jq
  openssh-server
  openssh-client
  sudo
  shadow-utils
  ca-bundle
  libustream-mbedtls
  kmod-usb-core
  kmod-usb-storage
  kmod-fs-ext4
  kmod-fs-vfat
  e2fsprogs
  dosfstools
  gdisk
  lsblk
  pciutils
  usbutils
  logd
  iwinfo
"

# Remove newlines/comments, join with spaces
PACKAGES=$(echo "$OCR_PACKAGES" | grep -v '^#' | grep -v '^$' | tr '\n' ' ' | sed 's/  */ /g')

# ── Colors ────────────────────────────────────────────────────────────────────
RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'
CYAN='\033[0;36m'; BOLD='\033[1m'; NC='\033[0m'

info()    { echo -e "${CYAN}[INFO]${NC} $*"; }
success() { echo -e "${GREEN}[OK]${NC} $*"; }
warn()    { echo -e "${YELLOW}[WARN]${NC} $*"; }
error()   { echo -e "${RED}[ERR]${NC} $*"; exit 1; }
step()    { echo -e "\n${BOLD}${CYAN}━━ $* ━━${NC}"; }

# ── Dependency check ──────────────────────────────────────────────────────────
check_deps() {
  step "Checking build dependencies"
  local missing=()
  for cmd in wget tar make xorriso mksquashfs mkdosfs sgdisk; do
    if ! command -v "$cmd" &>/dev/null; then
      missing+=("$cmd")
    fi
  done
  if [[ ${#missing[@]} -gt 0 ]]; then
    error "Missing: ${missing[*]}. Run inside Docker: docker build -f scripts/Dockerfile.build -t ocr-builder ."
  fi
  success "All dependencies present"
}

# ── Download Image Builder ────────────────────────────────────────────────────
download_imagebuilder() {
  step "Downloading OpenWrt ${OPENWRT_VERSION} Image Builder"

  mkdir -p "${BUILD_DIR}/dl"
  local tarball="${BUILD_DIR}/dl/openwrt-imagebuilder-${OPENWRT_VERSION}.tar.zst"

  if [[ ! -f "$tarball" ]]; then
    info "Downloading from: ${IMAGEBUILDER_URL}"
    wget -q --show-progress -O "$tarball" "$IMAGEBUILDER_URL" || \
      error "Download failed. Check internet connection."
    success "Downloaded Image Builder"
  else
    info "Image Builder already cached at ${tarball}"
  fi

  if [[ ! -d "$IMAGEBUILDER_DIR" ]]; then
    info "Extracting Image Builder..."
    mkdir -p "$IMAGEBUILDER_DIR"
    tar --zstd -xf "$tarball" -C "$IMAGEBUILDER_DIR" --strip-components=1
    success "Image Builder extracted"
  fi
}

# ── Prepare OCR overlay ───────────────────────────────────────────────────────
prepare_overlay() {
  step "Preparing OCR file overlay"

  # Copy overlay files from build/overlay/ to imagebuilder files/ dir
  local ib_files="${IMAGEBUILDER_DIR}/files"
  mkdir -p "$ib_files"
  cp -r "${OVERLAY_DIR}/." "$ib_files/"

  # Copy Web UI to overlay
  mkdir -p "${ib_files}/www/ocr"
  cp -r "${BUILD_DIR}/../webui/frontend/." "${ib_files}/www/ocr/"

  # Copy Go daemon binary (if built)
  if [[ -f "${BUILD_DIR}/../daemon/bin/ocrd" ]]; then
    cp "${BUILD_DIR}/../daemon/bin/ocrd" "${ib_files}/usr/sbin/ocrd"
    chmod +x "${ib_files}/usr/sbin/ocrd"
    info "Go daemon binary included"
  else
    warn "ocrd binary not found — build with: cd daemon && GOOS=linux GOARCH=amd64 go build -o bin/ocrd ./cmd/ocrd"
  fi

  success "Overlay prepared ($(find "$ib_files" -type f | wc -l) files)"
}

# ── Build image ───────────────────────────────────────────────────────────────
build_image() {
  step "Building OpenWrt image with OCR packages"

  mkdir -p "$OUTPUT_DIR"

  cd "$IMAGEBUILDER_DIR"

  # Run Image Builder
  make image \
    PROFILE="generic" \
    PACKAGES="$PACKAGES" \
    FILES="${IMAGEBUILDER_DIR}/files" \
    BIN_DIR="$OUTPUT_DIR" \
    EXTRA_IMAGE_NAME="opencr-${OCR_VERSION}" \
    2>&1 | tee "${BUILD_DIR}/build.log"

  success "Image build completed"
}

# ── Rename and collect output ─────────────────────────────────────────────────
collect_output() {
  step "Collecting build output"

  # Find the generated EFI image
  local efi_img
  efi_img=$(find "$OUTPUT_DIR" -name "*combined-efi*" -not -name "*.gz" | head -1)

  if [[ -z "$efi_img" ]]; then
    # Try gzip'd
    efi_img=$(find "$OUTPUT_DIR" -name "*combined-efi*.gz" | head -1)
    if [[ -n "$efi_img" ]]; then
      info "Decompressing ${efi_img}..."
      gunzip "$efi_img"
      efi_img="${efi_img%.gz}"
    fi
  fi

  if [[ -z "$efi_img" ]]; then
    error "No EFI image found in ${OUTPUT_DIR}"
  fi

  # Rename to OCR output name
  cp "$efi_img" "$OUTPUT_IMG"
  success "IMG → ${OUTPUT_IMG} ($(du -h "$OUTPUT_IMG" | cut -f1))"
}

# ── Convert IMG → ISO ─────────────────────────────────────────────────────────
build_iso() {
  step "Converting IMG to bootable ISO"

  local WORK_DIR="${BUILD_DIR}/iso_work"
  local MOUNT_EFI="${BUILD_DIR}/mnt_efi"
  local MOUNT_ROOT="${BUILD_DIR}/mnt_root"

  mkdir -p "$WORK_DIR" "$MOUNT_EFI" "$MOUNT_ROOT"

  # Get partition offsets from IMG
  info "Reading partition table from ${OUTPUT_IMG}..."

  local SECTOR_SIZE=512
  # EFI partition: starts at LBA 512 → byte 262144
  local EFI_OFFSET=$((512 * SECTOR_SIZE))
  local EFI_SIZE=$((32768 * SECTOR_SIZE))   # ~16MB

  # Root FS partition: starts at LBA 33280 → byte 17039360
  local ROOT_OFFSET=$((33280 * SECTOR_SIZE))

  # Mount partitions using loop device
  local LOOP_DEV
  LOOP_DEV=$(losetup -f)
  losetup -P "$LOOP_DEV" "$OUTPUT_IMG"

  local LOOP_EFI="${LOOP_DEV}p1"
  local LOOP_ROOT="${LOOP_DEV}p2"

  mount -o ro "$LOOP_EFI"  "$MOUNT_EFI"
  mount -o ro "$LOOP_ROOT" "$MOUNT_ROOT"

  # Create ISO directory structure
  local ISO_DIR="${WORK_DIR}/iso"
  mkdir -p "${ISO_DIR}/boot/grub" "${ISO_DIR}/EFI/BOOT" "${ISO_DIR}/LiveOS"

  # Copy EFI bootloader
  cp -r "${MOUNT_EFI}/." "${ISO_DIR}/EFI/"

  # Copy kernel and initramfs
  if [[ -f "${MOUNT_ROOT}/boot/vmlinuz" ]]; then
    cp "${MOUNT_ROOT}/boot/vmlinuz"  "${ISO_DIR}/boot/"
    cp "${MOUNT_ROOT}/boot/initrd"   "${ISO_DIR}/boot/" 2>/dev/null || true
  fi

  # Create squashfs of root filesystem for ISO
  info "Creating root SquashFS..."
  mksquashfs "$MOUNT_ROOT" "${ISO_DIR}/LiveOS/squashfs.img" \
    -noappend -comp xz -Xbcj x86 -b 1M \
    -e boot -e proc -e sys -e dev -e tmp \
    2>/dev/null
  success "SquashFS created ($(du -h "${ISO_DIR}/LiveOS/squashfs.img" | cut -f1))"

  # Create GRUB config for ISO boot
  cat > "${ISO_DIR}/boot/grub/grub.cfg" << 'GRUBCFG'
set default=0
set timeout=5

menuentry "Open Core Router 1.0.0 (OpenWrt 25.12.5)" {
  linux   /boot/vmlinuz root=live:CDLABEL=OCR-LIVE ro console=tty0 console=ttyS0,115200n8
  initrd  /boot/initrd
}

menuentry "Open Core Router — Rescue Mode" {
  linux   /boot/vmlinuz root=live:CDLABEL=OCR-LIVE ro rescue console=tty0 console=ttyS0,115200n8
  initrd  /boot/initrd
}

menuentry "Boot from local disk" {
  exit
}
GRUBCFG

  # Build ISO with xorriso + GRUB EFI
  info "Building ISO with xorriso..."
  xorriso \
    -as mkisofs \
    -o "$OUTPUT_ISO" \
    -V "OCR-LIVE" \
    --modification-date="$(date +%Y%m%d%H%M%S)00" \
    -isohybrid-mbr /usr/lib/ISOLINUX/isohdpfx.bin 2>/dev/null || \
  xorriso \
    -as mkisofs \
    -o "$OUTPUT_ISO" \
    -V "OCR-LIVE" \
    -J -R \
    -b boot/grub/i386-pc/eltorito.img \
    -c boot/grub/i386-pc/boot.cat \
    -no-emul-boot -boot-load-size 4 -boot-info-table \
    --efi-boot EFI/BOOT/bootx64.efi \
    -efi-boot-part --efi-boot-image \
    "$ISO_DIR" \
    2>&1 | tail -5

  # Cleanup mounts
  umount "$MOUNT_EFI" "$MOUNT_ROOT" 2>/dev/null || true
  losetup -d "$LOOP_DEV" 2>/dev/null || true

  success "ISO → ${OUTPUT_ISO} ($(du -h "$OUTPUT_ISO" | cut -f1))"
}

# ── VM image (VMDK/QCOW2) ────────────────────────────────────────────────────
build_vm() {
  step "Converting to VM formats"

  if command -v qemu-img &>/dev/null; then
    # QCOW2 for QEMU/KVM/Proxmox
    local QCOW2="${OUTPUT_DIR}/${OCR_NAME}-${OCR_VERSION}-x86-64.qcow2"
    qemu-img convert -f raw -O qcow2 "$OUTPUT_IMG" "$QCOW2"
    success "QCOW2 → ${QCOW2} ($(du -h "$QCOW2" | cut -f1))"

    # VMDK for VMware/VirtualBox
    local VMDK="${OUTPUT_DIR}/${OCR_NAME}-${OCR_VERSION}-x86-64.vmdk"
    qemu-img convert -f raw -O vmdk "$OUTPUT_IMG" "$VMDK"
    success "VMDK → ${VMDK} ($(du -h "$VMDK" | cut -f1))"
  else
    warn "qemu-img not found — skipping VM formats"
  fi
}

# ── Generate SHA256 checksums ─────────────────────────────────────────────────
generate_checksums() {
  step "Generating checksums"
  cd "$OUTPUT_DIR"
  sha256sum *.img *.iso *.qcow2 *.vmdk 2>/dev/null > SHA256SUMS || true
  cat SHA256SUMS
  success "Checksums saved to ${OUTPUT_DIR}/SHA256SUMS"
}

# ── Print summary ─────────────────────────────────────────────────────────────
print_summary() {
  echo ""
  echo -e "${BOLD}${GREEN}╔══════════════════════════════════════════════════════════╗${NC}"
  echo -e "${BOLD}${GREEN}║     Open Core Router — Build Complete!                   ║${NC}"
  echo -e "${BOLD}${GREEN}╚══════════════════════════════════════════════════════════╝${NC}"
  echo ""
  echo -e "  Base:    OpenWrt ${OPENWRT_VERSION} x86-64 (Linux 6.6.x)"
  echo -e "  Version: OCR ${OCR_VERSION}"
  echo -e "  Output:  ${OUTPUT_DIR}/"
  echo ""
  ls -lh "${OUTPUT_DIR}"/*.img "${OUTPUT_DIR}"/*.iso "${OUTPUT_DIR}"/*.qcow2 "${OUTPUT_DIR}"/*.vmdk 2>/dev/null || true
  echo ""
  echo -e "  ${BOLD}Flash to USB:${NC}"
  echo -e "    sudo dd if=${OUTPUT_IMG} of=/dev/sdX bs=4M status=progress"
  echo -e ""
  echo -e "  ${BOLD}Boot in QEMU:${NC}"
  echo -e "    qemu-system-x86_64 -m 512M -enable-kvm -hda ${OUTPUT_IMG}"
  echo -e ""
  echo -e "  ${BOLD}Default credentials:${NC}  root / (no password) → set via Web UI"
  echo -e "  ${BOLD}Web UI:${NC}               http://192.168.1.1"
  echo -e "  ${BOLD}API:${NC}                  http://192.168.1.1/api/v1"
  echo ""
}

# ── Main ──────────────────────────────────────────────────────────────────────
main() {
  echo ""
  echo -e "${BOLD}${CYAN}Open Core Router — Image Builder${NC}"
  echo -e "Based on OpenWrt ${OPENWRT_VERSION} x86-64"
  echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
  echo ""

  if [[ $DO_CLEAN -eq 1 ]]; then
    step "Cleaning previous build"
    rm -rf "$OUTPUT_DIR" "${IMAGEBUILDER_DIR}/files"
    success "Cleaned"
  fi

  check_deps
  download_imagebuilder
  prepare_overlay
  build_image
  collect_output

  if [[ $DO_ISO -eq 1 ]]; then
    build_iso
  fi

  if [[ $DO_VM -eq 1 ]]; then
    build_vm
  fi

  generate_checksums
  print_summary
}

main "$@"
