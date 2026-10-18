//! Embed `assets/icon/nouwall.ico` as icon group 1 in the Windows binaries.
//!
//! The `.res` file is written directly (no rc.exe / llvm-rc needed), so the
//! same build works with MSVC link.exe and with lld-link under cargo-xwin.

use std::env;
use std::fs;
use std::path::PathBuf;

const ICON: &str = "assets/icon/nouwall.ico";
const RT_ICON: u16 = 3;
const RT_GROUP_ICON: u16 = 14;
const LANG_EN_US: u16 = 0x0409;

fn main() {
    println!("cargo:rerun-if-changed={ICON}");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let ico = fs::read(ICON).expect("read icon");
    let res = icon_res(&ico);
    let out = PathBuf::from(env::var("OUT_DIR").unwrap()).join("nouwall-icon.res");
    fs::write(&out, res).expect("write .res");
    println!("cargo:rustc-link-arg-bins={}", out.display());
}

fn u16_at(b: &[u8], i: usize) -> u16 {
    u16::from_le_bytes([b[i], b[i + 1]])
}

fn u32_at(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
}

/// One resource entry with ordinal type and name.
fn entry(out: &mut Vec<u8>, kind: u16, name: u16, flags: u16, data: &[u8]) {
    out.extend((data.len() as u32).to_le_bytes());
    out.extend(32u32.to_le_bytes()); // header size
    out.extend([0xFF, 0xFF]);
    out.extend(kind.to_le_bytes());
    out.extend([0xFF, 0xFF]);
    out.extend(name.to_le_bytes());
    out.extend(0u32.to_le_bytes()); // data version
    out.extend(flags.to_le_bytes());
    out.extend(LANG_EN_US.to_le_bytes());
    out.extend(0u32.to_le_bytes()); // version
    out.extend(0u32.to_le_bytes()); // characteristics
    out.extend(data);
    while !out.len().is_multiple_of(4) {
        out.push(0);
    }
}

/// Convert an `.ico` file into RT_ICON entries plus one RT_GROUP_ICON.
fn icon_res(ico: &[u8]) -> Vec<u8> {
    assert!(ico.len() >= 6 && u16_at(ico, 2) == 1, "not an .ico file");
    let count = u16_at(ico, 4) as usize;
    let mut res = Vec::new();
    // A .res file starts with an empty 32-byte entry.
    res.extend([0, 0, 0, 0, 32, 0, 0, 0, 0xFF, 0xFF, 0, 0, 0xFF, 0xFF, 0, 0]);
    res.extend([0u8; 16]);

    let mut group = Vec::new();
    group.extend(0u16.to_le_bytes());
    group.extend(1u16.to_le_bytes());
    group.extend((count as u16).to_le_bytes());
    for i in 0..count {
        let e = 6 + i * 16;
        let size = u32_at(ico, e + 8) as usize;
        let offset = u32_at(ico, e + 12) as usize;
        let id = (i + 1) as u16;
        entry(&mut res, RT_ICON, id, 0x1010, &ico[offset..offset + size]);
        // GRPICONDIRENTRY: the ICONDIRENTRY fields with the offset replaced by the id.
        group.extend(&ico[e..e + 12]);
        group.extend(id.to_le_bytes());
    }
    entry(&mut res, RT_GROUP_ICON, 1, 0x1030, &group);
    res
}
