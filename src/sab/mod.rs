//! Native SAB (SharedArrayBuffer) layout writer (issue #3 sub-issue 6).
//!
//! Pushes Y-offset / height math to Rust and writes directly to a SAB
//! pointer so the JS virtualizer doesn't have to.
//!
//! # Layout entry format
//!
//! Each entry is [`LAYOUT_ENTRY_BYTES`] (16) bytes, little-endian:
//!
//! ```text
//! offset  size  field
//! 0       4     y_offset (f32)
//! 4       4     height   (f32)
//! 8       8     padding  (zeros — leaves room for a future cached
//!                        "absolute_bottom" f64 or a u64 node-id slot
//!                        without breaking the wire format)
//! ```
//!
//! # WASM binding story
//!
//! In WASM, the `buffer` field is a `Vec<u8>` owned by Rust; the JS side
//! obtains a view via `wasm-bindgen`'s `memory.buffer` slice + the
//! `as_bytes_ptr` accessor (TODO: future `#[wasm_bindgen]` impl block under
//! the `wasm` feature will expose the pointer + length directly to JS). For
//! now the pure-Rust API is sufficient for tests + native hot-path callers.

/// Layout entry size: 4 (y_offset: f32) + 4 (height: f32) + 8 (padding) = 16.
pub const LAYOUT_ENTRY_BYTES: usize = 16;

/// Native SAB layout writer.
///
/// Maintains a packed byte buffer of layout entries + an entry count. The
/// buffer is pre-allocated to `capacity_entries * LAYOUT_ENTRY_BYTES` and
/// never grows — callers must size the capacity up-front. (A `resize` helper
/// is a TODO if the virtualizer ever needs dynamic capacity.)
pub struct SabLayoutWriter {
    /// Raw byte buffer. In WASM, this is a view into the SharedArrayBuffer
    /// (via `wasm-bindgen` memory sharing — TODO under `wasm` feature).
    buffer: Vec<u8>,
    entry_count: usize,
}

impl SabLayoutWriter {
    /// Create a writer with capacity for `capacity_entries` layout entries.
    /// All entries start zeroed (y_offset=0, height=0).
    pub fn new(capacity_entries: usize) -> Self {
        let buf_len = capacity_entries.checked_mul(LAYOUT_ENTRY_BYTES).expect(
            "SabLayoutWriter capacity overflow: capacity_entries * LAYOUT_ENTRY_BYTES exceeds usize",
        );
        Self {
            buffer: vec![0u8; buf_len],
            entry_count: 0,
        }
    }

    /// Set the layout entry at `index`. Grows `entry_count` if `index` is
    /// exactly the next slot (i.e. `index == entry_count`); for `index <
    /// entry_count` the existing entry is overwritten.
    ///
    /// # Panics
    ///
    /// Panics if `index > entry_count` (would leave a gap) or if `index` is
    /// out of buffer capacity. Both are programmer errors — the virtualizer
    /// should pre-size capacity up-front and write entries sequentially.
    pub fn set_layout(&mut self, index: usize, y_offset: f32, height: f32) {
        assert!(
            index <= self.entry_count,
            "SabLayoutWriter::set_layout: index {} > entry_count {} (would leave a gap)",
            index,
            self.entry_count
        );
        let byte_off = index
            .checked_mul(LAYOUT_ENTRY_BYTES)
            .expect("index * LAYOUT_ENTRY_BYTES overflow");
        assert!(
            byte_off + LAYOUT_ENTRY_BYTES <= self.buffer.len(),
            "SabLayoutWriter::set_layout: index {} out of capacity {}",
            index,
            self.buffer.len() / LAYOUT_ENTRY_BYTES
        );
        self.buffer[byte_off..byte_off + 4].copy_from_slice(&y_offset.to_le_bytes());
        self.buffer[byte_off + 4..byte_off + 8].copy_from_slice(&height.to_le_bytes());
        // Padding (bytes 8..16) stays zeroed.
        if index == self.entry_count {
            self.entry_count += 1;
        }
    }

    /// Cascade y_offset changes starting at `start_index`. For each entry
    /// `i >= start_index`, sets `y_offset[i] = y_offset[i-1] + height[i-1]`.
    ///
    /// This is the "push math to Rust" hot path — JS would otherwise loop
    /// over the SAB doing floating-point arithmetic per entry. Here we do it
    /// in a tight Rust loop with no FFI overhead per entry.
    ///
    /// # No-op cases
    ///
    /// - `start_index == 0`: leaves entry 0's y_offset as-is (it's the root).
    /// - `start_index >= entry_count`: no-op (nothing to cascade).
    pub fn recompute_offsets(&mut self, start_index: usize) {
        if self.entry_count == 0 || start_index >= self.entry_count {
            return;
        }
        let mut i = start_index;
        // Edge case: if start_index == 0, leave entry 0's y_offset alone and
        // start cascading from entry 1.
        if i == 0 {
            i = 1;
        }
        while i < self.entry_count {
            let prev_off = (i - 1) * LAYOUT_ENTRY_BYTES;
            let cur_off = i * LAYOUT_ENTRY_BYTES;
            let prev_y = f32::from_le_bytes([
                self.buffer[prev_off],
                self.buffer[prev_off + 1],
                self.buffer[prev_off + 2],
                self.buffer[prev_off + 3],
            ]);
            let prev_h = f32::from_le_bytes([
                self.buffer[prev_off + 4],
                self.buffer[prev_off + 5],
                self.buffer[prev_off + 6],
                self.buffer[prev_off + 7],
            ]);
            let new_y = prev_y + prev_h;
            self.buffer[cur_off..cur_off + 4].copy_from_slice(&new_y.to_le_bytes());
            i += 1;
        }
    }

