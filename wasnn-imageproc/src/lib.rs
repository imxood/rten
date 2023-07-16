//! Functions for pre and post-processing images.

use std::fmt::Display;

use wasnn_tensor::{MatrixLayout, NdTensorView, NdTensorViewMut};

mod contours;
mod math;
mod poly_algos;
mod shapes;

pub use contours::{find_contours, RetrievalMode};
pub use math::Vec2;
pub use poly_algos::{convex_hull, min_area_rect, simplify_polygon, simplify_polyline};
pub use shapes::{bounding_rect, BoundingRect, Line, Point, Polygon, Polygons, Rect, RotatedRect};

/// Print out elements of a 2D grid for debugging.
#[allow(dead_code)]
fn print_grid<T: Display>(grid: NdTensorView<T, 2>) {
    for y in 0..grid.rows() {
        for x in 0..grid.cols() {
            print!("{:2} ", grid[[y, x]]);
        }
        println!();
    }
    println!();
}

// Draw the outline of a rectangle `rect` with border width `width`.
//
// The outline is drawn such that the bounding box of the outermost pixels
// will be `rect`.
pub fn stroke_rect<T: Copy>(mut mask: NdTensorViewMut<T, 2>, rect: Rect, value: T, width: u32) {
    let width = width as i32;

    // Left edge
    fill_rect(
        mask.view_mut(),
        Rect::from_tlbr(rect.top(), rect.left(), rect.bottom(), rect.left() + width),
        value,
    );

    // Top edge (minus ends)
    fill_rect(
        mask.view_mut(),
        Rect::from_tlbr(
            rect.top(),
            rect.left() + width,
            rect.top() + width,
            rect.right() - width,
        ),
        value,
    );

    // Right edge
    fill_rect(
        mask.view_mut(),
        Rect::from_tlbr(
            rect.top(),
            rect.right() - width,
            rect.bottom(),
            rect.right(),
        ),
        value,
    );

    // Bottom edge (minus ends)
    fill_rect(
        mask.view_mut(),
        Rect::from_tlbr(
            rect.bottom() - width,
            rect.left() + width,
            rect.bottom(),
            rect.right() - width,
        ),
        value,
    );
}

/// Fill all points inside `rect` with the value `value`.
pub fn fill_rect<T: Copy>(mut mask: NdTensorViewMut<T, 2>, rect: Rect, value: T) {
    for y in rect.top()..rect.bottom() {
        for x in rect.left()..rect.right() {
            mask[[y as usize, x as usize]] = value;
        }
    }
}

/// Return a copy of `p` with X and Y coordinates clamped to `[0, width)` and
/// `[0, height)` respectively.
fn clamp_to_bounds(p: Point, height: i32, width: i32) -> Point {
    Point::from_yx(
        p.y.clamp(0, height.saturating_sub(1).max(0)),
        p.x.clamp(0, width.saturating_sub(1).max(0)),
    )
}

/// Iterator over points that lie on a line, as determined by the Bresham
/// algorithm.
///
/// The implementation in [Pillow](https://pillow.readthedocs.io/en/stable/) was
/// used as a reference.
struct BreshamPoints {
    /// Next point to return
    current: Point,

    /// Remaining points to return
    remaining_steps: u32,

    /// Twice total change in X along line
    dx: i32,

    /// Twice total change in Y along line
    dy: i32,

    /// Tracks error between integer points yielded by this iterator and the
    /// "true" coordinate.
    error: i32,

    /// Increment to X coordinate of `current`.
    x_step: i32,

    /// Increment to Y coordinate of `current`.
    y_step: i32,
}

impl BreshamPoints {
    fn new(l: Line) -> BreshamPoints {
        let dx = (l.end.x - l.start.x).abs();
        let dy = (l.end.y - l.start.y).abs();

        BreshamPoints {
            current: l.start,
            remaining_steps: dx.max(dy) as u32,

            // dx and dy are doubled here as it makes stepping simpler.
            dx: dx * 2,
            dy: dy * 2,

            error: if dx >= dy { dy * 2 - dx } else { dx * 2 - dy },
            x_step: (l.end.x - l.start.x).signum(),
            y_step: (l.end.y - l.start.y).signum(),
        }
    }
}

impl Iterator for BreshamPoints {
    type Item = Point;

