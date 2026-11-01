//! One shared 255-color palette for a smooth animation, and ordered dithering into it.
//!
//! A GIF frame holds at most 256 colors. The pond's gradients need more, so this builds
//! one palette for the whole loop from a sample of its frames: colors are binned, the
//! bins are split by variance until there are 255, and a few rounds of k-means settle
//! each color on its bin cluster. Pixels are then mapped through an 8x8 Bayer threshold
//! so gradients dither instead of banding. The threshold depends only on a pixel's
//! position and color, so a pixel that does not change between frames keeps its index,
//! which lets the encoder store each frame as only the pixels that changed.

/// The GIF index reserved for transparent pixels; the palette fills the other 255.
pub(crate) const TRANSPARENT_INDEX: u8 = 255;
const PALETTE_SIZE: usize = 255;
/// How far the Bayer threshold moves a channel, out of 255.
const DITHER_STRENGTH: f32 = 9.0;
const BAYER: [u8; 64] = [
    0, 32, 8, 40, 2, 34, 10, 42, 48, 16, 56, 24, 50, 18, 58, 26, 12, 44, 4, 36, 14, 46, 6, 38, 60,
    28, 52, 20, 62, 30, 54, 22, 3, 35, 11, 43, 1, 33, 9, 41, 51, 19, 59, 27, 49, 17, 57, 25, 15,
    47, 7, 39, 13, 45, 5, 37, 63, 31, 55, 23, 61, 29, 53, 21,
];

/// A palette and a cache of nearest colors over a 64-level-per-channel grid.
pub(crate) struct Palette {
    colors: Vec<[f32; 3]>,
    nearest: Vec<u8>,
}

impl Palette {
    /// Build a palette from the opaque pixels of every `frame_step`th RGBA frame.
    pub(crate) fn from_frames<'a>(frames: impl Iterator<Item = &'a [u8]>) -> Self {
        let mut bins = vec![(0_u64, [0_u64; 3]); 1 << 15];
        for (frame_number, rgba) in frames.enumerate() {
            let offset = frame_number % 3;
            for pixel in rgba.as_chunks::<4>().0.iter().skip(offset).step_by(3) {
                if pixel[3] < 128 {
                    continue;
                }
                let key = (usize::from(pixel[0] >> 3) << 10)
                    | (usize::from(pixel[1] >> 3) << 5)
                    | usize::from(pixel[2] >> 3);
                let bin = &mut bins[key];
                bin.0 += 1;
                for (sum, &value) in bin.1.iter_mut().zip(&pixel[..3]) {
                    *sum += u64::from(value);
                }
            }
        }
        let samples: Vec<([f32; 3], f32)> = bins
            .iter()
            .filter(|(count, _)| *count > 0)
            .map(|(count, sums)| {
                (
                    std::array::from_fn(|channel| sums[channel] as f32 / *count as f32),
                    *count as f32,
                )
            })
            .collect();
        let mut colors = split_by_variance(&samples);
        refine_with_k_means(&samples, &mut colors);
        if colors.is_empty() {
            colors.push([0.0; 3]);
        }
        Self {
            colors,
            nearest: vec![TRANSPARENT_INDEX; 1 << 18],
        }
    }

    /// The palette as GIF color-table bytes, padded to 256 entries.
    pub(crate) fn color_table(&self) -> Vec<u8> {
        let mut table: Vec<u8> = self
            .colors
            .iter()
            .flat_map(|color| color.map(|channel| channel.round().clamp(0.0, 255.0) as u8))
            .collect();
        table.resize(256 * 3, 0);
        table
    }

    /// The dithered palette index for an opaque pixel at a position.
    pub(crate) fn index_at(&mut self, pixel: [u8; 3], x: usize, y: usize) -> u8 {
        let threshold = (f32::from(BAYER[(y % 8) * 8 + x % 8]) + 0.5) / 64.0 - 0.5;
        let level = |channel: u8| -> usize {
            ((f32::from(channel) + threshold * DITHER_STRENGTH)
                .round()
                .clamp(0.0, 255.0) as usize)
                >> 2
        };
        let key = (level(pixel[0]) << 12) | (level(pixel[1]) << 6) | level(pixel[2]);
        if self.nearest[key] == TRANSPARENT_INDEX {
            let grid_color =
                [(key >> 12) & 63, (key >> 6) & 63, key & 63].map(|level| (level * 4 + 2) as f32);
            self.nearest[key] = nearest_color(&self.colors, grid_color) as u8;
        }
        self.nearest[key]
    }
}

