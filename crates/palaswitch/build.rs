use embed_manifest::{embed_manifest, new_manifest};

const ICON: &str = "../../assets/palaswitch.ico";

fn main() {
    if std::env::var_os("CARGO_CFG_WINDOWS").is_some() {
        embed_manifest(new_manifest("Palawizard.palaswitch")).expect("unable to embed manifest");
        winresource::WindowsResource::new()
            .set_icon(ICON)
            .compile()
            .expect("unable to embed the icon");
    }
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed={ICON}");
}