    fn next(&mut self) -> Option<Point> {
        if self.remaining_steps == 0 {
            return None;
        }

        let current = self.current;
        self.remaining_steps -= 1;

        if self.x_step == 0 {
            // Vertical line
            self.current.y += self.y_step;
        } else if self.y_step == 0 {
            // Horizontal line
            self.current.x += self.x_step;
        } else if self.dx >= self.dy {
            // X-major line (width >= height). Advances X on each step and
            // advances Y on some steps.
            if self.error >= 0 {
                self.current.y += self.y_step;
                self.error -= self.dx;
            }
            self.error += self.dy;
            self.current.x += self.x_step;
        } else {
            // Y-major line (height > width). Advances Y on each step and
            // advances X on some steps.
            if self.error >= 0 {
                self.current.x += self.x_step;
                self.error -= self.dy
            }
            self.error += self.dx;
            self.current.y += self.y_step;
        }

        Some(current)
    }
}

/// Draw a non-antialiased line in an image.
pub fn draw_line<T: Copy>(mut image: NdTensorViewMut<T, 2>, line: Line, value: T) {
    // This function uses Bresham's line algorithm, with the implementation
    // in Pillow (https://pillow.readthedocs.io/en/stable/) used as a reference.
    let height: i32 = image.rows().try_into().unwrap();
    let width: i32 = image.cols().try_into().unwrap();

    let start = clamp_to_bounds(line.start, height, width);
    let end = clamp_to_bounds(line.end, height, width);
    let clamped = Line::from_endpoints(start, end);

    for p in BreshamPoints::new(clamped) {
        image[p.coord()] = value;
    }
}

/// Draw the outline of a non anti-aliased polygon in an image.
pub fn draw_polygon<T: Copy>(mut image: NdTensorViewMut<T, 2>, poly: &[Point], value: T) {
    for edge in Polygon::new(poly).edges() {
        draw_line(image.view_mut(), edge, value);
    }
}

/// Tracks data about an edge in a polygon being traversed by [FillIter].
#[derive(Clone, Copy, Debug)]
struct Edge {
    /// Y coordinate where this edge starts
    start_y: i32,

    /// Number of scanlines remaining for this edge
    y_steps: u32,

    /// X coordinate where this edge intersects the current scanline
    x: i32,

    /// Error term indicating difference between true X coordinate for current
    /// scanline and `x`.
    error: i32,

    /// Amount to increment `error` for every scanline.
    error_incr: i32,

    /// Amount to decrement `error` when it becomes positive.
    error_decr: i32,

    /// Amount to increment `x` for every scanline.
    x_step: i32,

    /// Amount to increment `x` when `error` becomes positive.
    extra_x_step: i32,
}

/// Iterator over coordinates of pixels that fill a polygon. See
/// [Polygon::fill_iter] for notes on how this iterator determines which
/// pixels are inside the polygon.
///
/// The implementation follows <https://www.jagregory.com/abrash-black-book/#filling-arbitrary-polygons>.
pub struct FillIter {
    /// Edges in the polygon, sorted by Y coordinate.
    edges: Vec<Edge>,

    /// Edges in the polygon which intersect the horizontal line at `cursor.y`.
    ///
    /// Sorted by X coordinate.
    active_edges: Vec<Edge>,

    /// Bounding rect that contains the polygon.
    bounds: Rect,

    /// Coordinates of next pixel to return.
    cursor: Point,
}

impl FillIter {
    fn new(poly: Polygon<&[Point]>) -> FillIter {
        let mut edges: Vec<_> = poly
            .edges()
            // Ignore horizontal edges
            .filter(|e| e.start.y != e.end.y)
            .map(|e| {
                // Normalize edge so that `delta_y` is +ve
                let (start, end) = if e.start.y <= e.end.y {
                    (e.start, e.end)
                } else {
                    (e.end, e.start)
                };

                let delta_x = end.x - start.x;
                let delta_y = end.y - start.y;

                Edge {
                    start_y: start.y,
                    y_steps: delta_y as u32,

                    x: start.x,

                    // `x_step` is the integer part of `1/slope`.
                    x_step: delta_x / delta_y,

                    // The error term tracks when `x` needs an adjustment due
                    // to accumulation of the fractional part of `1/slope`.
                    error: if delta_x >= 0 {
                        0
                    } else {
                        // TODO - Clarify where this comes from.
                        -delta_y + 1
                    },
                    error_incr: delta_x.abs() % delta_y,
                    error_decr: delta_y,
                    extra_x_step: delta_x.signum(),
                }
            })
            .collect();
        edges.sort_by_key(|e| -e.start_y);

        let active_edges = Vec::with_capacity(edges.len());

        let bounds = poly.bounding_rect();
        let mut iter = FillIter {
            edges,
            active_edges,
            bounds,
            cursor: if bounds.is_empty() {
                // If the polygon is empty, the cursor starts at the bottom right
                // so that the iterator immediately yields `None`, rather than
                // having to loop over all the empty rows.
                bounds.bottom_right()
            } else {
                bounds.top_left()
            },
        };
        iter.update_active_edges();

        iter
    }

