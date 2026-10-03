//! Dataset-scoped secret and variable values.
//!
//! Declarations live on a version (`schemas/types/secret.yaml`,
//! `variable.yaml`) and are what `/schema` serves. A value belongs to one
//! dataset — two stores install the same package and must not share a
//! credential — and is written only through `/config`. See [`store`] for the
//! path encoding and [`seal`] for `LOCO_SECRET_KEY`.

mod seal;
mod store;

pub use seal::parse_secret_key;
pub use seal::KeyStatus;
pub use seal::OpenError;
pub use store::ConfigValueStore;
