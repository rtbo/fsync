use std::{fs, path::Path};

use fsync_client::ts::Types;
use typescript_type_def::{write_definition_file, DefinitionFileOptions};

fn main() {
    // building ts definitions
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("frontend")
        .join("src")
        .join("lib")
        .join("types.d.ts");
    let writer = fs::File::create(&path).unwrap();
    let options = DefinitionFileOptions::default();
    write_definition_file::<_, Types>(writer, options).unwrap();

    // building tauri app
    tauri_build::build()
}
