use std::{
    collections::HashSet,
    convert::TryInto,
    ops::{Index, IndexMut},
};

use flo_curves::{bezier::Curve, BezierCurve, Coord2, Coordinate2D};
use visioniechor::{BinaryImage, BoundingRect, CompoundPath, PointF64, PointI32, PointUsize};

#[derive(Clone, Copy, PartialEq)]
pub enum FilledHoleElement {
    Blank,
    Structure,
    Texture,
}

pub struct FilledHoleMatrix {
    pub width: usize,
    pub height: usize,
    pub elems: Vec<FilledHoleElement>,
}

impl FilledHoleMatrix {
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            elems: vec![FilledHoleElement::Blank; width * height],
        }
    }

    pub fn new_without_column(&self, col: usize) -> Self {
        let mut matrix = Self::new(self.width - 1, self.height);
        for i in 0..matrix.height {
            for j in 0..matrix.width {
                matrix[i][j] = self[i][if j < col { j } else { j + 1 }];
            }
        }
        matrix
    }

    pub fn new_without_row(&self, row: usize) -> Self {
        let mut matrix = Self::new(self.width, self.height - 1);
        for i in 0..matrix.height {
            for j in 0..matrix.width {
                matrix[i][j] = self[if i < row { i } else { i + 1 }][j];
            }
        }
        matrix
    }
}

impl Index<usize> for FilledHoleMatrix {
    type Output = [FilledHoleElement]; // Output a row for further indexing

    fn index(&self, index: usize) -> &Self::Output {
        &self.elems[(index * self.width)..((index + 1) * self.width)]
    }
}

impl IndexMut<usize> for FilledHoleMatrix {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        &mut self.elems[(index * self.width)..((index + 1) * self.width)]
    }
}

impl Index<PointUsize> for FilledHoleMatrix {
    type Output = FilledHoleElement; // Output a row for further indexing

    fn index(&self, index: PointUsize) -> &Self::Output {
        &self.elems[index.y * self.width + index.x]
    }
}

impl IndexMut<PointUsize> for FilledHoleMatrix {
    fn index_mut(&mut self, index: PointUsize) -> &mut Self::Output {
        &mut self.elems[index.y * self.width + index.x]
    }
}

/// A class to fill colors into image whose structural information has been recovered.
pub struct HoleFiller;

// API
impl HoleFiller {
    /// Return a FilledHoleMatrix representing what is inside the hole after filling.
    /// The behavior is undefined unless the size of 'image' is at least the size
    /// of 'hole_rect'.
    pub fn fill(
        image: &BinaryImage,
        hole_rect: BoundingRect,
        intrapolated_curves: Vec<CompoundPath>,
        endpoints: Vec<PointI32>,
        blank_broundary_pixels_threshold: usize,
    ) -> Result<FilledHoleMatrix, String> {
        let matrix = FilledHoleMatrix::new(hole_rect.width() as usize, hole_rect.height() as usize);
        let origin = PointI32::new(hole_rect.left, hole_rect.top);

        let matrix = Self::rasterize_intrapolated_curves(matrix, intrapolated_curves, origin);

        Self::fill_holes(
            matrix,
            image,
            hole_rect,
            origin,
            endpoints,
            blank_broundary_pixels_threshold,
        )
    }
}

// Helper functions
impl HoleFiller {
    fn rasterize_intrapolated_curves(
        mut matrix: FilledHoleMatrix,
        curves: Vec<CompoundPath>,
        origin: PointI32,
    ) -> FilledHoleMatrix {
        let offset = -origin;
        curves.into_iter().for_each(|mut compound_path| {
            compound_path
                .iter_mut()
                .for_each(|path_elem| match path_elem {
                    visioniechor::CompoundPathElement::PathI32(path) => {
                        path.offset(&offset);
                        path.iter().for_each(|point| {
                            let point = PointUsize::new(point.x as usize, point.y as usize);
                            matrix[point] = FilledHoleElement::Structure;
                        })
                    }
                    visioniechor::CompoundPathElement::PathF64(path) => {
                        path.offset(&offset.to_point_f64());
                        path.iter().for_each(|point| {
                            let point = PointUsize::new(point.x as usize, point.y as usize);
                            matrix[point] = FilledHoleElement::Structure;
                        })
                    }
                    visioniechor::CompoundPathElement::Spline(spline) => {
                        spline.offset(&offset.to_point_f64());
                        spline.get_control_points().into_iter().for_each(|points| {
                            Self::rasterize_bezier_curve(
                                &mut matrix,
                                points
                                    .try_into()
                                    .expect("Control points must have 4 elements"),
                            );
                        });
                    }
                });
        });

        matrix
    }

    fn rasterize_bezier_curve(matrix: &mut FilledHoleMatrix, control_points: [PointF64; 4]) {
        let points: Vec<Coord2> = control_points.iter().map(|p| Coord2(p.x, p.y)).collect();

        let curve = Curve {
            start_point: points[0],
            end_point: points[3],
            control_points: (points[1], points[2]),
        };
        let quantization_levels = (curve.estimate_length() as usize) << 2;

        for i in 0..quantization_levels {
            let t = i as f64 / quantization_levels as f64;
            let p = curve.point_at_pos(t);
            let clipped_p = PointUsize::new(
                std::cmp::min(p.x() as usize, matrix.width - 1),
                std::cmp::min(p.y() as usize, matrix.height - 1),
            );
            matrix[clipped_p] = FilledHoleElement::Structure;
        }
    }

