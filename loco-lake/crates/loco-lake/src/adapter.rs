use crate::error::Error;
use crate::record::Record;

pub trait DataAdapter: Send + Sync {
    fn insert(&self, tenant_id: &str, collection: &str, record: Record) -> Result<Record, Error>;
    fn get(&self, tenant_id: &str, collection: &str, id: &str) -> Result<Option<Record>, Error>;
    fn update(&self, tenant_id: &str, collection: &str, id: &str, record: Record) -> Result<Record, Error>;
    fn delete(&self, tenant_id: &str, collection: &str, id: &str) -> Result<(), Error>;
    fn list(&self, tenant_id: &str, collection: &str) -> Result<Vec<Record>, Error>;
}
