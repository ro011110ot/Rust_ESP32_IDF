use std::env;
use std::path::PathBuf;

fn main() {
    // WICHTIG: ESP-IDF Build-System initialisieren
    embuild::espidf::sysenv::output();

    // Finde Workspace-Root
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let workspace_root = PathBuf::from(&manifest_dir).parent().unwrap().to_path_buf();

    let secrets_path = workspace_root.join("secrets.toml");

    println!("cargo:rerun-if-changed={}", secrets_path.display());

    // Prüfe ob secrets.toml existiert
    if !secrets_path.exists() {
        panic!(
            "\n\n\
            ❌ ERROR: secrets.toml nicht gefunden!\n\
            \n\
            Erwartet in: {}\n\
            \n\
            Erstelle die Datei:\n\
            cp secrets.toml.example secrets.toml\n\
            ",
            secrets_path.display()
        );
    }
}
