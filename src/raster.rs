//! A small anti-aliased vector rasterizer for the pond, so it needs no drawing crate.
//!
//! Paths are polygons in logical units, flattened from lines, quadratic curves, circles,
//! and ellipses. Filling measures each pixel's exact covered area by accumulating signed
//! edge areas along each row, so edges are smooth at any angle. Overlapping contours of
//! the same winding union rather than add, which lets a stroke be built as a union of
//! segment quads and round joins. Paints are flat colors or linear and radial gradients,
//! composited over, multiplied, or added. A [`Mask`] holds the coverage of one shape for
//! clipping or, blurred, for a soft shadow. Captions come from glyph masks prerendered
//! from Inter into `assets/caption-font.bin`.
//!
//! Colors are straight-alpha sRGB with channels from zero to one; a [`Canvas`] stores
//! premultiplied pixels. Everything here is deterministic and performs no I/O.

use std::collections::BTreeMap;
use std::sync::OnceLock;

/// A straight-alpha sRGB color, each channel from zero to one.
pub(crate) type Rgba = [f32; 4];

/// An opaque color from `0xRRGGBB`.
pub(crate) fn hex(code: u32) -> Rgba {
    [
        ((code >> 16) & 0xff) as f32 / 255.0,
        ((code >> 8) & 0xff) as f32 / 255.0,
        (code & 0xff) as f32 / 255.0,
        1.0,
    ]
}

/// The same color at a different opacity.
pub(crate) fn with_alpha(color: Rgba, alpha: f32) -> Rgba {
    [color[0], color[1], color[2], alpha]
}

/// Interpolate every channel, alpha included.
pub(crate) fn mix(from: Rgba, to: Rgba, amount: f32) -> Rgba {
    std::array::from_fn(|channel| from[channel] + (to[channel] - from[channel]) * amount)
}

/// A point in logical units.
pub(crate) type Point = (f32, f32);

/// Closed polygons in logical units. Every contour is stored with the same winding, so
/// overlapping contours always union and a path has no holes.
#[derive(Clone, Debug, Default)]
pub(crate) struct Path {
    contours: Vec<Vec<Point>>,
    current: Vec<Point>,
}

impl Path {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn move_to(&mut self, point: Point) {
        self.finish_contour();
        self.current.push(point);
    }

    pub(crate) fn line_to(&mut self, point: Point) {
        self.current.push(point);
    }

    /// A quadratic curve from the current point, flattened finely enough that no
    /// segment spans more than about a device pixel and a half.
    pub(crate) fn quad_to(&mut self, control: Point, point: Point) {
        let start = *self.current.last().unwrap_or(&control);
        let estimate = distance(start, control) + distance(control, point);
        let steps = ((estimate / 0.75).ceil() as usize).clamp(2, 48);
        for step in 1..=steps {
            let t = step as f32 / steps as f32;
            let u = 1.0 - t;
            self.current.push((
                u * u * start.0 + 2.0 * u * t * control.0 + t * t * point.0,
                u * u * start.1 + 2.0 * u * t * control.1 + t * t * point.1,
            ));
        }
    }

    pub(crate) fn close(&mut self) {
        self.finish_contour();
    }

    /// A whole polygon as its own contour.
    pub(crate) fn polygon(&mut self, points: &[Point]) {
        self.finish_contour();
        self.contours.push(with_positive_winding(points.to_vec()));
    }

    pub(crate) fn circle(&mut self, center: Point, radius: f32) {
        self.ellipse(center, radius, radius, 0.0);
    }

    /// An ellipse with radii along and across `rotation` radians.
    pub(crate) fn ellipse(&mut self, center: Point, radius_x: f32, radius_y: f32, rotation: f32) {
        if radius_x <= 0.0 || radius_y <= 0.0 {
            return;
        }
        let perimeter =
            std::f32::consts::TAU * ((radius_x * radius_x + radius_y * radius_y) / 2.0).sqrt();
        let steps = ((perimeter / 0.9).ceil() as usize).clamp(10, 96);
        let (sin_rotation, cos_rotation) = rotation.sin_cos();
        let points: Vec<Point> = (0..steps)
            .map(|step| {
                let angle = step as f32 / steps as f32 * std::f32::consts::TAU;
                let (x, y) = (radius_x * angle.cos(), radius_y * angle.sin());
                (
                    center.0 + x * cos_rotation - y * sin_rotation,
                    center.1 + x * sin_rotation + y * cos_rotation,
                )
            })
            .collect();
        self.polygon(&points);
    }

