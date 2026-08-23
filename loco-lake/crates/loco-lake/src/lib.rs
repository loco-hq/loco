pub mod adapter;
pub mod adapters;
pub mod error;
pub mod record;
pub mod value;

pub use adapter::DataAdapter;
pub use adapters::memory::InMemoryAdapter;
pub use adapters::sqlite::SqliteAdapter;
pub use error::Error;
pub use record::{InsertRequest, Record, UpdatePatch};
pub use value::Value;
