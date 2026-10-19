//! Locating the bundled subagent extension (`extensions/pi-subagents`).
//!
//! PF spawns the vendored pi with `-e <path>` when the extension is present, the
//! same way `pf_providers` mounts its provider extension (051). The extension is
//! plain TypeScript: pi loads it with jiti, so no build step is involved.
//!
//! Search order:
//! 1. `PI_FLASH_SUBAGENTS_EXT` (explicit override; disabling by `PI_FLASH_SUBAGENTS=0`)
//! 2. next to the executable (`<exe_dir>/extensions/pi-subagents`, release layout)
//! 3. one level up (`target/debug/../..`, `cargo run` during dev)
//! 4. derived from the vendored pi directory (`vendor/pi` → repo root), the
//!    layout used by `cargo run` and by the macOS `.app` bundle

use std::path::{Path, PathBuf};

const REL: &str = "extensions/pi-subagents/src/index.ts";

/// Resolve the extension entry file, if this build ships it.
pub fn ext_path() -> Option<PathBuf> {
    if let Ok(flag) = std::env::var("PI_FLASH_SUBAGENTS") {
        if flag == "0" || flag.eq_ignore_ascii_case("false") {
            return None;
        }
    }
    if let Ok(explicit) = std::env::var("PI_FLASH_SUBAGENTS_EXT") {
        let p = PathBuf::from(explicit);
        return p.is_file().then_some(p);
    }
    for base in candidates() {
        let p = base.join(REL);
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

fn candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            out.push(dir.to_path_buf());
            if let Some(up) = dir.parent() {
                out.push(up.to_path_buf());
                // target/debug/<exe> → repo root is two levels up.
                if let Some(root) = up.parent() {
                    out.push(root.to_path_buf());
                }
            }
        }
    }
    if let Some(vendor) = crate::vendor::vendor_dir() {
        // <root>/vendor/pi → <root>
        if let Some(root) = vendor.parent().and_then(Path::parent) {
            out.push(root.to_path_buf());
        }
    }
    out
}
