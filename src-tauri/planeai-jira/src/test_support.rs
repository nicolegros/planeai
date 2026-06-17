use std::collections::HashMap;
use std::sync::Mutex;

use crate::auth::{Error, TokenStore};

/// In-memory token store for deterministic tests.
pub struct MemStore(Mutex<HashMap<String, String>>);

impl MemStore {
    pub fn new() -> Self {
        Self(Mutex::new(HashMap::new()))
    }

    pub fn with_entries(entries: Vec<(&str, &str)>) -> Self {
        let map = entries
            .into_iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        Self(Mutex::new(map))
    }
}

impl TokenStore for MemStore {
    fn get(&self, key: &str) -> Result<String, Error> {
        self.0
            .lock()
            .unwrap()
            .get(key)
            .cloned()
            .ok_or_else(|| Error::Keyring(format!("not found: {key}")))
    }

    fn set(&self, key: &str, value: &str) -> Result<(), Error> {
        self.0
            .lock()
            .unwrap()
            .insert(key.to_string(), value.to_string());
        Ok(())
    }

    fn delete(&self, key: &str) -> Result<(), Error> {
        self.0.lock().unwrap().remove(key);
        Ok(())
    }
}
