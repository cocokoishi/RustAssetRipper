//! `ColorRGBA32Extensions` and `ColorRGBAfExtensions`.
//!
//! Mirrors the `ColorRGBA32` byte packing (`Rgba = r | g << 8 | b << 16 | a << 24`) and the
//! black/white helpers for both color forms.
//!
//! Differential fixture: `color_fixture.txt` (probe `Temp/opencode/miscprobe`).

/// `ColorRGBA32Extensions.SetValues`.
pub fn set_values(rgba: &mut u32, r: u8, g: u8, b: u8, a: u8) {
    *rgba = u32::from(r) | (u32::from(g) << 8) | (u32::from(b) << 16) | (u32::from(a) << 24);
}

/// `ColorRGBA32Extensions.SetAsBlack`.
pub fn set_as_black(rgba: &mut u32) {
    set_values(rgba, 0, 0, 0, u8::MAX);
}

/// `ColorRGBA32Extensions.SetAsWhite`.
pub fn set_as_white(rgba: &mut u32) {
    *rgba = u32::MAX;
}

/// `ColorRGBA32Extensions.R` getter.
pub fn red(rgba: u32) -> u8 {
    rgba as u8
}

/// `ColorRGBA32Extensions.G` getter.
pub fn green(rgba: u32) -> u8 {
    (rgba >> 8) as u8
}

/// `ColorRGBA32Extensions.B` getter.
pub fn blue(rgba: u32) -> u8 {
    (rgba >> 16) as u8
}

/// `ColorRGBA32Extensions.A` getter.
pub fn alpha(rgba: u32) -> u8 {
    (rgba >> 24) as u8
}

/// `ColorRGBA32Extensions.R` setter.
pub fn set_red(rgba: &mut u32, value: u8) {
    set_values(rgba, value, green(*rgba), blue(*rgba), alpha(*rgba));
}

/// `ColorRGBA32Extensions.G` setter.
pub fn set_green(rgba: &mut u32, value: u8) {
    set_values(rgba, red(*rgba), value, blue(*rgba), alpha(*rgba));
}

/// `ColorRGBA32Extensions.B` setter.
pub fn set_blue(rgba: &mut u32, value: u8) {
    set_values(rgba, red(*rgba), green(*rgba), value, alpha(*rgba));
}

/// `ColorRGBA32Extensions.A` setter.
pub fn set_alpha(rgba: &mut u32, value: u8) {
    set_values(rgba, red(*rgba), green(*rgba), blue(*rgba), value);
}

/// `ColorRGBAfExtensions.SetAsBlack`.
pub fn rgbaf_set_as_black(color: &mut [f32; 4]) {
    *color = [0.0, 0.0, 0.0, 1.0];
}

/// `ColorRGBAfExtensions.SetAsWhite`.
pub fn rgbaf_set_as_white(color: &mut [f32; 4]) {
    *color = [1.0, 1.0, 1.0, 1.0];
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Byte-for-byte differential against the reference color extensions (probe
    /// `Temp/opencode/miscprobe`).
    #[test]
    fn color_packing_matches_the_reference_runtime() {
        let fixture = include_str!("color_fixture.txt");
        let mut rows = 0;
        for line in fixture.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let parts: Vec<&str> = line.split('|').collect();
            assert_eq!(parts[0], "C", "{line}");
            let mut rgba = 0u32;
            match parts[1] {
                "set" => set_values(&mut rgba, 0x12, 0x34, 0x56, 0x78),
                "black" => set_as_black(&mut rgba),
                "white" => set_as_white(&mut rgba),
                other => panic!("unknown color op {other}"),
            }
            assert_eq!(rgba, parts[2].parse::<u32>().unwrap(), "{line}");
            assert_eq!(
                u32::from(red(rgba)),
                parts[3].parse::<u32>().unwrap(),
                "{line}"
            );
            assert_eq!(
                u32::from(green(rgba)),
                parts[4].parse::<u32>().unwrap(),
                "{line}"
            );
            assert_eq!(
                u32::from(blue(rgba)),
                parts[5].parse::<u32>().unwrap(),
                "{line}"
            );
            assert_eq!(
                u32::from(alpha(rgba)),
                parts[6].parse::<u32>().unwrap(),
                "{line}"
            );
            rows += 1;
        }
        assert_eq!(rows, 3);

        // The individual setters round-trip through the getters.
        let mut rgba = 0u32;
        set_red(&mut rgba, 1);
        set_green(&mut rgba, 2);
        set_blue(&mut rgba, 3);
        set_alpha(&mut rgba, 4);
        assert_eq!(
            (red(rgba), green(rgba), blue(rgba), alpha(rgba)),
            (1, 2, 3, 4)
        );

        let mut float = [0.0f32; 4];
        rgbaf_set_as_black(&mut float);
        assert_eq!(float, [0.0, 0.0, 0.0, 1.0]);
        rgbaf_set_as_white(&mut float);
        assert_eq!(float, [1.0, 1.0, 1.0, 1.0]);
    }
}
