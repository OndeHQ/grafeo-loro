use crate::constants::{ORIGIN_GRAFEO_BRIDGE, ORIGIN_LORO_BRIDGE};

/// Checks if Loro event originated from bridge. Prevents echo.
pub fn is_grafeo_bridge_origin(origin: &str) -> bool {
    origin == ORIGIN_GRAFEO_BRIDGE
}

/// Checks if Grafeo CDC event originated from bridge. Prevents echo.
pub fn is_loro_bridge_origin(origin: Option<&str>) -> bool {
    origin == Some(ORIGIN_LORO_BRIDGE)
}