use crate::error::Error;
use crate::record::Record;

pub trait DataAdapter: Send + Sync {
    fn insert(&self, dataset_id: &str, collection: &str, record: Record) -> Result<Record, Error>;
    fn get(&self, dataset_id: &str, collection: &str, id: &str) -> Result<Option<Record>, Error>;
    fn update(&self, dataset_id: &str, collection: &str, id: &str, record: Record) -> Result<Record, Error>;
    fn delete(&self, dataset_id: &str, collection: &str, id: &str) -> Result<(), Error>;
    fn list(&self, dataset_id: &str, collection: &str) -> Result<Vec<Record>, Error>;
    fn delete_dataset(&self, dataset_id: &str) -> Result<(), Error>;
}
