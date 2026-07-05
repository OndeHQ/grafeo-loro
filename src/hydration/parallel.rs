use std::sync::Arc;
use loro::LoroDoc;
use grafeo::GrafeoDB;
use crate::error::Result;

/// Rebuilds Grafeo indexes from Loro state using Rayon chunks.
pub fn parallel_hydrate_grafeo(db: &Arc<GrafeoDB>, doc: &LoroDoc) -> Result<()>;