    pub(crate) fn rect(&mut self, left: f32, top: f32, width: f32, height: f32) {
        self.polygon(&[
            (left, top),
            (left + width, top),
            (left + width, top + height),
            (left, top + height),
        ]);
    }

    pub(crate) fn round_rect(&mut self, left: f32, top: f32, width: f32, height: f32, radius: f32) {
        let mut points = Vec::new();
        let corners = [
            (left + width - radius, top + radius, -0.25),
            (left + width - radius, top + height - radius, 0.0),
            (left + radius, top + height - radius, 0.25),
            (left + radius, top + radius, 0.5),
        ];
        for (corner_x, corner_y, start_turn) in corners {
            for step in 0..=12 {
                let angle = (start_turn + step as f32 / 48.0) * std::f32::consts::TAU;
                points.push((
                    corner_x + radius * angle.cos(),
                    corner_y + radius * angle.sin(),
                ));
            }
        }
        self.polygon(&points);
    }

    /// A closed outline through the midpoints of `points`, curving at each point.
    pub(crate) fn smooth_closed(&mut self, points: &[Point]) {
        let count = points.len();
        if count < 3 {
            return;
        }
        self.move_to(midpoint(points[count - 1], points[0]));
        for index in 0..count {
            self.quad_to(
                points[index],
                midpoint(points[index], points[(index + 1) % count]),
            );
        }
        self.close();
    }

    /// Add another path's contours to this one.
    pub(crate) fn extend(&mut self, other: &Path) {
        self.finish_contour();
        self.contours.extend(other.contours().cloned());
    }

    fn finish_contour(&mut self) {
        if self.current.len() >= 2 {
            let contour = std::mem::take(&mut self.current);
            self.contours.push(with_positive_winding(contour));
        } else {
            self.current.clear();
        }
    }

    fn contours(&self) -> impl Iterator<Item = &Vec<Point>> {
        self.contours
            .iter()
            .chain((self.current.len() >= 2).then_some(&self.current))
    }
}

/// Reverse a contour whose signed area is negative.
fn with_positive_winding(mut contour: Vec<Point>) -> Vec<Point> {
    let signed_area: f32 = (0..contour.len())
        .map(|index| {
            let (from, to) = (contour[index], contour[(index + 1) % contour.len()]);
            from.0 * to.1 - to.0 * from.1
        })
        .sum();
    if signed_area < 0.0 {
        contour.reverse();
    }
    contour
}

fn distance(from: Point, to: Point) -> f32 {
    ((to.0 - from.0).powi(2) + (to.1 - from.1).powi(2)).sqrt()
}

pub(crate) fn midpoint(from: Point, to: Point) -> Point {
    ((from.0 + to.0) / 2.0, (from.1 + to.1) / 2.0)
}

/// An open polyline that starts and ends at the ends of `points` and curves through
/// the midpoints between them.
pub(crate) fn smooth_open(points: &[Point]) -> Vec<Point> {
    let Some(&first) = points.first() else {
        return Vec::new();
    };
    let mut path = Path::new();
    path.move_to(first);
    for index in 1..points.len().saturating_sub(1) {
        path.quad_to(points[index], midpoint(points[index], points[index + 1]));
    }
    path.line_to(points[points.len() - 1]);
    path.current
}

/// The union of round-capped, round-joined segments along `points`, `width` across.
pub(crate) fn stroke_path(points: &[Point], width: f32, closed: bool) -> Path {
    let mut stroke = Path::new();
    let half_width = width / 2.0;
    let segment_count = if closed {
        points.len()
    } else {
        points.len().saturating_sub(1)
    };
    for segment_index in 0..segment_count {
        let from = points[segment_index];
        let to = points[(segment_index + 1) % points.len()];
        let length = distance(from, to);
        if length < 1e-4 {
            continue;
        }
        let normal = (
            -(to.1 - from.1) / length * half_width,
            (to.0 - from.0) / length * half_width,
        );
        stroke.polygon(&[
            (from.0 + normal.0, from.1 + normal.1),
            (to.0 + normal.0, to.1 + normal.1),
            (to.0 - normal.0, to.1 - normal.1),
            (from.0 - normal.0, from.1 - normal.1),
        ]);
    }
    if half_width > 0.35 {
        for &point in points {
            stroke.circle(point, half_width);
        }
    }
    stroke
}

