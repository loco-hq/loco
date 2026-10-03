use std::collections::HashMap;

use crate::error::Error;
use crate::query::{LakeQuery, Page};
use crate::record::{InsertRequest, Record, UpdatePatch};
use crate::value::Value;

pub trait DataAdapter: Send + Sync {
    fn insert(
        &self,
        dataset_id: &str,
        collection: &str,
        req: InsertRequest,
    ) -> Result<Record, Error>;
    /// Insert or replace the record `id`. A new row is stamped like `insert`,
    /// with this id instead of a generated one. An existing row keeps
    /// `created_*`, `dataset_id`, and `owner`, and `fields` are merged the
    /// way `update` merges them.
    ///
    /// One critical section: the check and the write share the adapter's
    /// lock. `insert` cannot do this, because it always mints a new id.
    fn upsert(
        &self,
        dataset_id: &str,
        collection: &str,
        id: &str,
        user: &str,
        fields: HashMap<String, Value>,
    ) -> Result<Record, Error>;
    fn get(&self, dataset_id: &str, collection: &str, id: &str) -> Result<Option<Record>, Error>;
    fn update(
        &self,
        dataset_id: &str,
        collection: &str,
        id: &str,
        patch: UpdatePatch,
    ) -> Result<Record, Error>;
    fn delete(&self, dataset_id: &str, collection: &str, id: &str) -> Result<(), Error>;
    fn list(&self, dataset_id: &str, collection: &str) -> Result<Vec<Record>, Error>;
    fn delete_dataset(&self, dataset_id: &str) -> Result<(), Error>;
    /// Run every query against one consistent snapshot of `dataset_id`.
    /// Results are in the same order as `queries`. Semantics: `query.rs`.
    fn query(&self, dataset_id: &str, queries: &[LakeQuery]) -> Result<Vec<Page>, Error>;
}
