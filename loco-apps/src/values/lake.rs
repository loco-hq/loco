//! [`LakeSecretStore`]: ciphertext in the lake, under [`super::SECRETS`].
//!
//! The record id is the declaration reference. Fields are `nonce` and
//! `ciphertext`, both standard base64. The plaintext is not a field.

use std::collections::HashMap;
use std::sync::Arc;

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use loco_lake::{DataAdapter, Record, Value};

use super::seal::{open, seal, secret_aad, OpenError};
use super::{check_name, meta_from, upsert, SecretError, SecretMeta, SecretStore, SECRETS};
use crate::values::KeyStatus;

const NONCE: &str = "nonce";
const CIPHERTEXT: &str = "ciphertext";

pub struct LakeSecretStore {
    data: Arc<dyn DataAdapter>,
    key: KeyStatus,
}

impl LakeSecretStore {
    pub fn new(data: Arc<dyn DataAdapter>, key: KeyStatus) -> Self {
        Self { data, key }
    }
}

impl SecretStore for LakeSecretStore {
    fn put(
        &self,
        dataset_id: &str,
        name: &str,
        plaintext: &str,
    ) -> Result<SecretMeta, SecretError> {
        let key = self.key.require().map_err(SecretError::Unavailable)?;
        check_name(name).map_err(SecretError::InvalidName)?;
        let sealed = seal(key, &secret_aad(dataset_id, name), plaintext.as_bytes())
            .map_err(|_| SecretError::Failed("aes-256-gcm encrypt failed".to_string()))?;
        let mut fields = HashMap::new();
        fields.insert(
            NONCE.to_string(),
            Value::String(STANDARD.encode(sealed.nonce)),
        );
        fields.insert(
            CIPHERTEXT.to_string(),
            Value::String(STANDARD.encode(sealed.ciphertext)),
        );
        let record = upsert(self.data.as_ref(), dataset_id, SECRETS, name, fields)?;
        Ok(meta_from(&record))
    }

    fn get(&self, dataset_id: &str, name: &str) -> Result<Option<String>, SecretError> {
        check_name(name).map_err(SecretError::InvalidName)?;
        let Some(record) = self.data.get(dataset_id, SECRETS, name)? else {
            return Ok(None);
        };
        let key = self.key.require().map_err(SecretError::Unavailable)?;
        let plaintext = decrypt(key, dataset_id, name, &record)?;
        Ok(Some(plaintext))
    }

    fn delete(&self, dataset_id: &str, name: &str) -> Result<(), SecretError> {
        check_name(name).map_err(SecretError::InvalidName)?;
        self.data.delete(dataset_id, SECRETS, name)?;
        Ok(())
    }

    fn list(&self, dataset_id: &str) -> Result<Vec<SecretMeta>, SecretError> {
        let mut rows: Vec<SecretMeta> = self
            .data
            .list(dataset_id, SECRETS)?
            .iter()
            .map(meta_from)
            .collect();
        rows.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(rows)
    }

    fn delete_dataset(&self, dataset_id: &str) -> Result<(), SecretError> {
        let names: Vec<String> = self
            .data
            .list(dataset_id, SECRETS)?
            .into_iter()
            .map(|record| record.id)
            .collect();
        let mut first = None;
        for name in names {
            if let Err(err) = self.data.delete(dataset_id, SECRETS, &name) {
                if first.is_none() {
                    first = Some(SecretError::from(err));
                }
            }
        }
        match first {
            Some(err) => Err(err),
            None => Ok(()),
        }
    }
}

fn decrypt(
    key: &[u8; 32],
    dataset_id: &str,
    name: &str,
    record: &Record,
) -> Result<String, SecretError> {
    let nonce = field_bytes(record, NONCE)?;
    let ciphertext = field_bytes(record, CIPHERTEXT)?;
    match open(key, &secret_aad(dataset_id, name), &nonce, &ciphertext) {
        Ok(bytes) => String::from_utf8(bytes)
            .map_err(|_| SecretError::Failed("secret plaintext was not utf-8".to_string())),
        Err(OpenError::Failed) => Err(SecretError::Failed(
            "secret ciphertext failed to decrypt".to_string(),
        )),
    }
}