/// How a paint combines with what is already drawn.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Blend {
    /// Painted on top.
    Over,
    /// Darkens by the paint's color, like light through tinted glass.
    Multiply,
    /// Adds light.
    Plus,
}

/// What fills a shape.
#[derive(Clone, Debug)]
pub(crate) enum Paint {
    Solid(Rgba),
    /// Colors along the line from `from` to `to`, by offset from zero to one.
    Linear {
        from: Point,
        to: Point,
        stops: Vec<(f32, Rgba)>,
    },
    /// Colors outward from `center`, reaching the last stop at `radius`.
    Radial {
        center: Point,
        radius: f32,
        stops: Vec<(f32, Rgba)>,
    },
}

impl Paint {
    fn color_at(&self, point: Point) -> Rgba {
        match self {
            Self::Solid(color) => *color,
            Self::Linear { from, to, stops } => {
                let direction = (to.0 - from.0, to.1 - from.1);
                let length_squared =
                    (direction.0 * direction.0 + direction.1 * direction.1).max(1e-6);
                let offset = ((point.0 - from.0) * direction.0 + (point.1 - from.1) * direction.1)
                    / length_squared;
                sample_stops(stops, offset)
            }
            Self::Radial {
                center,
                radius,
                stops,
            } => sample_stops(stops, distance(*center, point) / radius.max(1e-6)),
        }
    }
}

fn sample_stops(stops: &[(f32, Rgba)], offset: f32) -> Rgba {
    let offset = offset.clamp(0.0, 1.0);
    let Some(first) = stops.first() else {
        return [0.0; 4];
    };
    if offset <= first.0 {
        return first.1;
    }
    for pair in stops.windows(2) {
        let ((start, start_color), (end, end_color)) = (pair[0], pair[1]);
        if offset <= end {
            return mix(
                start_color,
                end_color,
                (offset - start) / (end - start).max(1e-6),
            );
        }
    }
    stops[stops.len() - 1].1
}

/// Call `visit(y, x, coverages)` for each row a path covers, clipped to the canvas,
/// with the exact covered area of each pixel from `x` onward.
fn rasterize(
    path: &Path,
    scale: f32,
    width: usize,
    height: usize,
    mut visit: impl FnMut(usize, usize, &[f32]),
) {
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for contour in path.contours() {
        for &(x, y) in contour {
            min_x = min_x.min(x * scale);
            min_y = min_y.min(y * scale);
            max_x = max_x.max(x * scale);
            max_y = max_y.max(y * scale);
        }
    }
    if min_x > max_x {
        return;
    }
    let left = min_x.floor().max(0.0) as usize;
    let right = (max_x.ceil().max(0.0) as usize).min(width);
    let top = min_y.floor().max(0.0) as usize;
    let bottom = (max_y.ceil().max(0.0) as usize).min(height);
    if left >= right || top >= bottom {
        return;
    }
    let box_width = right - left;
    let box_height = bottom - top;
    let stride = box_width + 2;
    let mut areas = vec![0.0_f32; stride * box_height + 2];
    for contour in path.contours() {
        for edge_index in 0..contour.len() {
            let from = contour[edge_index];
            let to = contour[(edge_index + 1) % contour.len()];
            let from = (from.0 * scale - left as f32, from.1 * scale - top as f32);
            let to = (to.0 * scale - left as f32, to.1 * scale - top as f32);
            draw_clipped_edge(&mut areas, stride, box_width, box_height, from, to);
        }
    }
    let mut coverages = vec![0.0_f32; box_width];
    for row in 0..box_height {
        let mut running_area = 0.0;
        let row_areas = &areas[row * stride..row * stride + box_width];
        for (coverage, area) in coverages.iter_mut().zip(row_areas) {
            running_area += area;
            *coverage = running_area.abs().min(1.0);
        }
        visit(top + row, left, &coverages);
    }
}

