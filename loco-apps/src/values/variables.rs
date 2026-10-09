//! [`LakeVariableStore`]: plaintext in the lake, under [`super::VARIABLES`].
//!
//! The record id is the declaration reference, the same string a secret uses:
//! a loose name, or `{qualified integration}:{name}` for a connection value.
//! The only field is `value`, a string. There is no key.

use std::collections::HashMap;
use std::sync::Arc;

use loco_lake::{DataAdapter, Record, Value};

use super::{check_name, upsert, VariableError, VariableMeta, VariableStore, VARIABLES};

/// A stored value wins, including `""`. A missing row uses `default` when it
/// is non-empty, and is absent when `default` is empty.
pub fn with_default(stored: Option<String>, default: &str) -> Option<String> {
    match stored {
        Some(value) => Some(value),
        None if default.is_empty() => None,
        None => Some(default.to_string()),
    }
}

pub struct LakeVariableStore {
    data: Arc<dyn DataAdapter>,
}

impl LakeVariableStore {
    pub fn new(data: Arc<dyn DataAdapter>) -> Self {
        Self { data }
    }
}

impl VariableStore for LakeVariableStore {
    fn set(
        &self,
        dataset_id: &str,
        name: &str,
        value: &str,
    ) -> Result<VariableMeta, VariableError> {
        check_name(name).map_err(VariableError::InvalidName)?;
        let mut fields = HashMap::new();
        fields.insert("value".to_string(), Value::String(value.to_string()));
        let record = upsert(self.data.as_ref(), dataset_id, VARIABLES, name, fields)?;
        meta(&record)
    }

    fn get(&self, dataset_id: &str, name: &str) -> Result<Option<String>, VariableError> {
        check_name(name).map_err(VariableError::InvalidName)?;
        let Some(record) = self.data.get(dataset_id, VARIABLES, name)? else {
            return Ok(None);
        };
        Ok(Some(value_of(&record)?))
    }

    fn delete(&self, dataset_id: &str, name: &str) -> Result<(), VariableError> {
        check_name(name).map_err(VariableError::InvalidName)?;
        self.data.delete(dataset_id, VARIABLES, name)?;
        Ok(())
    }

    fn list(&self, dataset_id: &str) -> Result<Vec<VariableMeta>, VariableError> {
        let mut rows = Vec::new();
        for record in self.data.list(dataset_id, VARIABLES)? {
            rows.push(meta(&record)?);
        }
        rows.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(rows)
    }

    fn delete_dataset(&self, dataset_id: &str) -> Result<(), VariableError> {
        let names: Vec<String> = self
            .data
            .list(dataset_id, VARIABLES)?
            .into_iter()
            .map(|record| record.id)
            .collect();
        let mut first = None;
        for name in names {
            if let Err(err) = self.data.delete(dataset_id, VARIABLES, &name) {
                if first.is_none() {
                    first = Some(VariableError::from(err));
                }
            }
        }
        match first {
            Some(err) => Err(err),
            None => Ok(()),
        }
    }
}

fn meta(record: &Record) -> Result<VariableMeta, VariableError> {
    Ok(VariableMeta {
        name: record.id.clone(),
        value: value_of(record)?,
        updated_at: record.updated_at.clone(),
    })
}

