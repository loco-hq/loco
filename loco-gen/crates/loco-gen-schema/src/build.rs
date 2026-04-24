use std::path::Path;

/// Scan `types_dir` for `.yaml` type definitions and generate Rust structs,
/// writing the output to `$OUT_DIR/loco_generated.rs`.
///
/// Call this from your crate's `build.rs`.
pub fn generate(types_dir: &str) {
    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR not set");
    let out_path = Path::new(&out_dir).join("loco_generated.rs");
    let types_path = Path::new(types_dir);

    println!("cargo:rerun-if-changed={types_dir}");

    let mut type_defs = Vec::new();

    let entries = std::fs::read_dir(types_path)
        .unwrap_or_else(|e| panic!("failed to read types dir '{}': {}", types_dir, e));

    for entry in entries {
        let entry = entry.expect("failed to read directory entry");
        let path = entry.path();

        if path.extension().and_then(|e| e.to_str()) == Some("yaml") {
            println!("cargo:rerun-if-changed={}", path.display());

            let type_def = crate::parser::parse_schema_file(&path)
                .unwrap_or_else(|e| panic!("failed to parse {}: {}", path.display(), e));

            type_defs.push(type_def);
        }
    }

    type_defs.sort_by(|a, b| a.name.cmp(&b.name));

    let code = crate::codegen::generate_all(&type_defs);
    std::fs::write(&out_path, code)
        .unwrap_or_else(|e| panic!("failed to write {}: {}", out_path.display(), e));
}
