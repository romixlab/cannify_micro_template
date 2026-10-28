use std::path::PathBuf;
use std::{env, fs};

fn main() {
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    // Put memory.x on the linker search path (link.x from cortex-m-rt INCLUDEs it)
    fs::write(out.join("memory.x"), include_bytes!("memory.x")).unwrap();
    println!("cargo:rustc-link-search={}", out.display());
    println!("cargo:rerun-if-changed=memory.x");

    println!("cargo:rustc-link-arg=--nmagic");
    println!("cargo:rustc-link-arg=-Tlink.x");
    println!("cargo:rustc-link-arg=-Tdefmt.x");
    println!("cargo:rustc-link-arg=-Tcnt.x"); // counters index allocation (cnt crate)
    // GNU build id: the firmware's identity (memory.x keeps it in flash), a host finds the ELF by it
    println!("cargo:rustc-link-arg=--build-id=sha1");

    // Name, version, git commit and build time for ww_firmware_info::firmware_summary!() (FirmwareInfo in src/ww.rs)
    ww_firmware_info::build::emit();
}