/// Split an edge where it leaves the box sideways and pin the outside parts to the box
/// side, which keeps every row's area exact for the pixels inside.
fn draw_clipped_edge(
    areas: &mut [f32],
    stride: usize,
    box_width: usize,
    box_height: usize,
    from: Point,
    to: Point,
) {
    let right = box_width as f32;
    let mut cuts = [0.0_f32, 1.0, 1.0, 1.0];
    let mut cut_count = 2;
    for side in [0.0, right] {
        if (from.0 - side) * (to.0 - side) < 0.0 {
            cuts[cut_count] = (side - from.0) / (to.0 - from.0);
            cut_count += 1;
        }
    }
    let cuts = &mut cuts[..cut_count];
    cuts.sort_by(f32::total_cmp);
    let at = |t: f32| {
        (
            (from.0 + (to.0 - from.0) * t).clamp(0.0, right),
            from.1 + (to.1 - from.1) * t,
        )
    };
    for pair in cuts.windows(2) {
        draw_edge(areas, stride, box_height, at(pair[0]), at(pair[1]));
    }
}

/// Accumulate one edge's signed area into the rows it crosses (after font-rs).
fn draw_edge(areas: &mut [f32], stride: usize, box_height: usize, from: Point, to: Point) {
    if (from.1 - to.1).abs() <= f32::EPSILON {
        return;
    }
    let (direction, upper, lower) = if from.1 < to.1 {
        (1.0, from, to)
    } else {
        (-1.0, to, from)
    };
    let x_per_y = (lower.0 - upper.0) / (lower.1 - upper.1);
    let mut x = upper.0;
    if upper.1 < 0.0 {
        x -= upper.1 * x_per_y;
    }
    let first_row = upper.1.max(0.0) as usize;
    let last_row = (lower.1.ceil().max(0.0) as usize).min(box_height);
    for row in first_row..last_row {
        let row_start = row * stride;
        let row_height = ((row + 1) as f32).min(lower.1) - (row as f32).max(upper.1);
        let next_x = x + x_per_y * row_height;
        let signed_height = row_height * direction;
        let (x0, x1) = if x < next_x { (x, next_x) } else { (next_x, x) };
        let x0_floor = x0.floor();
        let x0_index = x0_floor as usize;
        let x1_ceil = x1.ceil();
        let x1_index = x1_ceil as usize;
        if x1_index <= x0_index + 1 {
            let mid_fraction = 0.5 * (x + next_x) - x0_floor;
            areas[row_start + x0_index] += signed_height - signed_height * mid_fraction;
            areas[row_start + x0_index + 1] += signed_height * mid_fraction;
        } else {
            let inverse_span = (x1 - x0).recip();
            let x0_fraction = x0 - x0_floor;
            let first_area = 0.5 * inverse_span * (1.0 - x0_fraction) * (1.0 - x0_fraction);
            let x1_fraction = x1 - x1_ceil + 1.0;
            let last_area = 0.5 * inverse_span * x1_fraction * x1_fraction;
            areas[row_start + x0_index] += signed_height * first_area;
            if x1_index == x0_index + 2 {
                areas[row_start + x0_index + 1] += signed_height * (1.0 - first_area - last_area);
            } else {
                let second_area = inverse_span * (1.5 - x0_fraction);
                areas[row_start + x0_index + 1] += signed_height * (second_area - first_area);
                for column in x0_index + 2..x1_index - 1 {
                    areas[row_start + column] += signed_height * inverse_span;
                }
                let before_last = second_area + (x1_index - x0_index - 3) as f32 * inverse_span;
                areas[row_start + x1_index - 1] += signed_height * (1.0 - before_last - last_area);
            }
            areas[row_start + x1_index] += signed_height * last_area;
        }
        x = next_x;
    }
}

/// The coverage of one shape over a box of the canvas, for clipping or soft shadows.
#[derive(Clone, Debug)]
pub(crate) struct Mask {
    left: isize,
    top: isize,
    width: usize,
    height: usize,
    values: Vec<f32>,
}

impl Mask {
    /// Cover `path`, with `margin` device pixels of empty border for blurring into.
    pub(crate) fn from_path(path: &Path, scale: f32, canvas: &Canvas, margin: usize) -> Self {
        let mut rows: Vec<(usize, usize, Vec<f32>)> = Vec::new();
        rasterize(
            path,
            scale,
            canvas.width,
            canvas.height,
            |y, x, coverages| {
                rows.push((y, x, coverages.to_vec()));
            },
        );
        let Some(&(first_y, first_x, ref first_row)) = rows.first() else {
            return Self {
                left: 0,
                top: 0,
                width: 0,
                height: 0,
                values: Vec::new(),
            };
        };
        let content_width = first_row.len();
        let width = content_width + 2 * margin;
        let height = rows.len() + 2 * margin;
        let mut values = vec![0.0; width * height];
        for (row_index, (_, _, coverages)) in rows.iter().enumerate() {
            let start = (row_index + margin) * width + margin;
            values[start..start + content_width].copy_from_slice(coverages);
        }
        Self {
            left: first_x as isize - margin as isize,
            top: first_y as isize - margin as isize,
            width,
            height,
            values,
        }
    }

