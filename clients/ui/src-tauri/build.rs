use std::{fs, path::Path};

use typescript_type_def::{write_definition_file, DefinitionFileOptions};

type Api = (
    fsync::Provider,
    fsync_client::drive::SecretOpts,
    fsync_client::drive::Opts,
    fsync_client::new::ProviderOpts,
);

fn main() {
    // building ts definitions
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("src")
        .join("lib")
        .join("types.d.ts");
    let writer = fs::File::create(&path).unwrap();
    let options = DefinitionFileOptions::default();
    write_definition_file::<_, Api>(writer, options).unwrap();

    // building tauri app
    tauri_build::build()
}
