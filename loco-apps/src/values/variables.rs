//! Variable values as plain lake records in [`super::VARIABLES`].
//!
//! No trait: a variable is a string, and the lake already stores strings.
//! The record id is the declaration reference, the same string a secret uses.

use std::collections::HashMap;

use loco_lake::{DataAdapter, Error, Record, Value};

use super::{check_name, upsert, VARIABLES};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VariableMeta {
    pub name: String,
    pub value: String,
    pub updated_at: String,
}

pub fn put_variable(
    data: &dyn DataAdapter,
    dataset_id: &str,
    name: &str,
    value: &str,
) -> Result<VariableMeta, Error> {
    check_name(name).map_err(Error::Internal)?;
    let mut fields = HashMap::new();
    fields.insert("value".to_string(), Value::String(value.to_string()));
    let record = upsert(data, dataset_id, VARIABLES, name, fields)?;
    meta(&record)
}

pub fn get_variable(
    data: &dyn DataAdapter,
    dataset_id: &str,
    name: &str,
) -> Result<Option<String>, Error> {
    check_name(name).map_err(Error::Internal)?;
    let Some(record) = data.get(dataset_id, VARIABLES, name)? else {
        return Ok(None);
    };
    Ok(Some(value_of(&record)?))
}

pub fn delete_variable(data: &dyn DataAdapter, dataset_id: &str, name: &str) -> Result<(), Error> {
    check_name(name).map_err(Error::Internal)?;
    data.delete(dataset_id, VARIABLES, name)
}

pub fn list_variables(
    data: &dyn DataAdapter,
    dataset_id: &str,
) -> Result<Vec<VariableMeta>, Error> {
    let mut rows = Vec::new();
    for record in data.list(dataset_id, VARIABLES)? {
        rows.push(meta(&record)?);
    }
    rows.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(rows)
}

fn meta(record: &Record) -> Result<VariableMeta, Error> {
    Ok(VariableMeta {
        name: record.id.clone(),
        value: value_of(record)?,
        updated_at: record.updated_at.clone(),
    })
}

fn value_of(record: &Record) -> Result<String, Error> {
    match record.fields.get("value") {
        Some(Value::String(value)) => Ok(value.clone()),
        _ => Err(Error::Internal(format!(
            "variable record {} has no string value",
            record.id
        ))),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use loco_lake::InMemoryAdapter;

    use super::*;

    #[test]
    fn ambiguous_strings_round_trip_as_strings() {
        let data = Arc::new(InMemoryAdapter::new());
        let dataset = "ben/crm/dev";
        for value in ["true", "null", "1", "", "yes", "Lot: 12"] {
            put_variable(data.as_ref(), dataset, "label", value).unwrap();
            assert_eq!(
                get_variable(data.as_ref(), dataset, "label")
                    .unwrap()
                    .as_deref(),
                Some(value)
            );
        }
        let record = data.get(dataset, VARIABLES, "label").unwrap().unwrap();
        assert_eq!(
            record.fields.get("value"),
            Some(&Value::String("Lot: 12".into()))
        );
        assert_eq!(data.list(dataset, VARIABLES).unwrap().len(), 1);
    }
}
