//! `GLTextureSettingsExtensions`.
//!
//! Mirrors the default-reset and copy helpers for the serialized texture sampler settings. The
//! wrap fields differ by layout generation: older files serialize a single `WrapMode`, newer ones
//! serialize `WrapU`/`WrapV`/`WrapW`.
//!
//! Differential fixture: `gl_texture_settings_fixture.txt` (probe `Temp/opencode/resprobe`).

/// A texture sampler-settings view.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GLTextureSettingsView {
    /// `FilterMode`.
    pub filter_mode: i32,
    /// `Aniso`.
    pub aniso: i32,
    /// `MipBias`.
    pub mip_bias: f32,
    /// `WrapMode`: present only on the older layout.
    pub wrap_mode: Option<i32>,
    /// `WrapU`: present only on the newer layout.
    pub wrap_u: Option<i32>,
    /// `WrapV`: present only on the newer layout.
    pub wrap_v: Option<i32>,
    /// `WrapW`: present only on the newer layout.
    pub wrap_w: Option<i32>,
}

impl GLTextureSettingsView {
    /// `GLTextureSettingsExtensions.SetToDefault`: `FilterMode = -1`, `Aniso = -1`,
    /// `MipBias = -100`, and every present wrap field `= -1`.
    pub fn set_to_default(&mut self) {
        self.filter_mode = -1;
        self.aniso = -1;
        self.mip_bias = -100.0;
        for value in [
            &mut self.wrap_mode,
            &mut self.wrap_u,
            &mut self.wrap_v,
            &mut self.wrap_w,
        ]
        .into_iter()
        .flatten()
        {
            *value = -1;
        }
    }

    /// `GLTextureSettingsExtensions.CopyValues`. A field absent on the destination stays absent;
    /// a present destination field takes the source value or `0` when the source lacks it.
    pub fn copy_values(&mut self, source: &GLTextureSettingsView) {
        self.aniso = source.aniso;
        self.filter_mode = source.filter_mode;
        self.mip_bias = source.mip_bias;
        self.wrap_mode = self.wrap_mode.map(|_| source.wrap_mode.unwrap_or(0));
        self.wrap_u = self.wrap_u.map(|_| source.wrap_u.unwrap_or(0));
        self.wrap_v = self.wrap_v.map(|_| source.wrap_v.unwrap_or(0));
        self.wrap_w = self.wrap_w.map(|_| source.wrap_w.unwrap_or(0));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_bool(text: &str) -> bool {
        text == "1"
    }

    /// Byte-for-byte differential against the reference `GLTextureSettingsExtensions` (probe
    /// `Temp/opencode/resprobe`).
    #[test]
    fn gl_texture_settings_match_the_reference_runtime() {
        let fixture = include_str!("gl_texture_settings_fixture.txt");
        let mut rows = 0;
        for line in fixture.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let parts: Vec<&str> = line.split('|').collect();
            assert_eq!(parts[0], "G", "{line}");
            let has_wrap_mode = parse_bool(parts[2]);
            let has_wrap_u = parse_bool(parts[3]);
            let has_wrap_v = parse_bool(parts[4]);
            let has_wrap_w = parse_bool(parts[5]);
            let presence = |present: bool| present.then_some(0);

            let mut settings = GLTextureSettingsView {
                wrap_mode: presence(has_wrap_mode),
                wrap_u: presence(has_wrap_u),
                wrap_v: presence(has_wrap_v),
                wrap_w: presence(has_wrap_w),
                ..GLTextureSettingsView::default()
            };
            settings.set_to_default();
            assert_eq!(
                settings.filter_mode,
                parts[6].parse::<i32>().unwrap(),
                "{line}"
            );
            assert_eq!(settings.aniso, parts[7].parse::<i32>().unwrap(), "{line}");
            assert_eq!(
                settings.mip_bias.to_bits(),
                parts[8].parse::<u32>().unwrap(),
                "{line}"
            );
            assert_eq!(
                settings.wrap_mode.unwrap_or(0),
                parts[9].parse::<i32>().unwrap(),
                "{line}"
            );
            assert_eq!(
                settings.wrap_u.unwrap_or(0),
                parts[10].parse::<i32>().unwrap(),
                "{line}"
            );
            assert_eq!(
                settings.wrap_v.unwrap_or(0),
                parts[11].parse::<i32>().unwrap(),
                "{line}"
            );
            assert_eq!(
                settings.wrap_w.unwrap_or(0),
                parts[12].parse::<i32>().unwrap(),
                "{line}"
            );

            let source = GLTextureSettingsView {
                filter_mode: 2,
                aniso: 3,
                mip_bias: 4.5,
                wrap_mode: presence(has_wrap_mode).map(|_| 1),
                wrap_u: presence(has_wrap_u).map(|_| 1),
                wrap_v: presence(has_wrap_v).map(|_| 0),
                wrap_w: presence(has_wrap_w).map(|_| 5),
            };
            let mut dest = GLTextureSettingsView {
                wrap_mode: presence(has_wrap_mode),
                wrap_u: presence(has_wrap_u),
                wrap_v: presence(has_wrap_v),
                wrap_w: presence(has_wrap_w),
                ..GLTextureSettingsView::default()
            };
            dest.copy_values(&source);
            assert_eq!(
                dest.filter_mode,
                parts[13].parse::<i32>().unwrap(),
                "{line}"
            );
            assert_eq!(dest.aniso, parts[14].parse::<i32>().unwrap(), "{line}");
            assert_eq!(
                dest.mip_bias.to_bits(),
                parts[15].parse::<u32>().unwrap(),
                "{line}"
            );
            assert_eq!(
                dest.wrap_mode.unwrap_or(0),
                parts[16].parse::<i32>().unwrap(),
                "{line}"
            );
            assert_eq!(
                dest.wrap_u.unwrap_or(0),
                parts[17].parse::<i32>().unwrap(),
                "{line}"
            );
            assert_eq!(
                dest.wrap_v.unwrap_or(0),
                parts[18].parse::<i32>().unwrap(),
                "{line}"
            );
            assert_eq!(
                dest.wrap_w.unwrap_or(0),
                parts[19].parse::<i32>().unwrap(),
                "{line}"
            );
            rows += 1;
        }
        assert_eq!(rows, 3);
    }
}
