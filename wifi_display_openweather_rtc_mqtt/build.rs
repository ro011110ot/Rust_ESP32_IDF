use std::env;
use std::path::PathBuf;

fn main() {
    // IMPORTANT: Initialize the ESP-IDF build system
    embuild::espidf::sysenv::output();

    // Find the workspace root
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let workspace_root = PathBuf::from(&manifest_dir).parent().unwrap().to_path_buf();

    // Path to the secrets.toml file
    let secrets_path = workspace_root.join("secrets.toml");

    // Rerun the build script if secrets.toml changes
    println!("cargo:rerun-if-changed={}", secrets_path.display());

    // Check if secrets.toml exists
    if !secrets_path.exists() {
        panic!(
            "\n\n\
            ❌ ERROR: secrets.toml not found!\n\
            \n\
            Expected in: {}\n\
            \n\
            Create the file:\n\
            cp secrets.toml.example secrets.toml\n\
            ",
            secrets_path.display()
        );
    }
}
