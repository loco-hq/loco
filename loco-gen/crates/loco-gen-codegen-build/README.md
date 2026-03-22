# loco-gen-codegen-build

Build script API for generating Rust code from YAML schemas. This is the entry point that `build.rs` calls.

## Usage

In your crate's `build.rs`:

```rust
fn main() {
    loco_gen_codegen_build::generate("schemas/types", "schemas/instances");
}
```

Then include the generated code in your `main.rs` or `lib.rs`:

```rust
include!(concat!(env!("OUT_DIR"), "/loco_generated.rs"));
```

## What `generate()` Does

1. Reads all `.yaml` files from the types directory
2. Parses each into a `TypeDef` via `loco-gen-schema`
3. Recursively scans the instances directory, validating each instance against its type
4. Generates Rust source code (structs, constructors, accessors, instance loaders)
5. Writes the output to `$OUT_DIR/loco_generated.rs`
6. Emits `cargo:rerun-if-changed` directives so Cargo rebuilds when schemas change

## Dependencies

This crate depends on `loco-gen-schema` for parsing and codegen. It is a **build dependency only** — it does not appear in the final binary.
