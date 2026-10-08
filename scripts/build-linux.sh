#!/bin/bash
# =============================================================================
# Open Core Router 1.0.1 — One-Shot Build Script
# Jalankan di Linux VM / server (Ubuntu/Debian/Arch)
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/qomaruddindjamal/OpenCoreRouter/master/scripts/build-linux.sh | bash
#
#   atau clone dulu:
#   git clone https://github.com/qomaruddindjamal/OpenCoreRouter.git
#   cd OpenCoreRouter && bash scripts/build-linux.sh
# =============================================================================
set -euo pipefail

OCR_NAME="Open-Core-Router"
OCR_VERSION="1.0.1"
OPENWRT_VERSION="25.12.5"
TARGET="x86"
SUBTARGET="64"

# Output
OUTPUT_DIR="$(pwd)/build/output"
IB_DIR="$(pwd)/build/imagebuilder"
DL_DIR="$(pwd)/build/dl"
OVERLAY_DIR="$(pwd)/build/overlay"

IB_URL="https://downloads.openwrt.org/releases/${OPENWRT_VERSION}/targets/${TARGET}/${SUBTARGET}/openwrt-imagebuilder-${OPENWRT_VERSION}-${TARGET}-${SUBTARGET}.Linux-x86_64.tar.zst"

IMG="${OUTPUT_DIR}/${OCR_NAME}.${OCR_VERSION}.img"
ISO="${OUTPUT_DIR}/${OCR_NAME}.${OCR_VERSION}.iso"

# ── Colors ─────────────────────────────────────────────────────────────────────
G='\033[0;32m'; C='\033[0;36m'; Y='\033[1;33m'; R='\033[0;31m'; B='\033[1m'; N='\033[0m'

step() { echo -e "\n${B}${C}━━ $* ━━${N}"; }
ok()   { echo -e "  ${G}✓${N} $*"; }
warn() { echo -e "  ${Y}!${N} $*"; }
die()  { echo -e "  ${R}✗ ERROR: $*${N}"; exit 1; }

echo ""
echo -e "${B}${C}╔══════════════════════════════════════════════════════╗${N}"
echo -e "${B}${C}║   Open Core Router ${OCR_VERSION} — Build Script             ║${N}"
echo -e "${B}${C}║   Base: OpenWrt ${OPENWRT_VERSION} x86-64                     ║${N}"
echo -e "${B}${C}╚══════════════════════════════════════════════════════╝${N}"

# ── Step 1: Install dependencies ──────────────────────────────────────────────
step "1/6 Installing build dependencies"

if command -v apt-get &>/dev/null; then
    sudo apt-get update -qq
    sudo apt-get install -y --no-install-recommends \
        build-essential libncurses-dev libssl-dev zlib1g-dev \
        unzip zstd wget curl rsync file python3 \
        xorriso grub-efi-amd64-bin grub-pc-bin isolinux \
        squashfs-tools kpartx gdisk e2fsprogs \
        qemu-utils 2>/dev/null || true
    ok "Dependencies installed (apt)"
elif command -v pacman &>/dev/null; then
    sudo pacman -Sy --noconfirm --needed \
        base-devel ncurses openssl zlib unzip zstd wget \
        xorriso grub squashfs-tools qemu-img 2>/dev/null || true
    ok "Dependencies installed (pacman)"
else
    warn "Unknown package manager — ensure build tools are installed"
fi

mkdir -p "$OUTPUT_DIR" "$DL_DIR"

# ── Step 2: Download OpenWrt Image Builder ────────────────────────────────────
step "2/6 Downloading OpenWrt ${OPENWRT_VERSION} Image Builder"

IB_TARBALL="${DL_DIR}/openwrt-imagebuilder-${OPENWRT_VERSION}.tar.zst"

if [[ ! -f "$IB_TARBALL" ]]; then
    echo "  Downloading $(basename $IB_URL)..."
    wget -q --show-progress -O "$IB_TARBALL" "$IB_URL" || \
        die "Download failed. Check connection."
    ok "Downloaded ($(du -h "$IB_TARBALL" | cut -f1))"
