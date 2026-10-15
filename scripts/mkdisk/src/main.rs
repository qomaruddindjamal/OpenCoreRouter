//! opcr-mkdisk — OpenCR host-side disk image installer (Sprint IS-01).
//!
//! Builds a raw Rinjani disk image end-to-end:
//!   - Protective MBR + GPT (Rinjani 3-partition layout via asl-router-gpt)
//!   - Partition 1 formatted as FAT32 ESP (via asl-router-fat32) containing
//!     /boot/limine/limine.conf, /boot/limine/limine-bios.sys,
//!     /boot/rinjani-kernel.elf
//!   - Zero-filled RJCFG01 / RJDATA partitions
//! The Limine MBR boot stage is embedded afterwards by `limine bios-install`.

use std::fs;
use std::process;

use asl_router_fat32::{format_esp, EspFile, ESP_SECTORS, SECTOR_SIZE};
use asl_router_gpt::GptDiskLayout;

fn die(msg: &str) -> ! {
    eprintln!("opcr-mkdisk ERROR: {}", msg);
    process::exit(1);
}

fn arg(args: &[String], flag: &str) -> Option<String> {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let kernel = arg(&args, "--kernel").unwrap_or_else(|| die("--kernel <path> required"));
    let bios_sys = arg(&args, "--limine-bios-sys").unwrap_or_else(|| die("--limine-bios-sys <path> required"));
    let conf_text = arg(&args, "--limine-conf-text").unwrap_or_else(|| die("--limine-conf-text <inline text> required"));
    let out = arg(&args, "--out").unwrap_or_else(|| die("--out <raw image path> required"));
    let disk_mib: u64 = arg(&args, "--disk-mib")
        .unwrap_or_else(|| "2048".to_string())
        .parse()
        .unwrap_or_else(|_| die("--disk-mib must be an integer (MiB)"));

    let kernel_bytes = fs::read(&kernel).unwrap_or_else(|e| die(&format!("read kernel: {}", e)));
    let bios_bytes = fs::read(&bios_sys).unwrap_or_else(|e| die(&format!("read limine-bios.sys: {}", e)));
    // Validate ELF magic — refuse non-ELF payloads (evidence-first).
    if kernel_bytes[0..4] != [0x7F, b'E', b'L', b'F'] {
        die("kernel payload is not an ELF binary");
    }

    let total_sectors = disk_mib * 2048;
    if total_sectors < 1024 * 1024 {
        die("disk too small: minimum 512 MiB for Rinjani GPT layout");
    }

    // ---- Partition 1: ESP FAT32 with Limine boot tree ----
    let mut esp = vec![0u8; ESP_SECTORS as usize * SECTOR_SIZE];
    let conf_bytes = conf_text.as_bytes();
    let files = [
        EspFile { path: "boot/rinjani-kernel.elf", data: &kernel_bytes },
        EspFile { path: "boot/limine/limine-bios.sys", data: &bios_bytes },
        EspFile { path: "boot/limine/limine.conf", data: conf_bytes },
    ];
    let stats = format_esp(&mut esp, &files).unwrap_or_else(|e| die(&format!("format_esp: {:?}", e)));

    // ---- Whole-disk assembly ----
    let mut disk: Vec<u8> = Vec::with_capacity(total_sectors as usize * SECTOR_SIZE);
    for _ in 0..total_sectors as usize * SECTOR_SIZE {
        disk.push(0);
    }

    let mut layout = GptDiskLayout::create_default_layout(total_sectors)
        .unwrap_or_else(|e| die(&format!("gpt layout: {}", e)));

    let mut head = vec![0u8; 34 * SECTOR_SIZE];
    layout.write_head(&mut head).unwrap_or_else(|e| die(&format!("write_head: {}", e)));
    disk[..head.len()].copy_from_slice(&head);

    let esp_start = (layout.partitions[0].starting_lba as usize) * SECTOR_SIZE;
    let esp_end = (layout.partitions[0].ending_lba as usize + 1) * SECTOR_SIZE;
    if esp_end - esp_start != esp.len() {
        die("ESP partition size mismatch with formatter buffer");
    }
    disk[esp_start..esp_end].copy_from_slice(&esp);

    let mut tail = vec![0u8; 33 * SECTOR_SIZE];
    layout.write_backup(&mut tail).unwrap_or_else(|e| die(&format!("write_backup: {}", e)));
    let tail_at = (total_sectors as usize - 33) * SECTOR_SIZE;
    disk[tail_at..tail_at + tail.len()].copy_from_slice(&tail);

    fs::write(&out, &disk).unwrap_or_else(|e| die(&format!("write {}: {}", out, e)));

    println!("opcr-mkdisk OK");
    println!("  disk        : {} ({} MiB, {} sectors)", out, disk_mib, total_sectors);
    println!("  ESP FAT32   : fat_sectors={}, allocatable_clusters={}, used={}",
        stats.fat_sectors, stats.total_clusters, stats.clusters_used);
    println!("  kernel elf  : {} bytes", kernel_bytes.len());
    println!("  limine sys  : {} bytes", bios_bytes.len());
    println!("  limine conf : {} bytes", conf_bytes.len());
}
