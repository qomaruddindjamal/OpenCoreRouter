#!/bin/bash
# =============================================================================
# Open Core Router — Docker Build Entrypoint
# /entrypoint.sh (inside container)
# =============================================================================
set -euo pipefail

OCR_NAME="Open-Core-Router"
OCR_VERSION="1.0.1"
OCR_DIR="/opencr"
OUTPUT_DIR="${OCR_DIR}/build/output"

echo ""
echo "╔══════════════════════════════════════════════════════════════╗"
echo "║   Open Core Router — Docker Build Environment               ║"
echo "║   Version: ${OCR_VERSION}   Base: OpenWrt 25.12.5 x86-64        ║"
echo "╚══════════════════════════════════════════════════════════════╝"
echo ""

mkdir -p "$OUTPUT_DIR"

# ── Step 1: Build Go daemon ───────────────────────────────────────────────────
echo "▶ [1/5] Building ocrd Go daemon..."
cd "${OCR_DIR}/daemon"
GOOS=linux GOARCH=amd64 CGO_ENABLED=0 \
  go build \
    -ldflags="-s -w \
      -X main.Version=${OCR_VERSION} \
      -X main.BuildDate=$(date -u +%Y-%m-%dT%H:%M:%SZ) \
      -X main.Commit=$(git -C ${OCR_DIR} rev-parse --short HEAD 2>/dev/null || echo 'unknown')" \
    -o "${OCR_DIR}/daemon/bin/ocrd" \
    ./cmd/ocrd
echo "   ✓ ocrd binary built ($(du -h ${OCR_DIR}/daemon/bin/ocrd | cut -f1))"

# ── Step 2: Run image builder ─────────────────────────────────────────────────
echo "▶ [2/5] Running OpenWrt Image Builder..."
cd "$OCR_DIR"
bash scripts/build-image.sh "$@"

# ── Step 3: Generate checksums ────────────────────────────────────────────────
echo "▶ [3/5] Generating checksums..."
cd "$OUTPUT_DIR"
sha256sum *.img *.iso 2>/dev/null > SHA256SUMS || true
md5sum    *.img *.iso 2>/dev/null > MD5SUMS    || true
echo "   SHA256SUMS:"
cat SHA256SUMS | sed 's/^/   /'

# ── Step 4: Verify outputs ────────────────────────────────────────────────────
echo "▶ [4/5] Verifying outputs..."
IMG="${OUTPUT_DIR}/${OCR_NAME}.${OCR_VERSION}.img"
ISO="${OUTPUT_DIR}/${OCR_NAME}.${OCR_VERSION}.iso"

FAILED=0
[ -f "$IMG" ] && echo "   ✓ IMG: ${OCR_NAME}.${OCR_VERSION}.img  ($(du -h "$IMG" | cut -f1))" || { echo "   ✗ IMG missing!"; FAILED=1; }
[ -f "$ISO" ] && echo "   ✓ ISO: ${OCR_NAME}.${OCR_VERSION}.iso  ($(du -h "$ISO" | cut -f1))" || { echo "   ✗ ISO missing!"; FAILED=1; }

# ── Step 5: Summary ───────────────────────────────────────────────────────────
echo ""
echo "▶ [5/5] Build summary"
echo ""
echo "╔══════════════════════════════════════════════════════════════╗"
echo "║   OUTPUT FILES                                               ║"
ls -lh "${OUTPUT_DIR}"/*.img "${OUTPUT_DIR}"/*.iso 2>/dev/null | \
  awk '{printf "║   %-52s ║\n", $NF" ("$5")"}' || true
echo "╚══════════════════════════════════════════════════════════════╝"
echo ""
echo "  Flash to USB drive:"
echo "    sudo dd if=${OCR_NAME}.${OCR_VERSION}.img of=/dev/sdX bs=4M status=progress oflag=sync"
echo ""
echo "  Test in QEMU/KVM:"
echo "    qemu-system-x86_64 -m 512M -enable-kvm \\"
echo "      -drive file=${OCR_NAME}.${OCR_VERSION}.img,format=raw"
echo ""
echo "  Boot from ISO (CD/VM):"
echo "    qemu-system-x86_64 -m 512M -enable-kvm \\"
echo "      -cdrom ${OCR_NAME}.${OCR_VERSION}.iso -boot d"
echo ""

[ $FAILED -eq 0 ] && exit 0 || exit 1