/// Squared distance, weighting green over red over blue the way eyes do.
fn distance(left: [f32; 3], right: [f32; 3]) -> f32 {
    let (red, green, blue) = (left[0] - right[0], left[1] - right[1], left[2] - right[2]);
    2.0 * red * red + 4.0 * green * green + 3.0 * blue * blue
}

fn nearest_color(colors: &[[f32; 3]], color: [f32; 3]) -> usize {
    let mut best = (0, f32::MAX);
    for (index, candidate) in colors.iter().enumerate() {
        let candidate_distance = distance(*candidate, color);
        if candidate_distance < best.1 {
            best = (index, candidate_distance);
        }
    }
    best.0
}

/// Split the samples into up to 255 boxes, always splitting the box with the largest
/// weighted variance along its widest channel at its weighted median.
fn split_by_variance(samples: &[([f32; 3], f32)]) -> Vec<[f32; 3]> {
    let mut boxes: Vec<Vec<usize>> = vec![(0..samples.len()).collect()];
    while boxes.len() < PALETTE_SIZE {
        let mut widest: Option<(usize, usize, f32)> = None;
        for (box_index, members) in boxes.iter().enumerate() {
            if members.len() < 2 {
                continue;
            }
            let mean = weighted_mean(samples, members);
            let mut spread = [0.0_f32; 3];
            for &member in members {
                let (color, count) = samples[member];
                for ((channel_spread, value), center) in spread.iter_mut().zip(color).zip(mean) {
                    *channel_spread += count * (value - center).powi(2);
                }
            }
            let channel = (0..3)
                .max_by(|&left, &right| spread[left].total_cmp(&spread[right]))
                .unwrap_or(0);
            let total: f32 = spread.iter().sum();
            if widest.is_none_or(|(_, _, best)| total > best) {
                widest = Some((box_index, channel, total));
            }
        }
        let Some((box_index, channel, total)) = widest else {
            break;
        };
        if total <= 0.0 {
            break;
        }
        let mut members = boxes.swap_remove(box_index);
        members.sort_by(|&left, &right| {
            samples[left].0[channel].total_cmp(&samples[right].0[channel])
        });
        let half_weight: f32 = members.iter().map(|&member| samples[member].1).sum::<f32>() / 2.0;
        let mut running = 0.0;
        let mut split = 1;
        for (position, &member) in members.iter().enumerate() {
            running += samples[member].1;
            if running >= half_weight {
                split = (position + 1).clamp(1, members.len() - 1);
                break;
            }
        }
        let upper = members.split_off(split);
        boxes.push(members);
        boxes.push(upper);
    }
    boxes
        .iter()
        .map(|members| weighted_mean(samples, members))
        .collect()
}

fn weighted_mean(samples: &[([f32; 3], f32)], members: &[usize]) -> [f32; 3] {
    let mut sum = [0.0_f32; 3];
    let mut weight = 0.0;
    for &member in members {
        let (color, count) = samples[member];
        for (channel_sum, value) in sum.iter_mut().zip(color) {
            *channel_sum += value * count;
        }
        weight += count;
    }
    sum.map(|channel| channel / weight.max(1e-6))
}

/// Move each palette color to the weighted mean of the samples nearest it.
fn refine_with_k_means(samples: &[([f32; 3], f32)], colors: &mut [[f32; 3]]) {
    for _ in 0..4 {
        let mut sums = vec![([0.0_f32; 3], 0.0_f32); colors.len()];
        for &(color, count) in samples {
            let (sum, weight) = &mut sums[nearest_color(colors, color)];
            for (channel_sum, value) in sum.iter_mut().zip(color) {
                *channel_sum += value * count;
            }
            *weight += count;
        }
        for (color, (sum, weight)) in colors.iter_mut().zip(sums) {
            if weight > 0.0 {
                *color = sum.map(|channel| channel / weight);
            }
        }
    }
}