    /// Update the `active_edges` list after moving to a new line.
    fn update_active_edges(&mut self) {
        // Remove entries that end at this line and update X coord of other entries.
        self.active_edges.retain_mut(|mut e| {
            e.y_steps -= 1;
            if e.y_steps > 0 {
                // Advance X coordinate for current line and error term that
                // tracks difference between `e.x` and true X coord.
                e.x += e.x_step;
                e.error += e.error_incr;
                if e.error > 0 {
                    e.error -= e.error_decr;
                    e.x += e.extra_x_step;
                }
                true
            } else {
                false
            }
        });

        // Add edges that begin at this line.
        while let Some(edge) = self.edges.last().copied() {
            if edge.start_y > self.cursor.y {
                // `self.edges` is sorted on Y coordinate, so remaining entries
                // start on lines with higher Y coordinate than cursor.
                break;
            }
            self.edges.pop();
            self.active_edges.push(edge);
        }

        // Sort edges by X coordinate of intersection with scanline. We only
        // need to sort by `e.x`, but including other elements in the sort key
        // provides more predictable ordering for debugging.
        self.active_edges
            .sort_by_key(|e| (e.x, e.x_step, e.extra_x_step));
    }
}

impl Iterator for FillIter {
    type Item = Point;

    fn next(&mut self) -> Option<Point> {
        while !self.active_edges.is_empty() {
            let current = self.cursor;
            let intersections =
                self.active_edges
                    .iter()
                    .fold(0, |i, e| if e.x <= current.x { i + 1 } else { i });

            self.cursor.move_by(0, 1);
            if self.cursor.x == self.bounds.right() {
                self.cursor.move_to(current.y + 1, self.bounds.left());
                self.update_active_edges();
            }

            if intersections % 2 == 1 {
                return Some(current);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use wasnn_tensor::{Layout, MatrixLayout, NdTensor, NdTensorView, NdTensorViewMut};

    use super::{draw_polygon, print_grid, stroke_rect, BoundingRect, Point, Polygon, Rect};

    /// Return a list of the points on the border of `rect`, in counter-clockwise
    /// order starting from the top-left corner.
    ///
    /// If `omit_corners` is true, the corner points of the rect are not
    /// included.
    pub fn border_points(rect: Rect, omit_corners: bool) -> Vec<Point> {
        let mut points = Vec::new();

        let left_range = if omit_corners {
            rect.top() + 1..rect.bottom() - 1
        } else {
            rect.top()..rect.bottom()
        };

        // Left edge
        for y in left_range.clone() {
            points.push(Point::from_yx(y, rect.left()));
        }

        // Bottom edge
        for x in rect.left() + 1..rect.right() - 1 {
            points.push(Point::from_yx(rect.bottom() - 1, x));
        }

        // Right edge
        for y in left_range.rev() {
            points.push(Point::from_yx(y, rect.right() - 1));
        }

        // Top edge
        for x in (rect.left() + 1..rect.right() - 1).rev() {
            points.push(Point::from_yx(rect.top(), x));
        }

        points
    }

    /// Set the elements of a grid listed in `points` to `value`.
    #[allow(dead_code)]
    fn plot_points<T: Copy>(mut grid: NdTensorViewMut<T, 2>, points: &[Point], value: T) {
        for point in points {
            grid[point.coord()] = value;
        }
    }

    /// Plot the 1-based indices of points in `points` on a grid. `step` is the
    /// increment value for each plotted point.
    #[allow(dead_code)]
    fn plot_point_indices<T: std::ops::AddAssign + Copy + Default>(
        mut grid: NdTensorViewMut<T, 2>,
        points: &[Point],
        step: T,
    ) {
        let mut value = T::default();
        value += step;
        for point in points {
            grid[point.coord()] = value;
            value += step;
        }
    }

    /// Return coordinates of all points in `grid` with a non-zero value.
    fn nonzero_points<T: Default + PartialEq>(grid: NdTensorView<T, 2>) -> Vec<Point> {
        let mut points = Vec::new();
        for y in 0..grid.rows() {
            for x in 0..grid.cols() {
                if grid[[y, x]] != T::default() {
                    points.push(Point::from_yx(y as i32, x as i32))
                }
            }
        }
        points
    }

    /// Create a 2D NdTensor from an MxN nested array.
    fn image_from_2d_array<const M: usize, const N: usize>(xs: [[i32; N]; M]) -> NdTensor<i32, 2> {
        let mut image = NdTensor::zeros([M, N]);
        for y in 0..M {
            for x in 0..N {
                image[[y, x]] = xs[y][x];
            }
        }
        image
    }

    /// Compare two single-channel images with i32 pixel values.
    fn compare_images(a: NdTensorView<i32, 2>, b: NdTensorView<i32, 2>) {
        assert_eq!(a.rows(), b.rows());
        assert_eq!(a.cols(), b.cols());

        for y in 0..a.rows() {
            for x in 0..a.cols() {
                if a[[y, x]] != b[[y, x]] {
                    print_grid(a);
                    panic!("mismatch at coord [{}, {}]", y, x);
                }
            }
        }
    }

    /// Convert a slice of `[y, x]` coordinates to `Point`s
    pub fn points_from_coords(coords: &[[i32; 2]]) -> Vec<Point> {
        coords.iter().map(|[y, x]| Point::from_yx(*y, *x)).collect()
    }

    /// Convery an array of `[y, x]` coordinates to `Point`s
    pub fn points_from_n_coords<const N: usize>(coords: [[i32; 2]; N]) -> [Point; N] {
        coords.map(|[y, x]| Point::from_yx(y, x))
    }

    #[test]
    fn test_draw_polygon() {
        struct Case {
            points: &'static [[i32; 2]],
            expected: NdTensor<i32, 2>,
        }

        let cases = [
            // A simple rect: Straight lines in each direction
            Case {
                points: &[[0, 0], [0, 4], [4, 4], [4, 0]],
                expected: image_from_2d_array([
                    [1, 1, 1, 1, 1],
                    [1, 0, 0, 0, 1],
                    [1, 0, 0, 0, 1],
                    [1, 0, 0, 0, 1],
                    [1, 1, 1, 1, 1],
                ]),
            },
            // Slopes in each direction.
            Case {
                points: &[[0, 2], [2, 0], [4, 2], [2, 4]],
                expected: image_from_2d_array([
                    [0, 0, 1, 0, 0],
                    [0, 1, 0, 1, 0],
                    [1, 0, 0, 0, 1],
                    [0, 1, 0, 1, 0],
                    [0, 0, 1, 0, 0],
                ]),
            },
            // Steep slopes in each direction.
            Case {
                points: &[[0, 2], [2, 1], [4, 2], [2, 3]],
                expected: image_from_2d_array([
                    [0, 0, 1, 0, 0],
                    [0, 1, 1, 0, 0],
                    [0, 1, 0, 1, 0],
                    [0, 0, 1, 1, 0],
                    [0, 0, 1, 0, 0],
                ]),
            },
        ];

        for case in cases {
            let points: Vec<_> = case
                .points
                .iter()
                .map(|[y, x]| Point::from_yx(*y, *x))
                .collect();

            let mut image = NdTensor::zeros(case.expected.shape());
            draw_polygon(image.view_mut(), &points, 1);
            compare_images(image.view(), case.expected.view());
        }
    }

    #[test]
    fn test_stroke_rect() {
        let mut mask = NdTensor::zeros([10, 10]);
        let rect = Rect::from_tlbr(4, 4, 9, 9);

        stroke_rect(mask.view_mut(), rect, 1, 1);
        let points = nonzero_points(mask.view());

        assert_eq!(
            Polygon::new(&points).bounding_rect(),
            rect.adjust_tlbr(0, 0, -1, -1)
        );
    }
}
