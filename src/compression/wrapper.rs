use loro::{LoroDoc, ExportMode};
use crate::config::CompressionType;
use crate::error::Result;

/// Compressed payload envelope: codec + raw bytes.
pub struct CompressedPayload {
    /// Codec used to produce `raw_data`.
    pub compression: CompressionType,
    /// Compressed bytes.
    pub raw_data: Vec<u8>,
}

impl CompressedPayload {
    /// Compress `raw_bytes` using `strategy`.
    pub fn compress(raw_bytes: &[u8], strategy: CompressionType) -> Self {
        let _ = (raw_bytes, strategy);
        unimplemented!()
    }

    /// Decompress `raw_data` back to the original Loro bytes.
    pub fn decompress(&self) -> std::result::Result<Vec<u8>, std::io::Error> {
        unimplemented!()
    }
}

/// Extension trait binding compression onto `LoroDoc` export/import.
pub trait LoroDocCompressionExt {
    /// Export the doc with `mode`, then compress.
    fn export_compressed(&self, mode: ExportMode, strategy: CompressionType) -> CompressedPayload;
    /// Decompress `payload`, then import into this doc.
    fn import_compressed(&mut self, payload: &CompressedPayload) -> Result<()>;
}

impl LoroDocCompressionExt for LoroDoc {
    fn export_compressed(&self, mode: ExportMode, strategy: CompressionType) -> CompressedPayload {
        let _ = (mode, strategy);
        unimplemented!()
    }

    fn import_compressed(&mut self, payload: &CompressedPayload) -> Result<()> {
        let _ = payload;
        unimplemented!()
    }
}
