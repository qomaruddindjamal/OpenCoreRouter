# =============================================================================
# Open Core Router — Master Makefile
# Version: 1.0.1
# Based on: OpenWrt 25.12.5 x86-64
# =============================================================================
#
# Targets:
#   make build        — Build IMG + ISO via Docker (recommended)
#   make build-native — Build directly on Linux host (without Docker)
#   make daemon       — Compile Go daemon only (cross-compile for Linux)
#   make docker       — Build Docker build image
#   make clean        — Remove build artifacts
#   make push         — Push source to GitHub (excludes images)
#   make help         — Show this help
#
# Output:
#   build/output/Open-Core-Router.1.0.1.img
#   build/output/Open-Core-Router.1.0.1.iso
# =============================================================================

.PHONY: all build build-native daemon docker clean push help

# ── Variables ─────────────────────────────────────────────────────────────────
OCR_NAME        := Open-Core-Router
OCR_VERSION     := 1.0.1
OPENWRT_VERSION := 25.12.5

DOCKER_IMAGE    := ocr-builder:$(OCR_VERSION)
OUTPUT_DIR      := build/output
DAEMON_DIR      := daemon
SCRIPTS_DIR     := scripts

IMG := $(OUTPUT_DIR)/$(OCR_NAME).$(OCR_VERSION).img
ISO := $(OUTPUT_DIR)/$(OCR_NAME).$(OCR_VERSION).iso

# Detect OS
UNAME := $(shell uname -s 2>/dev/null || echo Windows)

# ── Default ───────────────────────────────────────────────────────────────────
all: help

# ── Build via Docker (works on Windows/macOS/Linux) ──────────────────────────
build: docker
	@echo ""
	@echo "▶ Building $(OCR_NAME) $(OCR_VERSION) via Docker..."
	@mkdir -p $(OUTPUT_DIR)
	docker run --rm \
		--privileged \
		--name ocr-build \
		-v "$(PWD):/opencr" \
		-v "$(PWD)/build/dl:/opencr/build/dl" \
		$(DOCKER_IMAGE) \
		--iso
	@echo ""
	@echo "✓ Build complete — see $(OUTPUT_DIR)/"

# ── Build directly on Linux host ──────────────────────────────────────────────
build-native:
	@echo "▶ Building $(OCR_NAME) $(OCR_VERSION) natively (Linux only)..."
	@bash $(SCRIPTS_DIR)/build-image.sh --iso

# ── Compile Go daemon (cross-compile for OpenWrt x86-64) ─────────────────────
daemon:
	@echo "▶ Compiling ocrd daemon for linux/amd64..."
	@mkdir -p $(DAEMON_DIR)/bin
	cd $(DAEMON_DIR) && \
	  GOOS=linux GOARCH=amd64 CGO_ENABLED=0 \
	  go build \
	    -ldflags="-s -w \
	      -X main.Version=$(OCR_VERSION) \
	      -X main.BuildDate=$$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
	    -o bin/ocrd \
	    ./cmd/ocrd
	@echo "✓ daemon/bin/ocrd  ($$(du -h $(DAEMON_DIR)/bin/ocrd | cut -f1))"

# ── Build Docker builder image ────────────────────────────────────────────────
docker:
	@echo "▶ Building Docker build image: $(DOCKER_IMAGE)..."
	docker build \
		-f $(SCRIPTS_DIR)/Dockerfile.build \
		-t $(DOCKER_IMAGE) \
		--build-arg OCR_VERSION=$(OCR_VERSION) \
		.
	@echo "✓ Docker image ready: $(DOCKER_IMAGE)"

# ── QEMU test boot ────────────────────────────────────────────────────────────
qemu-img:
	@echo "▶ Booting $(IMG) in QEMU..."
	qemu-system-x86_64 \
		-m 512M \
		-enable-kvm \
		-nographic \
		-serial mon:stdio \
		-drive file=$(IMG),format=raw \
		-net nic,model=virtio \
		-net user,hostfwd=tcp::8080-:80,hostfwd=tcp::2222-:22

qemu-iso:
	@echo "▶ Booting $(ISO) in QEMU..."
	qemu-system-x86_64 \
		-m 512M \
		-enable-kvm \
		-nographic \
		-serial mon:stdio \
		-cdrom $(ISO) \
		-boot d \
		-net nic,model=virtio \
		-net user

# ── Push source to GitHub (no images) ────────────────────────────────────────
push:
	@echo "▶ Pushing source to GitHub..."
	@echo "  Note: *.img, *.iso excluded via .gitignore"
	git add -A
	git commit -m "build: $(OCR_NAME) $(OCR_VERSION)" || echo "Nothing to commit"
	git push origin master

# ── Clean ─────────────────────────────────────────────────────────────────────
clean:
	@echo "▶ Cleaning build artifacts..."
	rm -rf $(OUTPUT_DIR)
	rm -rf build/imagebuilder/files
	rm -rf build/iso_work build/mnt_*
	rm -f  daemon/bin/ocrd
	@echo "✓ Clean done (DL cache preserved in build/dl/)"

clean-all: clean
	rm -rf build/dl build/imagebuilder

# ── Help ──────────────────────────────────────────────────────────────────────
help:
	@echo ""
	@echo "  ╔══════════════════════════════════════════════════════════╗"
	@echo "  ║     Open Core Router $(OCR_VERSION) — Build System            ║"
	@echo "  ║     Based on OpenWrt $(OPENWRT_VERSION) x86-64                    ║"
	@echo "  ╚══════════════════════════════════════════════════════════╝"
	@echo ""
	@echo "  Targets:"
	@echo "    make build         Build IMG + ISO via Docker (Windows/Linux/macOS)"
	@echo "    make build-native  Build directly on Linux host"
	@echo "    make daemon        Compile Go daemon only"
	@echo "    make docker        Build Docker builder image only"
	@echo "    make qemu-img      Test boot IMG in QEMU"
	@echo "    make qemu-iso      Test boot ISO in QEMU"
	@echo "    make push          Push source to GitHub (no images)"
	@echo "    make clean         Remove build output (keep DL cache)"
	@echo "    make clean-all     Remove everything including DL cache"
	@echo ""
	@echo "  Output:"
	@echo "    $(IMG)"
	@echo "    $(ISO)"
	@echo ""
	@echo "  Quick start:"
	@echo "    make build"
	@echo "    sudo dd if=$(IMG) of=/dev/sdX bs=4M status=progress"
	@echo ""
