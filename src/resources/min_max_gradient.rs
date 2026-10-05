//! `MinMaxGradientExtensions`.
//!
//! Mirrors the state getter/setter and the default reset for the serialized particle gradient.
//! The color is packed 32-bit on older layouts and four floats on newer ones.
//!
//! Differential fixture: `min_max_gradient_fixture.txt` (probe `Temp/opencode/resprobe`).

/// The `MinMaxGradient` state enum, as serialized.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i32)]
pub enum MinMaxGradientState {
    /// A single color.
    Color = 0,
    /// A single gradient.
    Gradient = 1,
    /// A random color between two colors.
    RandomBetweenTwoColors = 2,
    /// A random gradient between two gradients.
    RandomBetweenTwoGradients = 3,
}

impl MinMaxGradientState {
    /// Decodes the serialized integer, preserving unknown values as `None`.
    pub fn from_i32(value: i32) -> Option<Self> {
        Some(match value {
            0 => Self::Color,
            1 => Self::Gradient,
            2 => Self::RandomBetweenTwoColors,
            3 => Self::RandomBetweenTwoGradients,
            _ => return None,
        })
    }
}

/// A min/max gradient view. Exactly one color representation is present per layout.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MinMaxGradientView {
    /// The serialized state.
    pub state: i32,
    /// `MinColor` as a packed `ColorRGBA32` on the older layout.
    pub min_color32: Option<u32>,
    /// `MinColor` as `ColorRGBAf` on the newer layout.
    pub min_color_f32: Option<[f32; 4]>,
    /// `MaxColor` as a packed `ColorRGBA32` on the older layout.
    pub max_color32: Option<u32>,
    /// `MaxColor` as `ColorRGBAf` on the newer layout.
    pub max_color_f32: Option<[f32; 4]>,
}

impl MinMaxGradientView {
    /// `MinMaxGradientExtensions.GetMinMaxState`.
    pub fn get_min_max_state(&self) -> i32 {
        self.state
    }

    /// `MinMaxGradientExtensions.SetMinMaxState`.
    pub fn set_min_max_state(&mut self, state: MinMaxGradientState) {
        self.state = state as i32;
    }

    /// `MinMaxGradientExtensions.SetToDefault`: a single-color state with both colors white.
    pub fn set_to_default(&mut self) {
        self.state = MinMaxGradientState::Color as i32;
        if let Some(color) = &mut self.min_color32 {
            *color = 0xFFFF_FFFF;
        }
        if let Some(color) = &mut self.max_color32 {
            *color = 0xFFFF_FFFF;
        }
        if let Some(color) = &mut self.min_color_f32 {
            *color = [1.0, 1.0, 1.0, 1.0];
        }
        if let Some(color) = &mut self.max_color_f32 {
            *color = [1.0, 1.0, 1.0, 1.0];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_bool(text: &str) -> bool {
        text == "1"
    }

    /// Byte-for-byte differential against the reference `MinMaxGradientExtensions` (probe
    /// `Temp/opencode/resprobe`).
    #[test]
    fn min_max_gradient_matches_the_reference_runtime() {
        let fixture = include_str!("min_max_gradient_fixture.txt");
        let mut rows = 0;
        for line in fixture.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let parts: Vec<&str> = line.split('|').collect();
            assert_eq!(parts[0], "MG", "{line}");
            let has_color32 = parse_bool(parts[6]);
            let mut view = MinMaxGradientView {
                min_color32: has_color32.then_some(0),
                max_color32: has_color32.then_some(0),
                min_color_f32: (!has_color32).then_some([0.0; 4]),
                max_color_f32: (!has_color32).then_some([0.0; 4]),
                ..MinMaxGradientView::default()
            };
            view.set_min_max_state(MinMaxGradientState::RandomBetweenTwoGradients);
            assert_eq!(
                view.get_min_max_state(),
                parts[2].parse::<i32>().unwrap(),
                "{line}"
            );
            view.set_to_default();
            assert_eq!(
                view.get_min_max_state(),
                parts[3].parse::<i32>().unwrap(),
                "{line}"
            );
            assert_eq!(
                view.min_color32.unwrap_or(0),
                parts[4].parse::<u32>().unwrap(),
                "{line}"
            );
            assert_eq!(
                view.max_color32.unwrap_or(0),
                parts[5].parse::<u32>().unwrap(),
                "{line}"
            );
            assert_eq!(
                view.min_color_f32.unwrap_or([0.0; 4])[0].to_bits(),
                parts[7].parse::<u32>().unwrap(),
                "{line}"
            );
            rows += 1;
        }
        assert_eq!(rows, 3);
    }
}
