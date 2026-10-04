//! `GameObjectExtensions` active-state helpers.
//!
//! Mirrors `AssetRipper.SourceGenerated.Extensions/GameObjectExtensions.cs` for the active state.
//! Old versions expose a `Boolean` field, newer ones a `Byte` field; the getter combines both, and
//! the setter writes both. In practice the sampled versions only serialize the boolean field, so
//! the byte contribution reads as zero.

/// A GameObject active-state view.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct GameObjectView {
    /// `Has_IsActive_Byte`.
    pub has_is_active_byte: bool,
    /// `IsActive_Byte`.
    pub is_active_byte: u8,
    /// `IsActive_Boolean`.
    pub is_active_boolean: bool,
}

impl GameObjectView {
    /// `GameObjectExtensions.GetIsActive`.
    pub fn get_is_active(&self) -> bool {
        self.is_active_boolean || (self.has_is_active_byte && self.is_active_byte > 0)
    }

    /// `GameObjectExtensions.SetIsActive`.
    pub fn set_is_active(&mut self, active: bool) {
        if self.has_is_active_byte {
            self.is_active_byte = u8::from(active);
        }
        self.is_active_boolean = active;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Byte-for-byte differential against the reference `GameObjectExtensions` (probe
    /// `Temp/opencode/goprobe2`).
    #[test]
    fn game_object_active_matches_the_reference_runtime() {
        let fixture = include_str!("game_object_fixture.txt");
        let mut rows = 0;
        for line in fixture.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let parts: Vec<&str> = line.split('|').collect();
            assert_eq!(parts[0], "GO", "{line}");
            let has_byte = parts[3] == "1";

            let active = |boolean: bool, byte: u8| {
                GameObjectView {
                    has_is_active_byte: has_byte,
                    is_active_byte: byte,
                    is_active_boolean: boolean,
                }
                .get_is_active()
            };

            assert_eq!(active(false, 0), parts[4] == "1", "{line}");
            assert_eq!(active(false, 1), parts[5] == "1", "{line}");
            assert_eq!(active(true, 0), parts[6] == "1", "{line}");
            assert_eq!(active(true, 5), parts[7] == "1", "{line}");

            let mut set_true = GameObjectView {
                has_is_active_byte: has_byte,
                ..GameObjectView::default()
            };
            set_true.set_is_active(true);
            assert_eq!(set_true.is_active_boolean, parts[8] == "1", "{line}");
            assert_eq!(
                set_true.is_active_byte,
                parts[9].parse::<u8>().unwrap(),
                "{line}"
            );

            let mut set_false = GameObjectView {
                has_is_active_byte: has_byte,
                ..GameObjectView::default()
            };
            set_false.set_is_active(false);
            assert_eq!(set_false.is_active_boolean, parts[10] == "1", "{line}");
            assert_eq!(
                set_false.is_active_byte,
                parts[11].parse::<u8>().unwrap(),
                "{line}"
            );
            rows += 1;
        }
        assert_eq!(rows, 3);
    }
}
