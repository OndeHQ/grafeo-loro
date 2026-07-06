#[async_trait::async_trait]
pub trait StorageBackend: Send + Sync + 'static {
    async fn load(&self, key: &str) -> std::result::Result<Vec<u8>, std::io::Error>;
    async fn save(&self, key: &str, bytes: Vec<u8>) -> std::result::Result<(), std::io::Error>;
    async fn list(&self, prefix: &str) -> std::result::Result<Vec<String>, std::io::Error>;
    async fn delete(&self, key: &str) -> std::result::Result<(), std::io::Error>;
}
