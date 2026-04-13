use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use crate::Value;

/// A thread-safe cache that stores typed values keyed by string.
/// Each entry is a map of field names to `Value`s, wrapped in an `Arc` so
/// concurrent reads are O(1) (a reference-count bump, not a deep clone).
pub struct TypedCache {
    inner: RwLock<HashMap<String, Arc<HashMap<String, Value>>>>,
}

impl TypedCache {
    pub fn new() -> Self {
        TypedCache {
            inner: RwLock::new(HashMap::new()),
        }
    }

    /// Returns a shared reference to the field map. Cloning the `Arc` is O(1).
    pub fn get(&self, key: &str) -> Option<Arc<HashMap<String, Value>>> {
        let guard = self.inner.read().unwrap();
        guard.get(key).cloned()
    }

    pub fn set(&self, key: &str, value: HashMap<String, Value>) {
        let mut guard = self.inner.write().unwrap();
        guard.insert(key.to_string(), Arc::new(value));
    }

    pub fn remove(&self, key: &str) -> Option<Arc<HashMap<String, Value>>> {
        let mut guard = self.inner.write().unwrap();
        guard.remove(key)
    }

    pub fn contains(&self, key: &str) -> bool {
        let guard = self.inner.read().unwrap();
        guard.contains_key(key)
    }
}

impl Default for TypedCache {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Value;

    #[test]
    fn test_set_and_get() {
        let cache = TypedCache::new();
        let mut map = HashMap::new();
        map.insert("name".to_string(), Value::String("test".to_string()));
        map.insert("count".to_string(), Value::Integer(42));

        cache.set("my_key", map);

        let retrieved = cache.get("my_key").unwrap();
        assert_eq!(retrieved.get("name").unwrap().as_string(), Some("test"));
        assert_eq!(retrieved.get("count").unwrap().as_integer(), Some(42));
    }

    #[test]
    fn test_get_missing() {
        let cache = TypedCache::new();
        assert!(cache.get("nonexistent").is_none());
    }

    #[test]
    fn test_remove() {
        let cache = TypedCache::new();
        cache.set("key", HashMap::new());
        assert!(cache.contains("key"));
        cache.remove("key");
        assert!(!cache.contains("key"));
    }

    #[test]
    fn test_overwrite() {
        let cache = TypedCache::new();
        let mut map1 = HashMap::new();
        map1.insert("v".to_string(), Value::Integer(1));
        cache.set("key", map1);

        let mut map2 = HashMap::new();
        map2.insert("v".to_string(), Value::Integer(2));
        cache.set("key", map2);

        let retrieved = cache.get("key").unwrap();
        assert_eq!(retrieved.get("v").unwrap().as_integer(), Some(2));
    }
}
