use std::env;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;

fn main() {
    let target = env::var("TARGET").unwrap_or_default();
    
    if target == "thumbv6m-none-eabi" {
        let out = &PathBuf::from(env::var_os("OUT_DIR").unwrap());
        let mut file = File::create(out.join("memory.x")).unwrap();
        file.write_all(b"
MEMORY {
    BOOT2 : ORIGIN = 0x10000000, LENGTH = 0x100
    FLASH : ORIGIN = 0x10000100, LENGTH = 2048K - 0x100
    RAM   : ORIGIN = 0x20000000, LENGTH = 264K
}
        ").unwrap();
        
        println!("cargo:rustc-link-search={}", out.display());
    }
}