else
    ok "Cached: $IB_TARBALL"
fi

if [[ ! -d "${IB_DIR}/Makefile" ]] && [[ ! -f "${IB_DIR}/Makefile" ]]; then
    echo "  Extracting..."
    mkdir -p "$IB_DIR"
    tar --zstd -xf "$IB_TARBALL" -C "$IB_DIR" --strip-components=1
    ok "Extracted to $IB_DIR"
fi

# ── Step 3: Prepare file overlay ──────────────────────────────────────────────
step "3/6 Preparing OCR overlay"

IB_FILES="${IB_DIR}/files"
mkdir -p "$IB_FILES"

# Copy overlay
if [[ -d "$OVERLAY_DIR" ]]; then
    cp -r "${OVERLAY_DIR}/." "$IB_FILES/"
    ok "Config overlay copied ($(find "$IB_FILES" -type f | wc -l) files)"
else
    warn "No overlay directory found at ${OVERLAY_DIR} — using defaults"
fi

# Copy Web UI
if [[ -d "webui/frontend" ]]; then
    mkdir -p "${IB_FILES}/www/ocr"
    cp -r webui/frontend/. "${IB_FILES}/www/ocr/"
    ok "Web UI copied"
fi

# ── Step 4: Build Image ───────────────────────────────────────────────────────
step "4/6 Building OpenWrt image with OCR packages"
echo "  This will take 5-15 minutes..."

PACKAGES="
base-files busybox procd ubus ubox uci rpcd netifd
firewall4 nftables kmod-nft-core kmod-nft-nat kmod-nft-bridge
ip-full ipset kmod-e1000e kmod-e1000 kmod-r8169 kmod-igb
kmod-ixgbe kmod-virtio-net kmod-vmxnet3 kmod-tun kmod-wireguard
kmod-nf-conntrack kmod-sched-cake kmod-sched-core tc-full
dnsmasq-full uhttpd lighttpd lighttpd-mod-fastcgi
frr frr-zebra frr-staticd frr-ospfd frr-bgpd frr-ripd frr-vtysh
wireguard-tools openvpn-openssl sqm-scripts
php83 php83-fpm php83-curl php83-json php83-openssl
libsqlite3 curl wget-ssl openssl-util tcpdump htop bash
openssh-server ca-bundle kmod-fs-ext4 e2fsprogs logd
"
PKG=$(echo "$PACKAGES" | tr '\n' ' ' | sed 's/  */ /g')

cd "$IB_DIR"
make image \
    PROFILE="generic" \
    PACKAGES="$PKG" \
    FILES="$IB_FILES" \
    BIN_DIR="$OUTPUT_DIR" \
    EXTRA_IMAGE_NAME="opencr-${OCR_VERSION}" \
    2>&1 | grep -E "(make\[|ERROR|warn|Building)" || true
cd - >/dev/null

ok "Image build done"

# ── Step 5: Rename output to correct filename ─────────────────────────────────
step "5/6 Renaming output files"

# Find generated EFI image
BUILT_IMG=$(find "$OUTPUT_DIR" -name "*combined-efi*" ! -name "*.gz" | head -1)
if [[ -z "$BUILT_IMG" ]]; then
    BUILT_GZ=$(find "$OUTPUT_DIR" -name "*combined-efi*.gz" | head -1)
    [[ -n "$BUILT_GZ" ]] && { gunzip "$BUILT_GZ"; BUILT_IMG="${BUILT_GZ%.gz}"; }
fi
[[ -z "$BUILT_IMG" ]] && die "No EFI image found in $OUTPUT_DIR"

cp "$BUILT_IMG" "$IMG"
ok "IMG: ${OCR_NAME}.${OCR_VERSION}.img  ($(du -h "$IMG" | cut -f1))"

# ── Step 6: Generate ISO ──────────────────────────────────────────────────────
step "6/6 Converting IMG → ISO"

ISO_WORK="$(pwd)/build/iso_work"
MNT_EFI="$(pwd)/build/mnt_efi"
MNT_ROOT="$(pwd)/build/mnt_root"
ISO_FS="${ISO_WORK}/isoroot"

