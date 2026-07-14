//! In-memory storage backend — reference impl for unit tests.
//!
//! NOT for production. Use a real backend (filesystem, S3, IDB, OPFS) in
//! your app. This backend is ` Rc<RefCell<>>`-friendly: all methods take
//! `&self` and mutate via interior mutability.

use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;

use super::StorageBackend;

/// In-memory key-value store. Thread-safe via `Mutex` (cheap uncontended
/// lock — production hot paths do not touch storage).
#[derive(Default)]
pub struct InMemoryStorage {
    inner: Mutex<HashMap<String, Vec<u8>>>,
}

impl InMemoryStorage {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait(?Send)]
impl StorageBackend for InMemoryStorage {
    async fn load(&self, key: &str) -> std::result::Result<Vec<u8>, std::io::Error> {
        let inner = self.inner.lock().unwrap();
        inner
            .get(key)
            .cloned()
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, format!("key not found: {key}")))
    }

    async fn save(&self, key: &str, bytes: Vec<u8>) -> std::result::Result<(), std::io::Error> {
        self.inner.lock().unwrap().insert(key.to_string(), bytes);
        Ok(())
    }

    async fn list(&self, prefix: &str) -> std::result::Result<Vec<String>, std::io::Error> {
        let inner = self.inner.lock().unwrap();
        let mut keys: Vec<String> = inner
            .keys()
            .filter(|k| k.starts_with(prefix))
            .cloned()
            .collect();
        keys.sort();
        Ok(keys)
    }

    async fn delete(&self, key: &str) -> std::result::Result<(), std::io::Error> {
        self.inner.lock().unwrap().remove(key);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn roundtrip() {
        let s = InMemoryStorage::new();
        s.save("k", vec![1, 2, 3]).await.unwrap();
        assert_eq!(s.load("k").await.unwrap(), vec![1, 2, 3]);
        assert!(matches!(
            s.load("missing").await,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound
        ));
        s.delete("k").await.unwrap();
        assert!(matches!(
            s.load("k").await,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound
        ));
    }

    #[tokio::test]
    async fn list_prefix() {
        let s = InMemoryStorage::new();
        s.save("graph_1/base.loro", vec![]).await.unwrap();
        s.save("graph_1/delta-1.loro", vec![]).await.unwrap();
        s.save("graph_2/base.loro", vec![]).await.unwrap();
        let keys = s.list("graph_1/").await.unwrap();
        assert_eq!(keys.len(), 2);
        assert!(keys.contains(&"graph_1/base.loro".to_string()));
        assert!(keys.contains(&"graph_1/delta-1.loro".to_string()));
    }
}