    /// Total height of the layout = max(y_offset + height) across all
    /// entries. Returns 0.0 if no entries.
    pub fn total_height(&self) -> f32 {
        let mut max: f32 = 0.0;
        for i in 0..self.entry_count {
            let off = i * LAYOUT_ENTRY_BYTES;
            let y = f32::from_le_bytes([
                self.buffer[off],
                self.buffer[off + 1],
                self.buffer[off + 2],
                self.buffer[off + 3],
            ]);
            let h = f32::from_le_bytes([
                self.buffer[off + 4],
                self.buffer[off + 5],
                self.buffer[off + 6],
                self.buffer[off + 7],
            ]);
            let bottom = y + h;
            if bottom > max {
                max = bottom;
            }
        }
        max
    }

    /// Borrow the raw byte buffer for FFI. The slice length is
    /// `capacity_entries * LAYOUT_ENTRY_BYTES` (NOT `entry_count * ...`) —
    /// unused trailing bytes are zeroed.
    pub fn as_bytes(&self) -> &[u8] {
        &self.buffer
    }

    /// Number of entries that have been written (via [`set_layout`]).
    /// Always ≤ capacity.
    ///
    /// [`set_layout`]: Self::set_layout
    pub fn entry_count(&self) -> usize {
        self.entry_count
    }

    /// Capacity (in entries) of the underlying buffer.
    pub fn capacity_entries(&self) -> usize {
        self.buffer.len() / LAYOUT_ENTRY_BYTES
    }

    /// Read back a single entry's `(y_offset, height)`. Returns `None` if
    /// `index >= entry_count`. Useful for tests + JS-side debugging.
    pub fn get_layout(&self, index: usize) -> Option<(f32, f32)> {
        if index >= self.entry_count {
            return None;
        }
        let off = index * LAYOUT_ENTRY_BYTES;
        let y = f32::from_le_bytes([
            self.buffer[off],
            self.buffer[off + 1],
            self.buffer[off + 2],
            self.buffer[off + 3],
        ]);
        let h = f32::from_le_bytes([
            self.buffer[off + 4],
            self.buffer[off + 5],
            self.buffer[off + 6],
            self.buffer[off + 7],
        ]);
        Some((y, h))
    }
}

impl Default for SabLayoutWriter {
    fn default() -> Self {
        Self::new(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_count_grows_sequentially() {
        let mut w = SabLayoutWriter::new(8);
        assert_eq!(w.entry_count(), 0);
        w.set_layout(0, 0.0, 10.0);
        assert_eq!(w.entry_count(), 1);
        w.set_layout(1, 10.0, 20.0);
        assert_eq!(w.entry_count(), 2);
    }

    #[test]
    #[should_panic(expected = "would leave a gap")]
    fn set_layout_panics_on_gap() {
        let mut w = SabLayoutWriter::new(8);
        w.set_layout(5, 0.0, 10.0); // skip 0..4
    }

    #[test]
    #[should_panic(expected = "out of capacity")]
    fn set_layout_panics_on_overflow() {
        let mut w = SabLayoutWriter::new(2);
        w.set_layout(0, 0.0, 10.0);
        w.set_layout(1, 10.0, 20.0);
        w.set_layout(2, 30.0, 40.0); // overflow
    }

    #[test]
    fn recompute_cascades_y_offsets() {
        let mut w = SabLayoutWriter::new(4);
        w.set_layout(0, 0.0, 10.0);
        w.set_layout(1, 999.0, 20.0); // bogus y_offset — will be recomputed
        w.set_layout(2, 999.0, 5.0);
        w.recompute_offsets(1);
        assert_eq!(w.get_layout(0), Some((0.0, 10.0)));
        assert_eq!(w.get_layout(1), Some((10.0, 20.0)));
        assert_eq!(w.get_layout(2), Some((30.0, 5.0)));
    }

    #[test]
    fn total_height_returns_max_bottom() {
        let mut w = SabLayoutWriter::new(4);
        w.set_layout(0, 0.0, 10.0);
        w.set_layout(1, 10.0, 20.0);
        w.set_layout(2, 30.0, 5.0);
        // max(0+10, 10+20, 30+5) = 35
        assert_eq!(w.total_height(), 35.0);
    }

    #[test]
    fn as_bytes_length_is_capacity_times_entry_size() {
        let w = SabLayoutWriter::new(7);
        assert_eq!(w.as_bytes().len(), 7 * LAYOUT_ENTRY_BYTES);
    }
}
