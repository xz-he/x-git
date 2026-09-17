fn main() {
    // Tauri tracks its config, but Windows resources also depend on the ICO.
    println!("cargo:rerun-if-changed=icons/icon.ico");
    tauri_build::build();
}
