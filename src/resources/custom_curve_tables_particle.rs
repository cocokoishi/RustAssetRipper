include!("custom_curve_tables_particle_01.rs");
include!("custom_curve_tables_particle_02.rs");

static PARTICLE_SYSTEM_PATHS_STORAGE: [(u32, &str); 1792] =
    combine_particle_system_paths();

const fn combine_particle_system_paths() -> [(u32, &'static str); 1792] {
    let mut combined = [(0, ""); 1792];
    let mut index = 0;
    while index < PARTICLE_SYSTEM_PATHS_PART_01.len() {
        combined[index] = PARTICLE_SYSTEM_PATHS_PART_01[index];
        index += 1;
    }

    let mut index = 0;
    while index < PARTICLE_SYSTEM_PATHS_PART_02.len() {
        combined[PARTICLE_SYSTEM_PATHS_PART_01.len() + index] =
            PARTICLE_SYSTEM_PATHS_PART_02[index];
        index += 1;
    }
    combined
}

/// <ParticleSystem>.s_FieldPathDictionary, sorted by hash.
pub const PARTICLE_SYSTEM_PATHS: &[(u32, &str)] = &PARTICLE_SYSTEM_PATHS_STORAGE;