fn value_of(record: &Record) -> Result<String, VariableError> {
    match record.fields.get("value") {
        Some(Value::String(value)) => Ok(value.clone()),
        _ => Err(VariableError::Lake(loco_lake::Error::Internal(format!(
            "variable record {} has no string value",
            record.id
        )))),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use loco_lake::{DataAdapter, InMemoryAdapter, Value};

    use super::*;
    use crate::values::{VariableError, VariableStore, SECRETS, VARIABLES};

    const DATASET: &str = "ben/crm/dev";

    fn store(data: Arc<dyn DataAdapter>) -> LakeVariableStore {
        LakeVariableStore::new(data)
    }

    #[test]
    fn set_get_list_and_delete_round_trip() {
        let data = Arc::new(InMemoryAdapter::new());
        let store = store(data.clone());
        assert!(store.get(DATASET, "label").unwrap().is_none());

        let written = store.set(DATASET, "label", "Lot: 12").unwrap();
        assert_eq!(written.name, "label");
        assert_eq!(written.value, "Lot: 12");
        assert_eq!(
            store.get(DATASET, "label").unwrap().as_deref(),
            Some("Lot: 12")
        );

        store.set(DATASET, "region", "eu").unwrap();
        let listed = store.list(DATASET).unwrap();
        assert_eq!(
            listed
                .iter()
                .map(|row| row.name.as_str())
                .collect::<Vec<_>>(),
            vec!["label", "region"]
        );
        assert_eq!(listed[0].value, "Lot: 12");

        store.set(DATASET, "label", "replaced").unwrap();
        assert_eq!(data.list(DATASET, VARIABLES).unwrap().len(), 2);
        assert_eq!(
            store.get(DATASET, "label").unwrap().as_deref(),
            Some("replaced")
        );

        store.delete(DATASET, "label").unwrap();
        assert!(store.get(DATASET, "label").unwrap().is_none());
        assert_eq!(store.list(DATASET).unwrap().len(), 1);
        let missing = store.delete(DATASET, "label").unwrap_err();
        assert!(matches!(missing, VariableError::NotFound), "{missing}");
    }

    #[test]
    fn ambiguous_strings_round_trip_as_strings() {
        let data = Arc::new(InMemoryAdapter::new());
        let store = store(data.clone());
        for value in ["true", "null", "1", "", "yes", "Lot: 12"] {
            store.set(DATASET, "label", value).unwrap();
            assert_eq!(store.get(DATASET, "label").unwrap().as_deref(), Some(value));
        }
        let record = data.get(DATASET, VARIABLES, "label").unwrap().unwrap();
        assert_eq!(
            record.fields.get("value"),
            Some(&Value::String("Lot: 12".into()))
        );
        assert_eq!(data.list(DATASET, VARIABLES).unwrap().len(), 1);
    }

    #[test]
    fn default_fallback_keeps_a_stored_empty_string() {
        let data = Arc::new(InMemoryAdapter::new());
        let store = store(data);
        let default = "https://default.example";
        assert_eq!(
            with_default(store.get(DATASET, "base_url").unwrap(), default).as_deref(),
            Some(default)
        );
        assert_eq!(
            with_default(store.get(DATASET, "base_url").unwrap(), ""),
            None
        );

        store.set(DATASET, "base_url", "").unwrap();
        assert_eq!(
            with_default(store.get(DATASET, "base_url").unwrap(), default).as_deref(),
            Some("")
        );
        store
            .set(DATASET, "base_url", "https://set.example")
            .unwrap();
        assert_eq!(
            with_default(store.get(DATASET, "base_url").unwrap(), default).as_deref(),
            Some("https://set.example")
        );
    }

    #[test]
    fn invalid_name_writes_nothing() {
        let data = Arc::new(InMemoryAdapter::new());
        let store = store(data.clone());
        let err = store.set(DATASET, "", "x").unwrap_err();
        assert!(
            matches!(err, VariableError::InvalidName(ref msg) if msg.contains("invalid config value name"))
        );
        assert!(data.list(DATASET, VARIABLES).unwrap().is_empty());
        let err = store.delete(DATASET, "bad\0name").unwrap_err();
        assert!(matches!(err, VariableError::InvalidName(_)), "{err}");
    }

    #[test]
    fn delete_dataset_removes_only_variables_in_that_dataset() {
        let data = Arc::new(InMemoryAdapter::new());
        let store = store(data.clone());
        store
            .set(DATASET, "api_base", "https://set.example")
            .unwrap();
        store.set("ben/crm/other", "api_base", "keep").unwrap();
        let mut fields = HashMap::new();
        fields.insert("nonce".into(), Value::String("stay".into()));
        data.upsert(DATASET, SECRETS, "token", "system", fields)
            .unwrap();

        store.delete_dataset(DATASET).unwrap();

        assert!(store.get(DATASET, "api_base").unwrap().is_none());
        assert_eq!(
            store.get("ben/crm/other", "api_base").unwrap().as_deref(),
            Some("keep")
        );
        assert!(data.get(DATASET, SECRETS, "token").unwrap().is_some());
    }
}