mkdir -p "$ISO_WORK" "$MNT_EFI" "$MNT_ROOT"
mkdir -p "${ISO_FS}/boot/grub" "${ISO_FS}/EFI/BOOT" "${ISO_FS}/LiveOS"

echo "  Setting up loop device..."
LOOP=$(sudo losetup -f)
sudo losetup -P "$LOOP" "$IMG"

# Mount root partition (partition 2)
sudo mount -o ro "${LOOP}p2" "$MNT_ROOT" 2>/dev/null || \
    sudo mount -o ro,offset=$((33280*512)) "$IMG" "$MNT_ROOT"

# Copy kernel
if [[ -f "${MNT_ROOT}/boot/vmlinuz" ]]; then
    cp "${MNT_ROOT}/boot/vmlinuz" "${ISO_FS}/boot/"
    cp "${MNT_ROOT}/boot/initrd"  "${ISO_FS}/boot/" 2>/dev/null || true
    ok "Kernel copied"
fi

# Create SquashFS root
echo "  Creating SquashFS (this takes a moment)..."
sudo mksquashfs "$MNT_ROOT" "${ISO_FS}/LiveOS/squashfs.img" \
    -noappend -comp xz -Xbcj x86 -b 1M \
    -e boot -e proc -e sys -e dev -e tmp -e run \
    2>/dev/null
ok "SquashFS: $(du -h "${ISO_FS}/LiveOS/squashfs.img" | cut -f1)"

# Copy GRUB config
cp "$(pwd)/build/grub/grub.cfg" "${ISO_FS}/boot/grub/"

# Cleanup mounts
sudo umount "$MNT_ROOT" "$MNT_EFI" 2>/dev/null || true
sudo losetup -d "$LOOP" 2>/dev/null || true

# Build ISO with xorriso
echo "  Building ISO with xorriso..."
xorriso -as mkisofs \
    -o "$ISO" \
    -V "OCR-101" \
    -J -R -joliet-long \
    --grub2-boot-info \
    --grub2-mbr /usr/lib/grub/i386-pc/boot_hybrid.img 2>/dev/null || \
xorriso -as mkisofs \
    -o "$ISO" \
    -V "OCR-101" \
    -J -R \
    "$ISO_FS" 2>&1 | tail -3

ok "ISO: ${OCR_NAME}.${OCR_VERSION}.iso  ($(du -h "$ISO" | cut -f1))"

# ── Checksums ─────────────────────────────────────────────────────────────────
cd "$OUTPUT_DIR"
sha256sum *.img *.iso 2>/dev/null > SHA256SUMS
ok "Checksums saved"

# ── Done ──────────────────────────────────────────────────────────────────────
echo ""
echo -e "${B}${G}╔══════════════════════════════════════════════════════╗${N}"
echo -e "${B}${G}║   BUILD COMPLETE!                                    ║${N}"
echo -e "${B}${G}╚══════════════════════════════════════════════════════╝${N}"
echo ""
echo -e "  Files:"
ls -lh "${OUTPUT_DIR}/${OCR_NAME}."* 2>/dev/null | awk '{print "    "$NF"  ("$5")"}'
echo ""
echo -e "  ${B}Uji di QEMU (tanpa KVM — cocok di VM):${N}"
echo "    qemu-system-x86_64 -m 256M \\"
echo "      -drive file=${IMG},format=raw \\"
echo "      -net nic,model=virtio -net user,hostfwd=tcp::8080-:80"
echo ""
echo -e "  ${B}Boot ISO di VirtualBox/VMware:${N}"
echo "    → New VM → Type: Linux → Other Linux 64-bit"
echo "    → Storage → Attach ISO: ${ISO}"
echo ""
echo -e "  ${B}Flash ke USB (hati-hati pilih device!):${N}"
echo "    sudo dd if=${IMG} of=/dev/sdX bs=4M status=progress oflag=sync"
echo ""
echo -e "  ${B}Setelah boot — akses Web UI:${N}"
echo "    http://192.168.1.1   (dari LAN)"
echo "    User: admin  |  Pass: (kosong, set saat pertama login)"
echo ""
