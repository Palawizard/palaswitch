use std::path::Path;

use embed_manifest::{embed_manifest, new_manifest};

const ICON: &str = "../../assets/palaswitch.ico";
/// The settings window loads the icon by this resource id.
const ICON_ID: &str = "1";

fn main() {
    if std::env::var_os("CARGO_CFG_WINDOWS").is_some() {
        embed_manifest(new_manifest("Palawizard.palaswitch")).expect("unable to embed manifest");
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").expect("set by cargo");
        let icon = Path::new(&manifest_dir).join(ICON);
        winresource::WindowsResource::new()
            .set_icon_with_id(icon.to_str().expect("icon path is valid UTF-8"), ICON_ID)
            .set("CompanyName", "Palawi")
            .set("LegalCopyright", "Copyright (c) 2026 Palawi")
            .set("OriginalFilename", "palaswitch.exe")
            .compile()
            .expect("unable to embed the icon");
    }
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed={ICON}");
}
