use loro::{LoroDoc, ExportMode};
use crate::config::CompressionType;
use crate::error::Result;

pub struct CompressedPayload {
    pub compression: CompressionType,
    pub raw_data: Vec<u8>,
}

impl CompressedPayload {
    pub fn compress(raw_bytes: &[u8], strategy: CompressionType) -> Self;
    pub fn decompress(&self) -> std::result::Result<Vec<u8>, std::io::Error>;
}

pub trait LoroDocCompressionExt {
    fn export_compressed(&self, mode: ExportMode, strategy: CompressionType) -> CompressedPayload;
    fn import_compressed(&mut self, payload: &CompressedPayload) -> Result<()>;
}

impl LoroDocCompressionExt for LoroDoc {
    fn export_compressed(&self, mode: ExportMode, strategy: CompressionType) -> CompressedPayload;
    fn import_compressed(&mut self, payload: &CompressedPayload) -> Result<()>;
}