    /// Coverage at a device pixel; zero outside the box.
    pub(crate) fn value(&self, x: isize, y: isize) -> f32 {
        let (column, row) = (x - self.left, y - self.top);
        if column < 0 || row < 0 || column >= self.width as isize || row >= self.height as isize {
            return 0.0;
        }
        self.values[row as usize * self.width + column as usize]
    }

    /// Soften with three box blurs of `radius` device pixels each way, close to a
    /// Gaussian blur.
    pub(crate) fn blur(&mut self, radius: usize) {
        if radius == 0 || self.values.is_empty() {
            return;
        }
        let mut scratch = vec![0.0; self.values.len()];
        for _ in 0..3 {
            blur_lines(
                &self.values,
                &mut scratch,
                self.width,
                self.height,
                1,
                self.width,
                radius,
            );
            blur_lines(
                &scratch,
                &mut self.values,
                self.height,
                self.width,
                self.width,
                1,
                radius,
            );
        }
    }
}

/// Box-blur `line_count` lines of `line_length` values, stepping `step` within a line
/// and `line_step` between lines.
fn blur_lines(
    source: &[f32],
    target: &mut [f32],
    line_length: usize,
    line_count: usize,
    step: usize,
    line_step: usize,
    radius: usize,
) {
    let window = (2 * radius + 1) as f32;
    for line in 0..line_count {
        let base = line * line_step;
        let read = |index: isize| -> f32 {
            if index < 0 || index >= line_length as isize {
                0.0
            } else {
                source[base + index as usize * step]
            }
        };
        let mut sum: f32 = (-(radius as isize)..=radius as isize).map(read).sum();
        for index in 0..line_length {
            target[base + index * step] = sum / window;
            sum +=
                read(index as isize + radius as isize + 1) - read(index as isize - radius as isize);
        }
    }
}

/// Premultiplied pixels with a logical-to-device scale.
#[derive(Clone, Debug)]
pub(crate) struct Canvas {
    pub(crate) width: usize,
    pub(crate) height: usize,
    pub(crate) scale: f32,
    pixels: Vec<[f32; 4]>,
}

impl Canvas {
    /// A transparent canvas.
    pub(crate) fn new(width: usize, height: usize, scale: f32) -> Self {
        Self {
            width,
            height,
            scale,
            pixels: vec![[0.0; 4]; width * height],
        }
    }

    pub(crate) fn fill(&mut self, path: &Path, paint: &Paint, blend: Blend) {
        self.fill_clipped(path, paint, blend, None);
    }

    /// Fill `path`, weighting each pixel by `clip` when given.
    pub(crate) fn fill_clipped(
        &mut self,
        path: &Path,
        paint: &Paint,
        blend: Blend,
        clip: Option<&Mask>,
    ) {
        let (width, height, scale) = (self.width, self.height, self.scale);
        let pixels = &mut self.pixels;
        rasterize(path, scale, width, height, |y, left, coverages| {
            for (offset, &coverage) in coverages.iter().enumerate() {
                let x = left + offset;
                let coverage = match clip {
                    Some(clip_mask) => coverage * clip_mask.value(x as isize, y as isize),
                    None => coverage,
                };
                if coverage <= 0.0 {
                    continue;
                }
                let color = paint.color_at(((x as f32 + 0.5) / scale, (y as f32 + 0.5) / scale));
                composite(&mut pixels[y * width + x], color, coverage, blend);
            }
        });
    }

    /// Stroke a polyline `width` logical units wide, with round joins and caps.
    pub(crate) fn stroke(
        &mut self,
        points: &[Point],
        width: f32,
        closed: bool,
        paint: &Paint,
        blend: Blend,
    ) {
        self.fill(&stroke_path(points, width, closed), paint, blend);
    }

    pub(crate) fn stroke_clipped(
        &mut self,
        points: &[Point],
        width: f32,
        closed: bool,
        paint: &Paint,
        blend: Blend,
        clip: &Mask,
    ) {
        self.fill_clipped(
            &stroke_path(points, width, closed),
            paint,
            blend,
            Some(clip),
        );
    }

