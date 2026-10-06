//! `Bounds` and `AABBExtensions` vertex-array bounds.
//!
//! Mirrors `AssetRipper.SourceGenerated.Extensions/Bounds.cs` and `AABBExtensions.cs`. The bounds
//! are the axis-aligned min/max box: center `(max + min) / 2`, extent `(max - min) / 2`, and a
//! default (zero) box for an empty vertex array.
//!
//! Differential fixture: `bounds_fixture.txt` (probe `Temp/opencode/miscprobe`).

/// A bounds value.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Bounds {
    /// `Center`.
    pub center: [f32; 3],
    /// `Extent`.
    pub extent: [f32; 3],
}

/// `Bounds.CalculateFromVertexArray`.
pub fn calculate_from_vertex_array(vertices: &[[f32; 3]]) -> Bounds {
    let Some(first) = vertices.first() else {
        return Bounds::default();
    };
    let mut min = *first;
    let mut max = *first;
    for vertex in &vertices[1..] {
        for axis in 0..3 {
            if vertex[axis] < min[axis] {
                min[axis] = vertex[axis];
            }
            if vertex[axis] > max[axis] {
                max[axis] = vertex[axis];
            }
        }
    }
    Bounds {
        center: [
            (max[0] + min[0]) / 2.0,
            (max[1] + min[1]) / 2.0,
            (max[2] + min[2]) / 2.0,
        ],
        extent: [
            (max[0] - min[0]) / 2.0,
            (max[1] - min[1]) / 2.0,
            (max[2] - min[2]) / 2.0,
        ],
    }
}

impl Bounds {
    /// `Bounds.CopyTo`: write the center and extent back into an AABB view.
    pub fn copy_to(&self, destination: &mut AabbView) {
        destination.copy_values_from(self.center, self.extent);
    }
}

impl From<AabbView> for Bounds {
    /// The implicit `Bounds(AABB)` conversion: read the live center and extent.
    fn from(value: AabbView) -> Self {
        Bounds {
            center: value.center,
            extent: value.extent,
        }
    }
}

/// An AABB view.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AabbView {
    /// `Center`.
    pub center: [f32; 3],
    /// `Extent`.
    pub extent: [f32; 3],
}

impl AabbView {
    /// `AABBExtensions.Reset`.
    pub fn reset(&mut self) {
        self.center = [0.0; 3];
        self.extent = [0.0; 3];
    }

    /// `AABBExtensions.CopyValuesFrom`.
    pub fn copy_values_from(&mut self, center: [f32; 3], extent: [f32; 3]) {
        self.center = center;
        self.extent = extent;
    }

    /// `AABBExtensions.CalculateFromVertexArray`.
    pub fn calculate_from_vertex_array(&mut self, vertices: &[[f32; 3]]) {
        let bounds = calculate_from_vertex_array(vertices);
        self.copy_values_from(bounds.center, bounds.extent);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bits(values: [f32; 3]) -> [u32; 3] {
        [
            values[0].to_bits(),
            values[1].to_bits(),
            values[2].to_bits(),
        ]
    }

    fn vertices_for(count: usize) -> Vec<[f32; 3]> {
        match count {
            0 => Vec::new(),
            1 => vec![[1.0, 2.0, 3.0]],
            2 => vec![[1.0, 2.0, 3.0], [-4.0, 5.0, -6.0]],
            3 => vec![[0.0, 0.0, 0.0], [2.0, 4.0, 6.0], [1.0, 1.0, 1.0]],
            other => panic!("unexpected vertex count {other}"),
        }
    }

    /// Byte-for-byte differential against the reference `Bounds.CalculateFromVertexArray` (probe
    /// `Temp/opencode/miscprobe`). `AABBExtensions.CalculateFromVertexArray` uses the same
    /// algorithm and is asserted alongside.
    #[test]
    fn bounds_match_the_reference_runtime() {
        let fixture = include_str!("bounds_fixture.txt");
        let mut rows = 0;
        for line in fixture.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let parts: Vec<&str> = line.split('|').collect();
            assert_eq!(parts[0], "B", "{line}");
            let count: usize = parts[1].parse().unwrap();
            let bounds = calculate_from_vertex_array(&vertices_for(count));
            assert_eq!(
                bits(bounds.center),
                [
                    parts[2].parse::<u32>().unwrap(),
                    parts[3].parse::<u32>().unwrap(),
                    parts[4].parse::<u32>().unwrap(),
                ],
                "{line}"
            );
            assert_eq!(
                bits(bounds.extent),
                [
                    parts[5].parse::<u32>().unwrap(),
                    parts[6].parse::<u32>().unwrap(),
                    parts[7].parse::<u32>().unwrap(),
                ],
                "{line}"
            );

            let mut aabb = AabbView {
                center: [9.0; 3],
                extent: [9.0; 3],
            };
            aabb.calculate_from_vertex_array(&vertices_for(count));
            assert_eq!(bits(aabb.center), bits(bounds.center), "{line}");
            assert_eq!(bits(aabb.extent), bits(bounds.extent), "{line}");
            rows += 1;
        }
        assert_eq!(rows, 4);

        let mut aabb = AabbView {
            center: [1.0; 3],
            extent: [1.0; 3],
        };
        aabb.reset();
        assert_eq!(aabb, AabbView::default());
    }

    #[test]
    fn bounds_and_aabb_convert_without_reinterpretation() {
        let aabb = AabbView {
            center: [1.0, -2.0, 3.0],
            extent: [4.0, 5.0, -6.0],
        };
        let bounds = Bounds::from(aabb);
        assert_eq!(bits(bounds.center), bits(aabb.center));
        assert_eq!(bits(bounds.extent), bits(aabb.extent));

        let mut target = AabbView::default();
        bounds.copy_to(&mut target);
        assert_eq!(target, aabb);
    }
}
