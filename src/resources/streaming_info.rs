//! `StreamingInfoExtensions`.
//!
//! Mirrors the offset accessors, set/copy/clear helpers for the streamed-resource descriptor.
//! The offset is 32-bit on old layouts and 64-bit on newer ones, selected by the field presence.
//!
//! Differential fixture: `streaming_info_fixture.txt` (probe `Temp/opencode/resprobe`).

/// A streamed-resource descriptor view.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StreamingInfoView {
    /// `Path`: a streamed file path; an empty path means the descriptor is unset.
    pub path: String,
    /// Whether the layout serializes `Offset` as a 64-bit field.
    pub has_offset_u64: bool,
    /// `Offset` when it is a 32-bit field.
    pub offset_u32: u32,
    /// `Offset` when it is a 64-bit field.
    pub offset_u64: u64,
    /// `Size`.
    pub size: u32,
}

impl StreamingInfoView {
    /// `StreamingInfoExtensions.IsSet`: whether a streamed path was recorded.
    pub fn is_set(&self) -> bool {
        !self.path.is_empty()
    }

    /// `StreamingInfoExtensions.GetOffset`: the offset in the layout's width.
    pub fn get_offset(&self) -> u64 {
        if self.has_offset_u64 {
            self.offset_u64
        } else {
            u64::from(self.offset_u32)
        }
    }

    /// `StreamingInfoExtensions.SetOffset`: truncates to 32 bits on the old layout, matching the
    /// baseline's unchecked `(uint)` cast.
    pub fn set_offset(&mut self, value: u64) {
        if self.has_offset_u64 {
            self.offset_u64 = value;
        } else {
            self.offset_u32 = value as u32;
        }
    }

    /// `StreamingInfoExtensions.CopyValues`.
    pub fn copy_values(&mut self, source: &StreamingInfoView) {
        self.set_offset(source.get_offset());
        self.size = source.size;
        self.path.clone_from(&source.path);
    }

    /// `StreamingInfoExtensions.ClearValues`.
    pub fn clear_values(&mut self) {
        self.offset_u32 = 0;
        self.offset_u64 = 0;
        self.path.clear();
        self.size = 0;
    }

    /// `StreamingInfoExtensions.GetValues`: `(path, offset, size)`.
    pub fn get_values(&self) -> (&str, u64, u32) {
        (&self.path, self.get_offset(), self.size)
    }

    /// `StreamingInfoExtensions.SetValues`.
    pub fn set_values(&mut self, path: &str, offset: u64, size: u32) {
        self.path.clear();
        self.path.push_str(path);
        self.set_offset(offset);
        self.size = size;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_bool(text: &str) -> bool {
        text == "1"
    }

    /// Byte-for-byte differential against the reference `StreamingInfoExtensions` (probe
    /// `Temp/opencode/resprobe`).
    #[test]
    fn streaming_info_matches_the_reference_runtime() {
        let fixture = include_str!("streaming_info_fixture.txt");
        let mut rows = 0;
        for line in fixture.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let parts: Vec<&str> = line.split('|').collect();
            assert_eq!(parts[0], "S", "{line}");
            let mut view = StreamingInfoView {
                path: String::new(),
                has_offset_u64: parse_bool(parts[2]),
                offset_u32: 0,
                offset_u64: 0,
                size: 0,
            };
            assert_eq!(view.is_set(), parse_bool(parts[3]), "{line}");
            view.path.push_str("abc");
            view.set_offset(12345);
            view.size = 999;
            assert_eq!(view.is_set(), parse_bool(parts[4]), "{line}");
            assert_eq!(
                view.get_offset(),
                parts[5].parse::<u64>().unwrap(),
                "{line}"
            );

            let mut copy = StreamingInfoView {
                has_offset_u64: view.has_offset_u64,
                ..StreamingInfoView::default()
            };
            copy.copy_values(&view);
            assert_eq!(
                copy.get_offset(),
                parts[6].parse::<u64>().unwrap(),
                "{line}"
            );
            assert_eq!(copy.size, parts[7].parse::<u32>().unwrap(), "{line}");
            assert_eq!(copy.path, parts[8], "{line}");

            let mut cleared = StreamingInfoView {
                has_offset_u64: view.has_offset_u64,
                ..StreamingInfoView::default()
            };
            cleared.path.push_str("xyz");
            cleared.set_offset(7);
            cleared.size = 8;
            cleared.clear_values();
            assert_eq!(cleared.path, parts[9], "{line}");
            assert_eq!(
                cleared.get_offset(),
                parts[10].parse::<u64>().unwrap(),
                "{line}"
            );
            assert_eq!(cleared.size, parts[11].parse::<u32>().unwrap(), "{line}");
            assert_eq!(cleared.is_set(), parse_bool(parts[12]), "{line}");
            rows += 1;
        }
        assert_eq!(rows, 3);
    }
}