    /// Paint `color` through a mask shifted by device pixels, as a soft shadow.
    pub(crate) fn shade(
        &mut self,
        mask: &Mask,
        offset_x: isize,
        offset_y: isize,
        color: Rgba,
        max_y: usize,
    ) {
        let (left, top) = (mask.left + offset_x, mask.top + offset_y);
        for row in 0..mask.height {
            let y = top + row as isize;
            if y < 0 || y >= max_y.min(self.height) as isize {
                continue;
            }
            for column in 0..mask.width {
                let x = left + column as isize;
                if x < 0 || x >= self.width as isize {
                    continue;
                }
                let coverage = mask.values[row * mask.width + column];
                if coverage > 0.002 {
                    composite(
                        &mut self.pixels[y as usize * self.width + x as usize],
                        color,
                        coverage,
                        Blend::Over,
                    );
                }
            }
        }
    }

    /// Multiply the rows from `top` to `bottom` (device pixels) by `color`.
    pub(crate) fn multiply_rows(&mut self, top: usize, bottom: usize, color: Rgba) {
        for pixel in &mut self.pixels[top * self.width..bottom.min(self.height) * self.width] {
            composite(pixel, color, 1.0, Blend::Multiply);
        }
    }

    /// Composite another canvas of the same size on top, or add its light.
    pub(crate) fn draw_layer(&mut self, layer: &Canvas, blend: Blend) {
        for (pixel, layer_pixel) in self.pixels.iter_mut().zip(&layer.pixels) {
            if layer_pixel[3] <= 0.0 && blend == Blend::Over {
                continue;
            }
            match blend {
                Blend::Over => {
                    let keep = 1.0 - layer_pixel[3];
                    for (value, layer_value) in pixel.iter_mut().zip(layer_pixel) {
                        *value = layer_value + *value * keep;
                    }
                }
                Blend::Plus => {
                    for (value, layer_value) in pixel.iter_mut().zip(layer_pixel) {
                        *value = (*value + layer_value).min(1.0);
                    }
                }
                Blend::Multiply => unreachable!("layers are only drawn over or added"),
            }
        }
    }

    /// Copy the rows from `top` to `bottom` (device pixels) from another canvas.
    pub(crate) fn copy_rows(&mut self, source: &Canvas, top: usize, bottom: usize) {
        let range = top * self.width..bottom.min(self.height) * self.width;
        self.pixels[range.clone()].copy_from_slice(&source.pixels[range]);
    }

    /// Weight every pixel's alpha by a mask, such as the pond's rounded outline.
    pub(crate) fn clip_to(&mut self, mask: &Mask) {
        for y in 0..self.height {
            for x in 0..self.width {
                let coverage = mask.value(x as isize, y as isize);
                if coverage < 1.0 {
                    for channel in &mut self.pixels[y * self.width + x] {
                        *channel *= coverage;
                    }
                }
            }
        }
    }

    /// Straight-alpha RGBA bytes, row by row.
    pub(crate) fn to_rgba_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(self.pixels.len() * 4);
        for pixel in &self.pixels {
            let alpha = pixel[3].clamp(0.0, 1.0);
            let unpremultiply = if alpha > 0.0 { 1.0 / alpha } else { 0.0 };
            for value in &pixel[..3] {
                bytes.push(((value * unpremultiply).clamp(0.0, 1.0) * 255.0).round() as u8);
            }
            bytes.push((alpha * 255.0).round() as u8);
        }
        bytes
    }

    /// Draw a run of text with its baseline at `baseline` and its left edge at `left`,
    /// both in device pixels. Returns the device x after the last glyph.
    pub(crate) fn draw_text(
        &mut self,
        text: &str,
        weight: TextWeight,
        left: f32,
        baseline: f32,
        color: Rgba,
    ) -> f32 {
        let font = caption_font();
        let mut pen_x = left;
        for character in text.chars() {
            let glyph = font.glyph(weight, character);
            let origin_x = pen_x.round() as isize + isize::from(glyph.left);
            let origin_y = baseline.round() as isize - isize::from(glyph.top);
            for row in 0..usize::from(glyph.height) {
                for column in 0..usize::from(glyph.width) {
                    let coverage =
                        f32::from(glyph.alpha[row * usize::from(glyph.width) + column]) / 255.0;
                    let (x, y) = (origin_x + column as isize, origin_y + row as isize);
                    if coverage > 0.0
                        && x >= 0
                        && y >= 0
                        && (x as usize) < self.width
                        && (y as usize) < self.height
                    {
                        composite(
                            &mut self.pixels[y as usize * self.width + x as usize],
                            color,
                            coverage,
                            Blend::Over,
                        );
                    }
                }
            }
            pen_x += f32::from(glyph.advance) / 64.0;
        }
        pen_x
    }
}

