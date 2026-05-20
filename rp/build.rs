use std::env;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;

fn main() {
    let target = env::var("TARGET").unwrap_or_default();
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());

    let memory_x: Option<&[u8]> = match target.as_str() {
        "thumbv6m-none-eabi" => Some(
            b"
MEMORY {
    BOOT2 : ORIGIN = 0x10000000, LENGTH = 0x100
    FLASH : ORIGIN = 0x10000100, LENGTH = 2048K - 0x100
    RAM   : ORIGIN = 0x20000000, LENGTH = 264K
}
        ",
        ),
        // RP2350: vector_table MUST live at the start of FLASH (0x10000000),
        // because the BOOTROM, after validating the IMAGE_DEF block found
        // anywhere in the first 4 KiB of flash, jumps to 0x10000000 and
        // interprets the first words as the standard ARMv8-M vector table
        // (initial MSP at +0, Reset handler at +4). The IMAGE_DEF block
        // therefore has to go AFTER the vector table, not before.
        //
        // Layout matches embassy-rs/embassy examples/rp235x/memory.x — the
        // section directives are in this file (not a separate fragment)
        // because cortex-m-rt's link.x includes memory.x BEFORE defining
        // `.text _stext :`, so our `_stext` override is in scope when
        // `.text`'s start address is resolved.
        //
        // SRAM0..7 = 512K striped. SRAM8/9 (4K direct-mapped each) intentionally
        // omitted — nothing links into them; declaring them only invites confusion.
        // Pico 2 flash is 2 MiB; raise LENGTH if you ever target an RP2350B board.
        "thumbv8m.main-none-eabihf" => Some(
            br#"
MEMORY {
    FLASH : ORIGIN = 0x10000000, LENGTH = 2048K
    RAM   : ORIGIN = 0x20000000, LENGTH = 512K
}

SECTIONS {
    .start_block : ALIGN(4)
    {
        __start_block_addr = .;
        KEEP(*(.start_block));
        KEEP(*(.boot_info));
    } > FLASH
} INSERT AFTER .vector_table;

_stext = ADDR(.start_block) + SIZEOF(.start_block);

SECTIONS {
    .end_block : ALIGN(4)
    {
        __end_block_addr = .;
        KEEP(*(.end_block));
    } > FLASH
} INSERT AFTER .uninit;

PROVIDE(start_to_end = __end_block_addr - __start_block_addr);
PROVIDE(end_to_start = __start_block_addr - __end_block_addr);
"#,
        ),
        _ => None,
    };

    if let Some(content) = memory_x {
        let mut file = File::create(out.join("memory.x")).unwrap();
        file.write_all(content).unwrap();
        println!("cargo:rustc-link-search={}", out.display());
    }
}
