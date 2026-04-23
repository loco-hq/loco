# loco-gen-codegen-build

Build script API for generating Rust code from YAML schemas. This is the entry point that `build.rs` calls.

## Usage

In your crate's `build.rs`:

```rust
fn main() {
    loco_gen_codegen_build::generate("schemas/types");
}
```

Then include the generated code in your `main.rs` or `lib.rs`:

```rust
include!(concat!(env!("OUT_DIR"), "/loco_generated.rs"));
```

## What `generate()` Does

1. Reads all `.yaml` files from the types directory
2. Parses each into a `TypeDef` via `loco-gen-schema`
3. Generates Rust source code (structs, constructors, accessors)
4. Writes the output to `$OUT_DIR/loco_generated.rs`
5. Emits `cargo:rerun-if-changed` directives so Cargo rebuilds when schemas change

## Dependencies

This crate depends on `loco-gen-schema` for parsing and codegen. It is a **build dependency only** — it does not appear in the final binary.