/// Combine one premultiplied pixel with a straight-alpha color at a coverage.
fn composite(pixel: &mut [f32; 4], color: Rgba, coverage: f32, blend: Blend) {
    let alpha = color[3] * coverage;
    match blend {
        Blend::Over => {
            let keep = 1.0 - alpha;
            for (value, source) in pixel[..3].iter_mut().zip(color) {
                *value = source * alpha + *value * keep;
            }
            pixel[3] = alpha + pixel[3] * keep;
        }
        Blend::Multiply => {
            let destination_alpha = pixel[3];
            for (value, source) in pixel[..3].iter_mut().zip(color) {
                let source = source * alpha;
                *value =
                    source * *value + source * (1.0 - destination_alpha) + *value * (1.0 - alpha);
            }
            pixel[3] = alpha + destination_alpha * (1.0 - alpha);
        }
        Blend::Plus => {
            for (value, source) in pixel[..3].iter_mut().zip(color) {
                *value = (*value + source * alpha).min(1.0);
            }
            pixel[3] = (pixel[3] + alpha).min(1.0);
        }
    }
}

/// The two caption weights.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TextWeight {
    Regular,
    Medium,
}

/// The caption's glyphs at their one size, from `assets/caption-font.bin`.
pub(crate) struct CaptionFont {
    weights: [BTreeMap<char, Glyph>; 2],
}

struct Glyph {
    left: i16,
    top: i16,
    width: u16,
    height: u16,
    advance: u16,
    alpha: Vec<u8>,
}

impl CaptionFont {
    fn glyph(&self, weight: TextWeight, character: char) -> &Glyph {
        let glyphs = &self.weights[weight as usize];
        glyphs
            .get(&character)
            .or_else(|| glyphs.get(&'?'))
            .expect("the caption font has a question mark")
    }

    /// The width of a run of text, in device pixels.
    pub(crate) fn measure(&self, text: &str, weight: TextWeight) -> f32 {
        text.chars()
            .map(|character| f32::from(self.glyph(weight, character).advance) / 64.0)
            .sum()
    }
}

/// The caption font, read once from the bytes built into the program.
pub(crate) fn caption_font() -> &'static CaptionFont {
    static FONT: OnceLock<CaptionFont> = OnceLock::new();
    FONT.get_or_init(|| {
        parse_caption_font(include_bytes!("../assets/caption-font.bin"))
            .expect("the built-in caption font is well formed")
    })
}

fn parse_caption_font(bytes: &[u8]) -> Option<CaptionFont> {
    let mut cursor = 0;
    let mut take = |count: usize| {
        let taken = bytes.get(cursor..cursor + count);
        cursor += count;
        taken
    };
    if take(4)? != b"KOIF" {
        return None;
    }
    let header = take(10)?;
    let field = |index: usize| u16::from_le_bytes([header[index * 2], header[index * 2 + 1]]);
    let (version, weight_count) = (field(0), field(4));
    if version != 1 || weight_count != 2 {
        return None;
    }
    let mut weights: [BTreeMap<char, Glyph>; 2] = [BTreeMap::new(), BTreeMap::new()];
    for glyphs in &mut weights {
        let count_bytes = take(2)?;
        let glyph_count = u16::from_le_bytes([count_bytes[0], count_bytes[1]]);
        for _ in 0..glyph_count {
            let record = take(14)?;
            let code_point = u32::from_le_bytes([record[0], record[1], record[2], record[3]]);
            let left = i16::from_le_bytes([record[4], record[5]]);
            let top = i16::from_le_bytes([record[6], record[7]]);
            let width = u16::from_le_bytes([record[8], record[9]]);
            let height = u16::from_le_bytes([record[10], record[11]]);
            let advance = u16::from_le_bytes([record[12], record[13]]);
            let alpha = take(usize::from(width) * usize::from(height))?.to_vec();
            glyphs.insert(
                char::from_u32(code_point)?,
                Glyph {
                    left,
                    top,
                    width,
                    height,
                    advance,
                    alpha,
                },
            );
        }
    }
    Some(CaptionFont { weights })
}