fn field_bytes(record: &Record, field: &str) -> Result<Vec<u8>, SecretError> {
    let Some(Value::String(text)) = record.fields.get(field) else {
        return Err(SecretError::Failed(format!(
            "secret record {} has no {field}",
            record.id
        )));
    };
    STANDARD
        .decode(text)
        .map_err(|_| SecretError::Failed(format!("secret record {} has a bad {field}", record.id)))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use loco_lake::{DataAdapter, InMemoryAdapter, SqliteAdapter};

    use super::*;
    use crate::values::{KeyStatus, SecretStore, VARIABLES};

    const DATASET: &str = "ben/crm/dev";
    const NAME: &str = "alice/bricklink.consumer_key";
    const PLAIN: &str = "s3cret-value";
    const KEY: [u8; 32] = [4u8; 32];

    fn ready(data: Arc<dyn DataAdapter>) -> LakeSecretStore {
        LakeSecretStore::new(data, KeyStatus::Ready(KEY))
    }

    fn assert_plaintext_absent(data: Arc<dyn DataAdapter>) {
        let store = ready(data.clone());
        store.put(DATASET, NAME, PLAIN).unwrap();
        let record = data.get(DATASET, SECRETS, NAME).unwrap().unwrap();
        let stored = serde_json::to_string(&record.fields).unwrap();
        assert!(!stored.contains(PLAIN), "{stored}");
        assert!(!record.fields.contains_key("value"));
        assert_eq!(record.id, NAME);
        assert_eq!(store.get(DATASET, NAME).unwrap().as_deref(), Some(PLAIN));

        let wrong = LakeSecretStore::new(data, KeyStatus::Ready([9u8; 32]));
        let err = wrong.get(DATASET, NAME).unwrap_err();
        assert!(matches!(err, SecretError::Failed(_)), "{err}");
    }

    #[test]
    fn plaintext_is_not_in_the_stored_record_and_the_wrong_key_fails() {
        assert_plaintext_absent(Arc::new(InMemoryAdapter::new()));
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("lake.db");
        assert_plaintext_absent(Arc::new(SqliteAdapter::new(&path).unwrap()));
        let raw = std::fs::read(&path).unwrap();
        let raw = String::from_utf8_lossy(&raw);
        assert!(!raw.contains(PLAIN), "sqlite file contains the plaintext");
    }

    #[test]
    fn missing_key_writes_nothing() {
        let data = Arc::new(InMemoryAdapter::new());
        let store = LakeSecretStore::new(data.clone(), KeyStatus::Missing);
        let err = store.put(DATASET, NAME, PLAIN).unwrap_err();
        assert!(
            matches!(err, SecretError::Unavailable(ref msg) if msg.contains("LOCO_SECRET_KEY"))
        );
        assert!(data.get(DATASET, SECRETS, NAME).unwrap().is_none());
    }

    #[test]
    fn delete_dataset_removes_only_secrets_in_that_dataset() {
        let data = Arc::new(InMemoryAdapter::new());
        let store = ready(data.clone());
        store.put(DATASET, NAME, PLAIN).unwrap();
        store.put("ben/crm/other", "api_base", "keep").unwrap();
        let mut fields = std::collections::HashMap::new();
        fields.insert("value".into(), loco_lake::Value::String("stay".into()));
        data.upsert(DATASET, VARIABLES, "label", "system", fields)
            .unwrap();

        store.delete_dataset(DATASET).unwrap();

        assert!(store.get(DATASET, NAME).unwrap().is_none());
        assert_eq!(
            store.get("ben/crm/other", "api_base").unwrap().as_deref(),
            Some("keep")
        );
        assert!(data.get(DATASET, VARIABLES, "label").unwrap().is_some());
    }

    #[test]
    fn two_puts_replace_the_same_row() {
        let data = Arc::new(InMemoryAdapter::new());
        let store = ready(data.clone());
        store.put(DATASET, NAME, "one").unwrap();
        store.put(DATASET, NAME, "two").unwrap();
        assert_eq!(data.list(DATASET, SECRETS).unwrap().len(), 1);
        assert_eq!(store.get(DATASET, NAME).unwrap().as_deref(), Some("two"));
        let stored =
            serde_json::to_string(&data.get(DATASET, SECRETS, NAME).unwrap().unwrap().fields)
                .unwrap();
        assert!(!stored.contains("one"));
        assert!(!stored.contains("two"));
    }
}