    /// The behavior is undefined unless 'offset' is the top-left corner of 'hole_rect' (exactly on its boundary).
    fn fill_holes(
        mut matrix: FilledHoleMatrix,
        image: &BinaryImage,
        hole_rect: BoundingRect,
        offset: PointI32,
        endpoints: Vec<PointI32>,
        blank_boundary_pixels_threshold: usize,
    ) -> Result<FilledHoleMatrix, String> {
        let endpoints = Self::adjust_endpoints(&hole_rect, endpoints);

        let bounding_points = hole_rect.get_boundary_points_from(endpoints[0], true);
        let num_points = bounding_points.len();
        let mut current_point = 0;
        // The middle point between from and to in a cyclic manner.
        // Used to sample the middle point between endpoints.
        let sample_point = |from: usize, to: usize| {
            let cyclic_dist = if to >= from {
                to - from
            } else {
                num_points - (from - to)
            };
            (from + (cyclic_dist >> 1)) % num_points
        };

        let endpoints_set = endpoints.iter().copied().collect::<HashSet<PointI32>>();
        let is_endpoint = |p| endpoints_set.contains(&p);

        let eval_outside_point = |point_idx| {
            let point_val: PointI32 = bounding_points[point_idx];
            if point_val.x == hole_rect.right || point_val.y == hole_rect.bottom {
                point_val
            } else {
                hole_rect.get_closest_point_outside(point_val)
            }
        };

        let eval_inside_point = |point_idx| {
            let point_val: PointI32 = bounding_points[point_idx];
            if point_val.x == hole_rect.left || point_val.y == hole_rect.top {
                point_val
            } else {
                hole_rect.get_closest_point_inside(point_val)
            }
        };

        // Go to next segment. Fill it if it should be filled.
        // Repeat this until the first endpoint is seen again.
        loop {
            // Not back to the first endpoint yet
            let prev_endpoint = current_point;
            let mut total_outside_pixels = 0_usize;
            let mut blank_outside_pixels = 0_usize;
            loop {
                current_point = (current_point + 1) % num_points;
                total_outside_pixels += 1;
                let outside_point = eval_outside_point(current_point);
                if !image.get_pixel_at_safe(outside_point) {
                    blank_outside_pixels += 1;
                }
                if is_endpoint(bounding_points[current_point]) {
                    break;
                }
            }
            if total_outside_pixels > blank_boundary_pixels_threshold && blank_outside_pixels <= blank_boundary_pixels_threshold {
                let sampled_mid_point = sample_point(prev_endpoint, current_point);
                let sampled_points = [
                    sample_point(prev_endpoint, sampled_mid_point),
                    sampled_mid_point,
                    sample_point(sampled_mid_point, current_point),
                ];

                IntoIterator::into_iter(sampled_points).for_each(|sampled_point| {
                    let inside_point = eval_inside_point(sampled_point);
                    Self::fill_hole_iterative(&mut matrix, inside_point - offset);
                });
            }

            if current_point == 0 {
                break;
            }

            current_point = (current_point + 1) % num_points;
        }

        Ok(matrix)
    }

    /// Correction for endpoints off boundary
    fn adjust_endpoints(hole_rect: &BoundingRect, endpoints: Vec<PointI32>) -> Vec<PointI32> {
        endpoints
            .into_iter()
            .map(|endpoint| {
                if hole_rect.have_point_on_boundary(endpoint, 0) {
                    endpoint
                } else {
                    // Determine if endpoint is vertically or horizontally aligned with the rect
                    if hole_rect.left <= endpoint.x && endpoint.x <= hole_rect.right {
                        // Should be adjusted to either top or bottom side
                        PointI32::new(
                            endpoint.x,
                            if (hole_rect.top - endpoint.y).abs()
                                < (hole_rect.bottom - endpoint.y).abs()
                            {
                                hole_rect.top
                            } else {
                                hole_rect.bottom
                            },
                        )
                    } else if hole_rect.top <= endpoint.y && endpoint.y <= hole_rect.bottom {
                        // Should be adjusted to either left and right side
                        PointI32::new(
                            if (hole_rect.left - endpoint.x).abs()
                                < (hole_rect.right - endpoint.x).abs()
                            {
                                hole_rect.left
                            } else {
                                hole_rect.right
                            },
                            endpoint.y,
                        )
                    } else {
                        // Should be adjusted to one of the corners
                        IntoIterator::into_iter([
                            hole_rect.top_left(),
                            hole_rect.top_right(),
                            hole_rect.bottom_left(),
                            hole_rect.bottom_right(),
                        ])
                        .min_by_key(|&corner| {
                            endpoint.to_point_f64().distance_to(corner.to_point_f64()) as i32
                        })
                        .unwrap()
                    }
                }
            })
            .collect()
    }

    /// Flood fill a region of FilledHoleElement::Blank starting at 'seed' in an iterative manner.
    fn fill_hole_iterative(matrix: &mut FilledHoleMatrix, seed: PointI32) {
        let mut stack = vec![seed];
        while !stack.is_empty() {
            let point = stack.pop().unwrap();

            // Out of range
            if point.x < 0
                || point.x >= matrix.width as i32
                || point.y < 0
                || point.y >= matrix.height as i32
            {
                continue;
            }

            let point_usize = point.to_point_usize();

            // Already filled
            if matrix[point_usize] != FilledHoleElement::Blank {
                continue;
            }

            // Fill it
            matrix[point_usize] = FilledHoleElement::Texture;

            IntoIterator::into_iter([
                point + PointI32::new(1, 0),
                point + PointI32::new(0, 1),
                point + PointI32::new(-1, 0),
                point + PointI32::new(0, -1),
            ])
            .for_each(|neighbor| {
                stack.push(neighbor);
            });
        }
    }
}
