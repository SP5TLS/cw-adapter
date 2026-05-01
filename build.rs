use std::env;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;

fn main() {
    let target = env::var("TARGET").unwrap_or_default();
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());

    let memory_x: Option<&[u8]> = match target.as_str() {
        "thumbv6m-none-eabi" => Some(b"
MEMORY {
    BOOT2 : ORIGIN = 0x10000000, LENGTH = 0x100
    FLASH : ORIGIN = 0x10000100, LENGTH = 2048K - 0x100
    RAM   : ORIGIN = 0x20000000, LENGTH = 264K
}
        "),
        // RP2350 has no second-stage bootloader, but the BOOTROM scans the
        // start of flash for an IMAGE_DEF block. We reserve the first 0x100
        // bytes for it (mirroring how RP2040 reserves space for boot2) so
        // that cortex-m-rt's `.vector_table` lands cleanly behind the block.
        // SRAM is 520 KiB (8 banks of 64 KiB + 2 banks of 4 KiB).
        "thumbv8m.main-none-eabihf" => Some(b"
MEMORY {
    START_BLOCK : ORIGIN = 0x10000000, LENGTH = 0x100
    FLASH       : ORIGIN = 0x10000100, LENGTH = 2048K - 0x100
    RAM         : ORIGIN = 0x20000000, LENGTH = 520K
}
        "),
        _ => None,
    };

    if let Some(content) = memory_x {
        let mut file = File::create(out.join("memory.x")).unwrap();
        file.write_all(content).unwrap();
        println!("cargo:rustc-link-search={}", out.display());
    }

    if target == "thumbv8m.main-none-eabihf" {
        // Project-root linker fragment for RP2350 — see link-rp235x.x.
        let manifest = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
        println!("cargo:rustc-link-search={}", manifest.display());
        println!("cargo:rerun-if-changed=link-rp235x.x");
    }
}
