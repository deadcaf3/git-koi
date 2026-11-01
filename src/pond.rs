//! The pond: a pair of koi circling under lily pads, drawn smooth for a profile README.
//!
//! A white koi and a black koi swim one oval as a yin-yang pair. Their patches come from
//! the account's languages through the same pigment rules as [`crate::coat`]: the white
//! koi carries the top pigment as Kohaku-style plates and the second as spots, and the
//! black koi carries white plates with the first colored pigment as accents. Patches
//! come in pale for young accounts, dull or vivid with the longest streak, the current
//! streak gilds the fin edges, and gin-rin glints sparkle on long records. Growth sets
//! the koi's length. The day's [`Behavior`] sets the pace: cruising takes one lap per
//! loop, darting two, and resting, deep rest, and surface feeding slow the whole pond.
//!
//! Each merged pull request to a repository someone else owns blooms as a lotus, up to
//! [`MAX_LOTUS_COUNT`]. The season, from the observation date in the northern
//! hemisphere, brings cherry petals, a dragonfly by day, maple leaves, or ice and snow.
//! [`PondTheme::Dark`] draws the same pond by moonlight, with fireflies outside winter.
//! A vermilion seal signs the lower right corner, dated with the day the pond shows. A
//! caption for the command's summary and the preview page names the streak, the main
//! language, the merged upstream pull requests, and the pace.
//!
//! The pond is [`POND_WIDTH_PIXELS`] by [`POND_HEIGHT_PIXELS`], twice its 400 by 240
//! layout, so it stays sharp shown 400 wide on high-density screens. Every frame of the
//! [`POND_FRAME_COUNT`] loop is a phase of the same cycle, so the loop is seamless.
//! Rendering is deterministic and performs no I/O.

use std::f32::consts::{PI, TAU};

use chrono::{Datelike, NaiveDate};

use crate::behavior::Behavior;
use crate::coat::{Pigment, calculate_pigment_development, calculate_pigment_shares};
use crate::koi::{
    FULL_COLOR_CONTRIBUTIONS, KoiDevelopment, RecentStreak, calculate_growth_fraction,
};
use crate::markings::calculate_streak_marks;
use crate::raster::{
    Blend, Canvas, Mask, Paint, Path, Point, Rgba, TextWeight, caption_font, hex, inscription_font,
    mix, smooth_open, stroke_path, with_alpha,
};

/// Device pixels per layout unit.
const SCALE: f32 = 2.5;
const WIDTH: f32 = 400.0;
const HEIGHT: f32 = 208.0;
/// The water ends here; the caption strip fills the rest.
const POND_BOTTOM: f32 = 208.0;
const CORNER_RADIUS: f32 = 14.0;
/// The caption keeps this far from the strip's sides.
const CAPTION_MARGIN: f32 = 8.0;
/// The pond's width in pixels.
pub const POND_WIDTH_PIXELS: u32 = (WIDTH * SCALE) as u32;
/// The pond's height in pixels.
pub const POND_HEIGHT_PIXELS: u32 = (HEIGHT * SCALE) as u32;
/// Frames in every loop; the behavior sets how long each one shows.
pub const POND_FRAME_COUNT: u32 = 150;
/// More merged pull requests than this still count in the caption but bloom no more.
pub const MAX_LOTUS_COUNT: u64 = 9;

const SWIM_CENTER: Point = (200.0, 102.0);
const SWIM_RADIUS_X: f32 = 80.0;
const SWIM_RADIUS_Y: f32 = 46.0;
const SPINE_POINTS: usize = 18;
const BODY_LENGTH: f32 = 74.0;
/// How long a ripple spreads, as a share of the loop.
const RIPPLE_LIFE: f32 = 0.18;

/// Which GitHub color scheme the pond is drawn for.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PondTheme {
    /// Daylight, for GitHub's light theme.
    Light,
    /// Moonlight, for GitHub's dark themes.
    Dark,
}

/// The pond's season.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Season {
    /// March through May: drifting cherry petals.
    Spring,
    /// June through August: extra pads, a dragonfly by day, the most fireflies.
    Summer,
    /// September through November: floating maple leaves and yellowing pads.
    Autumn,
    /// December through February: ice at the edges, snow, and frosted lotus.
    Winter,
}

impl Season {
    /// The season a date falls in, in the northern hemisphere.
    pub fn of_date(date: NaiveDate) -> Self {
        match date.month() {
            3..=5 => Self::Spring,
            6..=8 => Self::Summer,
            9..=11 => Self::Autumn,
            _ => Self::Winter,
        }
    }

    /// The season's name in lowercase.
    pub fn name(self) -> &'static str {
        match self {
            Self::Spring => "spring",
            Self::Summer => "summer",
            Self::Autumn => "autumn",
            Self::Winter => "winter",
        }
    }
}

/// How many frames of a behavior's loop pass each second is fixed by the GIF delay; the
/// pond always has [`POND_FRAME_COUNT`] frames, so slower behaviors last longer.
pub fn pond_frame_delay_centiseconds(behavior: Behavior) -> u16 {
    match behavior {
        Behavior::Cruise | Behavior::Dart => 8,
        Behavior::SurfaceFeeding => 12,
        Behavior::Rest => 16,
        Behavior::DeepRest => 24,
    }
}

/// One rendered frame: straight-alpha RGBA bytes, row by row, transparent outside the
/// pond's rounded corners.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PondFrame {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Four bytes per pixel.
    pub rgba: Vec<u8>,
}

/// Everything about one account's pond that stays the same from frame to frame,
/// including its prerendered water, pads, and caption.
pub struct PondScene {
    theme: PondTheme,
    season: Season,
    motion: Motion,
    koi: [KoiLook; 2],
    koi_scale: f32,
    glimmer: f32,
    caption: String,
    background: Canvas,
    surface: Canvas,
    night_glow: Canvas,
    /// The day the pond shows, as inscribed beside the seal.
    dated: String,
    outline: Mask,
}

impl PondScene {
    /// Lay out an account's pond. `upstream_merged_pull_requests` is `None` for older
    /// snapshots, which then show no lotus and leave the count out of the caption.
    pub fn new(
        koi_development: &KoiDevelopment,
        behavior: Behavior,
        upstream_merged_pull_requests: Option<u64>,
        season: Season,
        theme: PondTheme,
    ) -> Self {
        let streak_marks = calculate_streak_marks(koi_development);
        let growth = calculate_growth_fraction(koi_development.total_contributions) as f32;
        let koi = build_koi_looks(
            koi_development,
            streak_marks.streak_fin_strength as f32,
            streak_marks.color_vibrancy as f32,
        );
        let lotus_count = upstream_merged_pull_requests
            .unwrap_or(0)
            .min(MAX_LOTUS_COUNT) as usize;
        let caption_items =
            build_caption_items(koi_development, behavior, upstream_merged_pull_requests);
        let (width, height) = (POND_WIDTH_PIXELS as usize, POND_HEIGHT_PIXELS as usize);
        let mut outline_path = Path::new();
        outline_path.round_rect(0.0, 0.0, WIDTH, HEIGHT, CORNER_RADIUS);
        let blank = Canvas::new(width, height, SCALE);
        let outline = Mask::from_path(&outline_path, SCALE, &blank, 0);
        let pads = lay_out_pads(season, lotus_count);
        Self {
            theme,
            season,
            motion: Motion::of(behavior),
            koi,
            koi_scale: 0.8 + 0.3 * growth,
            glimmer: streak_marks.glimmer_strength as f32,
            caption: caption_items
                .iter()
                .map(CaptionItem::plain_text)
                .collect::<Vec<_>>()
                .join(" · "),
            background: draw_background(season, theme),
            surface: draw_surface(&pads, season),
            night_glow: draw_night_glow(&pads),
            dated: koi_development.observed_on.format("%-d %b %Y").to_string(),
            outline,
        }
    }

    /// The caption as plain text, items separated by middle dots.
    pub fn caption(&self) -> &str {
        &self.caption
    }

    /// The pond's season.
    pub fn season(&self) -> Season {
        self.season
    }
}

/// Draw one frame of the loop; frame indices wrap around the loop.
pub fn render_pond_frame(pond_scene: &PondScene, frame_index: u32) -> PondFrame {
    let phase = (frame_index % POND_FRAME_COUNT) as f32 / POND_FRAME_COUNT as f32;
    let night = pond_scene.theme == PondTheme::Dark;
    let mut canvas = pond_scene.background.clone();
    let spines: Vec<Vec<SpinePoint>> = pond_scene
        .koi
        .iter()
        .map(|koi_look| pond_scene.spine(koi_look, phase))
        .collect();
    for spine in &spines {
        pond_scene.draw_koi_shadow(&mut canvas, spine, night);
    }
    for (koi_look, spine) in pond_scene.koi.iter().zip(&spines) {
        pond_scene.draw_koi(&mut canvas, koi_look, spine, phase);
    }
    pond_scene.draw_ripples(&mut canvas, phase);
    match pond_scene.season {
        Season::Spring => draw_cherry_petals(&mut canvas, phase),
        Season::Autumn => draw_maple_leaves(&mut canvas, phase),
        Season::Summer | Season::Winter => {}
    }
    canvas.draw_layer(&pond_scene.surface, Blend::Over);
    if night {
        canvas.multiply_rows(
            0,
            (POND_BOTTOM * SCALE) as usize,
            night_tint(pond_scene.season),
        );
        for (koi_look, spine) in pond_scene.koi.iter().zip(&spines) {
            pond_scene.draw_moonlight_on_koi(&mut canvas, koi_look, spine);
        }
        canvas.draw_layer(&pond_scene.night_glow, Blend::Plus);
        draw_reflected_stars(&mut canvas, phase);
        draw_moon_shimmer(&mut canvas, phase);
        draw_fireflies(&mut canvas, pond_scene.season, phase);
    } else {
        if pond_scene.season == Season::Summer {
            draw_dragonfly(&mut canvas, phase);
        }
        draw_glints(&mut canvas, phase);
    }
    if pond_scene.season == Season::Winter {
        draw_snow(&mut canvas, phase, night);
    }
    draw_seal(&mut canvas, &pond_scene.dated, night);
    canvas.clip_to(&pond_scene.outline);
    PondFrame {
        width: POND_WIDTH_PIXELS,
        height: POND_HEIGHT_PIXELS,
        rgba: canvas.to_rgba_bytes(),
    }
}

/// A deterministic pseudo-random number from zero up to one.
fn random(seed: u32) -> f32 {
    let mut value = seed.wrapping_mul(0x9e37_79b1) ^ 0x85eb_ca6b;
    value ^= value >> 16;
    value = value.wrapping_mul(0x7feb_352d);
    value ^= value >> 15;
    value = value.wrapping_mul(0x846c_a68b);
    value ^= value >> 16;
    (value >> 8) as f32 / 16_777_216.0
}

/// How the day's behavior moves the pair; every count is whole per loop, so it loops.
struct Motion {
    laps: f32,
    tail_beats: f32,
    sway: f32,
    depth_tint: f32,
    shadow_offset: Point,
    shadow_alpha: f32,
    tail_ripples: u32,
    gulps: u32,
}

impl Motion {
    fn of(behavior: Behavior) -> Self {
        let (laps, tail_beats, sway, depth_tint, shadow_offset, shadow_alpha, tail_ripples, gulps) =
            match behavior {
                Behavior::Cruise => (1.0, 10.0, 4.2, 0.06, (7.0, 12.0), 0.38, 6, 0),
                Behavior::Dart => (2.0, 22.0, 4.8, 0.06, (7.0, 12.0), 0.38, 10, 0),
                Behavior::SurfaceFeeding => (1.0, 8.0, 3.4, 0.0, (9.0, 15.0), 0.3, 3, 6),
                Behavior::Rest => (1.0, 6.0, 3.0, 0.16, (5.0, 9.0), 0.42, 3, 0),
                Behavior::DeepRest => (1.0, 4.0, 2.2, 0.3, (3.0, 6.0), 0.46, 2, 0),
            };
        Self {
            laps,
            tail_beats,
            sway,
            depth_tint,
            shadow_offset,
            shadow_alpha,
            tail_ripples,
            gulps,
        }
    }
}

/// Colors and plates for one of the pair.
struct KoiLook {
    phase_offset: f32,
    body: Rgba,
    edge: Rgba,
    sheen: Rgba,
    wide_sheen: Rgba,
    fin: Rgba,
    fin_edge: Rgba,
    ray: Rgba,
    dorsal: Rgba,
    eye: Rgba,
    plates: Vec<Plate>,
}

/// A patch along the body: `along` from nose to tail, `side` across in half widths.
struct Plate {
    along: f32,
    side: f32,
    size: f32,
    color: Rgba,
}

#[derive(Clone, Copy)]
struct SpinePoint {
    position: Point,
    tangent: Point,
    normal: Point,
}

fn build_koi_looks(
    koi_development: &KoiDevelopment,
    streak_fin_strength: f32,
    color_vibrancy: f32,
) -> [KoiLook; 2] {
    let pigment_shares = calculate_pigment_shares(koi_development);
    let color_fraction = (calculate_growth_fraction(koi_development.total_contributions)
        / calculate_growth_fraction(FULL_COLOR_CONTRIBUTIONS))
    .min(1.0);
    let streak_gold = hex(0xf2c14e);
    let white_body = hex(0xf9f5ee);
    let black_body = hex(0x1c1d23);
    let pigment_color = |pigment: Pigment, body: Rgba, on_black: bool| {
        let full_color = adjust_vibrancy(
            hex(match (pigment, on_black) {
                (Pigment::Beni, _) => 0xe0462a,
                (Pigment::Orenji, _) => 0xee8a2a,
                (Pigment::Ki, true) => 0xf6d04d,
                (Pigment::Ki, false) => 0xf0c234,
                (Pigment::Ai, true) => 0x6fb3e8,
                (Pigment::Ai, false) => 0x3c6fb5,
                (Pigment::Sumi, true) => 0xf4f0e6,
                (Pigment::Sumi, false) => 0x1d1e23,
            }),
            color_vibrancy,
        );
        let development = calculate_pigment_development(pigment, color_fraction) as f32;
        mix(body, full_color, 0.3 + 0.7 * development)
    };
    let mut white_plates = Vec::new();
    if let Some(&(top_pigment, _)) = pigment_shares.first() {
        let color = pigment_color(top_pigment, white_body, false);
        for (along, side, size) in [
            (0.05, 0.0, 0.8),
            (0.27, 0.45, 0.95),
            (0.35, 0.55, 0.85),
            (0.5, -0.5, 0.9),
            (0.6, -0.35, 0.8),
            (0.76, 0.35, 0.75),
        ] {
            white_plates.push(Plate {
                along,
                side,
                size,
                color,
            });
        }
    }
    if let Some(&(second_pigment, _)) = pigment_shares.get(1) {
        let color = pigment_color(second_pigment, white_body, false);
        for (along, side, size) in [(0.43, 0.1, 0.38), (0.68, -0.15, 0.32)] {
            white_plates.push(Plate {
                along,
                side,
                size,
                color,
            });
        }
    }
    let mut black_plates: Vec<Plate> = [(0.04, 0.0, 0.75), (0.42, -0.5, 0.85), (0.5, -0.4, 0.7)]
        .into_iter()
        .map(|(along, side, size)| Plate {
            along,
            side,
            size,
            color: hex(0xf4f0e6),
        })
        .collect();
    if let Some(&(accent_pigment, _)) = pigment_shares
        .iter()
        .take(2)
        .find(|(pigment, _)| *pigment != Pigment::Sumi)
    {
        let color = pigment_color(accent_pigment, black_body, true);
        for (along, side, size) in [(0.22, 0.55, 0.65), (0.68, 0.4, 0.6)] {
            black_plates.push(Plate {
                along,
                side,
                size,
                color,
            });
        }
    }
    [
        KoiLook {
            phase_offset: 0.0,
            body: white_body,
            edge: [0.59, 0.47, 0.37, 0.35],
            sheen: [1.0, 1.0, 1.0, 0.7],
            wide_sheen: [1.0, 1.0, 1.0, 0.22],
            fin: [1.0, 0.98, 0.95, 0.5],
            fin_edge: mix(
                [1.0, 1.0, 1.0, 0.75],
                with_alpha(streak_gold, 0.95),
                streak_fin_strength,
            ),
            ray: [0.75, 0.63, 0.55, 0.35],
            dorsal: [0.75, 0.59, 0.51, 0.45],
            eye: hex(0x2b1d18),
            plates: white_plates,
        },
        KoiLook {
            phase_offset: PI,
            body: black_body,
            edge: [0.51, 0.55, 0.67, 0.35],
            sheen: [1.0, 1.0, 1.0, 0.32],
            wide_sheen: [0.67, 0.75, 1.0, 0.12],
            fin: [0.11, 0.12, 0.15, 0.55],
            fin_edge: mix(
                [1.0, 1.0, 1.0, 0.5],
                with_alpha(streak_gold, 0.95),
                streak_fin_strength,
            ),
            ray: [1.0, 1.0, 1.0, 0.16],
            dorsal: [1.0, 1.0, 1.0, 0.18],
            eye: hex(0x101114),
            plates: black_plates,
        },
    ]
}

/// Dull colors for a short longest streak, vivid ones for a long one; one is normal.
fn adjust_vibrancy(color: Rgba, color_vibrancy: f32) -> Rgba {
    let gray = 0.3 * color[0] + 0.59 * color[1] + 0.11 * color[2];
    let saturation = if color_vibrancy < 1.0 {
        1.0 - (1.0 - color_vibrancy) * 0.45
    } else {
        1.0 + (color_vibrancy - 1.0) * 0.25
    };
    let darkening = if color_vibrancy < 1.0 {
        1.0 - (1.0 - color_vibrancy) * 0.12
    } else {
        1.0
    };
    let mut adjusted = color;
    for channel in &mut adjusted[..3] {
        *channel = ((gray + (*channel - gray) * saturation) * darkening).clamp(0.0, 1.0);
    }
    adjusted
}

/// Half the body's width at a share of its length, before growth scaling.
fn body_half_width(along: f32) -> f32 {
    const PROFILE: [(f32, f32); 10] = [
        (0.0, 4.2),
        (0.06, 7.6),
        (0.16, 10.0),
        (0.3, 11.0),
        (0.45, 10.2),
        (0.6, 8.2),
        (0.74, 5.6),
        (0.86, 3.4),
        (0.95, 2.4),
        (1.0, 2.0),
    ];
    for pair in PROFILE.windows(2) {
        let ((start, start_width), (end, end_width)) = (pair[0], pair[1]);
        if along <= end {
            let fraction = ((along - start) / (end - start)).clamp(0.0, 1.0);
            let eased = fraction * fraction * (3.0 - 2.0 * fraction);
            return start_width + (end_width - start_width) * eased;
        }
    }
    2.0
}

fn add(point: Point, offset: Point, amount: f32) -> Point {
    (point.0 + offset.0 * amount, point.1 + offset.1 * amount)
}

impl PondScene {
    fn half_width(&self, along: f32) -> f32 {
        body_half_width(along) * self.koi_scale
    }

    /// The koi's spine at a phase of the loop, nose first.
    fn spine(&self, koi_look: &KoiLook, phase: f32) -> Vec<SpinePoint> {
        let motion = &self.motion;
        let breathing = 1.0 + 0.08 * (TAU * phase + koi_look.phase_offset).sin();
        let (radius_x, radius_y) = (SWIM_RADIUS_X * breathing, SWIM_RADIUS_Y * breathing);
        let mut angle = TAU * motion.laps * phase + koi_look.phase_offset;
        let step = BODY_LENGTH * self.koi_scale / (SPINE_POINTS - 1) as f32;
        let beat = TAU * motion.tail_beats * phase;
        let mut positions = Vec::with_capacity(SPINE_POINTS);
        for index in 0..SPINE_POINTS {
            let (sin_angle, cos_angle) = angle.sin_cos();
            let velocity = (-radius_x * sin_angle, radius_y * cos_angle);
            let speed = (velocity.0 * velocity.0 + velocity.1 * velocity.1).sqrt();
            let tangent = (velocity.0 / speed, velocity.1 / speed);
            let along = index as f32 / (SPINE_POINTS - 1) as f32;
            let sway =
                (beat - along * 5.5).sin() * motion.sway * along * along + (beat + 1.0).sin() * 0.5;
            positions.push((
                SWIM_CENTER.0 + radius_x * cos_angle - tangent.1 * sway,
                SWIM_CENTER.1 + radius_y * sin_angle + tangent.0 * sway,
            ));
            angle -= step / speed;
        }
        (0..SPINE_POINTS)
            .map(|index| {
                let ahead = positions[index.saturating_sub(1)];
                let behind = positions[(index + 1).min(SPINE_POINTS - 1)];
                let (dx, dy) = (ahead.0 - behind.0, ahead.1 - behind.1);
                let length = (dx * dx + dy * dy).sqrt().max(1e-6);
                let tangent = (dx / length, dy / length);
                SpinePoint {
                    position: positions[index],
                    tangent,
                    normal: (-tangent.1, tangent.0),
                }
            })
            .collect()
    }

    fn body_path(&self, spine: &[SpinePoint]) -> Path {
        let head = spine[0];
        let mut outline = vec![
            add(head.position, head.tangent, 5.6 * self.koi_scale),
            add(
                add(head.position, head.tangent, 3.8 * self.koi_scale),
                head.normal,
                3.4 * self.koi_scale,
            ),
        ];
        let along = |index: usize| index as f32 / (SPINE_POINTS - 1) as f32;
        for (index, point) in spine.iter().enumerate() {
            outline.push(add(
                point.position,
                point.normal,
                self.half_width(along(index)),
            ));
        }
        for (index, point) in spine.iter().enumerate().rev() {
            outline.push(add(
                point.position,
                point.normal,
                -self.half_width(along(index)),
            ));
        }
        outline.push(add(
            add(head.position, head.tangent, 3.8 * self.koi_scale),
            head.normal,
            -3.4 * self.koi_scale,
        ));
        let mut path = Path::new();
        path.smooth_closed(&outline);
        path
    }

    fn draw_koi_shadow(&self, canvas: &mut Canvas, spine: &[SpinePoint], night: bool) {
        let mut shadow = Mask::from_path(&self.body_path(spine), SCALE, canvas, 14);
        shadow.blur(4);
        let color = if night {
            [0.0, 0.02, 0.08, self.motion.shadow_alpha + 0.1]
        } else {
            [0.0, 0.12, 0.16, self.motion.shadow_alpha]
        };
        let (offset_x, offset_y) = self.motion.shadow_offset;
        canvas.shade(
            &shadow,
            (offset_x * SCALE).round() as isize,
            (offset_y * SCALE).round() as isize,
            color,
            (POND_BOTTOM * SCALE) as usize,
        );
    }

    fn draw_koi(&self, canvas: &mut Canvas, koi_look: &KoiLook, spine: &[SpinePoint], phase: f32) {
        let water = water_colors(self.season)[0];
        let tint = |color: Rgba| {
            let tinted = mix(color, water, self.motion.depth_tint);
            [tinted[0], tinted[1], tinted[2], color[3]]
        };
        self.draw_tail(canvas, koi_look, spine, phase);
        self.draw_fins(canvas, koi_look, spine, 3, 1.0, phase);
        self.draw_fins(canvas, koi_look, spine, 9, 0.62, phase);
        let body = self.body_path(spine);
        canvas.fill(&body, &Paint::Solid(tint(koi_look.body)), Blend::Over);
        let clip = Mask::from_path(&body, SCALE, canvas, 0);
        for plate in &koi_look.plates {
            let index = (plate.along * (SPINE_POINTS - 1) as f32).round() as usize;
            let point = spine[index];
            let half_width = self.half_width(plate.along);
            let mut plate_path = Path::new();
            plate_path.ellipse(
                add(point.position, point.normal, half_width * plate.side),
                half_width * plate.size * 1.35,
                half_width * plate.size,
                point.tangent.1.atan2(point.tangent.0),
            );
            canvas.fill_clipped(
                &plate_path,
                &Paint::Solid(tint(plate.color)),
                Blend::Over,
                Some(&clip),
            );
        }
        let back: Vec<Point> = smooth_open(
            &spine[1..=SPINE_POINTS - 4]
                .iter()
                .map(|point| point.position)
                .collect::<Vec<_>>(),
        );
        canvas.stroke_clipped(
            &back,
            9.0 * self.koi_scale,
            false,
            &Paint::Solid(koi_look.wide_sheen),
            Blend::Over,
            &clip,
        );
        canvas.stroke_clipped(
            &back,
            2.2 * self.koi_scale,
            false,
            &Paint::Solid(koi_look.sheen),
            Blend::Over,
            &clip,
        );
        let outline: Vec<Point> = {
            let along = |index: usize| index as f32 / (SPINE_POINTS - 1) as f32;
            let mut points: Vec<Point> = spine
                .iter()
                .enumerate()
                .map(|(index, point)| {
                    add(point.position, point.normal, self.half_width(along(index)))
                })
                .collect();
            points.extend(spine.iter().enumerate().rev().map(|(index, point)| {
                add(point.position, point.normal, -self.half_width(along(index)))
            }));
            points
        };
        canvas.stroke_clipped(
            &smooth_closed_points(&outline),
            3.0,
            true,
            &Paint::Solid(koi_look.edge),
            Blend::Over,
            &clip,
        );
        let dorsal: Vec<Point> = smooth_open(
            &spine[4..=10]
                .iter()
                .map(|point| point.position)
                .collect::<Vec<_>>(),
        );
        canvas.stroke(
            &dorsal,
            1.0,
            false,
            &Paint::Solid(koi_look.dorsal),
            Blend::Over,
        );
        let head = spine[0];
        for side in [-1.0, 1.0] {
            let eye = add(
                add(head.position, head.tangent, 2.0 * self.koi_scale),
                head.normal,
                side * 3.6 * self.koi_scale,
            );
            let mut eye_path = Path::new();
            eye_path.circle(eye, 1.15 * self.koi_scale.max(0.9));
            canvas.fill(&eye_path, &Paint::Solid(koi_look.eye), Blend::Over);
            let mut glint = Path::new();
            glint.circle((eye.0 - 0.35, eye.1 - 0.35), 0.4);
            canvas.fill(&glint, &Paint::Solid([1.0, 1.0, 1.0, 0.8]), Blend::Over);
        }
        if self.glimmer > 0.0 {
            let sparkle_count = (self.glimmer * 6.0).ceil() as u32;
            for sparkle in 0..sparkle_count {
                let seed = sparkle * 7 + (koi_look.phase_offset > 0.0) as u32 * 101;
                let brightness =
                    (TAU * (3.0 * phase + random(2000 + seed))).sin().max(0.0) * self.glimmer;
                if brightness < 0.05 {
                    continue;
                }
                let index = 3 + (sparkle as usize * 3) % 10;
                let point = spine[index];
                let center = add(
                    point.position,
                    point.normal,
                    (random(2100 + seed) - 0.5) * self.half_width(index as f32 / 17.0),
                );
                draw_sparkle(canvas, center, 2.4 * brightness, brightness * 0.9);
            }
        }
    }

    /// Light the koi's pale colors after the moonlight tint, so they glow against the
    /// darkened water.
    fn draw_moonlight_on_koi(&self, canvas: &mut Canvas, koi_look: &KoiLook, spine: &[SpinePoint]) {
        let body = self.body_path(spine);
        let lift = |color: Rgba| {
            [
                color[0],
                color[1],
                color[2],
                0.26 * (1.0 - self.motion.depth_tint),
            ]
        };
        canvas.fill(&body, &Paint::Solid(lift(koi_look.body)), Blend::Plus);
        let body_lightness =
            0.3 * koi_look.body[0] + 0.59 * koi_look.body[1] + 0.11 * koi_look.body[2];
        if body_lightness > 0.5 {
            return;
        }
        let clip = Mask::from_path(&body, SCALE, canvas, 0);
        for plate in &koi_look.plates {
            let point = spine[(plate.along * (SPINE_POINTS - 1) as f32).round() as usize];
            let half_width = self.half_width(plate.along);
            let mut plate_path = Path::new();
            plate_path.ellipse(
                add(point.position, point.normal, half_width * plate.side),
                half_width * plate.size * 1.35,
                half_width * plate.size,
                point.tangent.1.atan2(point.tangent.0),
            );
            canvas.fill_clipped(
                &plate_path,
                &Paint::Solid(lift(plate.color)),
                Blend::Plus,
                Some(&clip),
            );
        }
    }

    fn draw_tail(&self, canvas: &mut Canvas, koi_look: &KoiLook, spine: &[SpinePoint], phase: f32) {
        let end = spine[SPINE_POINTS - 1];
        let before = spine[SPINE_POINTS - 3];
        let (dx, dy) = (
            end.position.0 - before.position.0,
            end.position.1 - before.position.1,
        );
        let length = (dx * dx + dy * dy).sqrt().max(1e-6);
        let backward = (dx / length, dy / length);
        let side = (-backward.1, backward.0);
        let swing = (TAU * self.motion.tail_beats * phase - 5.9).sin();
        let scale = self.koi_scale;
        let at = |back: f32, across: f32| {
            add(
                add(end.position, backward, back * scale),
                side,
                across * scale,
            )
        };
        let (tail_length, tail_width) = (27.0, 14.0);
        let mut tail = Path::new();
        tail.move_to(at(0.0, 2.4));
        tail.quad_to(
            at(11.0, 9.0 + swing * 3.0),
            at(tail_length, tail_width + swing * 6.0),
        );
        tail.quad_to(at(19.0, 4.0 + swing * 5.0), at(15.0, swing * 5.0));
        tail.quad_to(
            at(19.0, -4.0 + swing * 5.0),
            at(tail_length, -tail_width + swing * 6.0),
        );
        tail.quad_to(at(11.0, -9.0 + swing * 3.0), at(0.0, -2.4));
        tail.close();
        canvas.fill(&tail, &Paint::Solid(koi_look.fin), Blend::Over);
        let edge = [
            at(0.0, 2.4),
            at(tail_length * 0.55, 10.5 + swing * 4.5),
            at(tail_length, tail_width + swing * 6.0),
            at(15.0, swing * 5.0),
            at(tail_length, -tail_width + swing * 6.0),
            at(tail_length * 0.55, -10.5 + swing * 4.5),
            at(0.0, -2.4),
        ];
        canvas.stroke(
            &smooth_open(&edge),
            0.7,
            false,
            &Paint::Solid(koi_look.fin_edge),
            Blend::Over,
        );
        let mut rays = Path::new();
        for ray in -3..=3 {
            let reach = 13.0 + (ray as f32).abs() * 4.2;
            let across = ray as f32 * (tail_width / 3.2) + swing * (5.0 + (ray as f32).abs() * 0.3);
            rays.extend(&stroke_path(&[end.position, at(reach, across)], 0.5, false));
        }
        canvas.fill(&rays, &Paint::Solid(koi_look.ray), Blend::Over);
    }

    fn draw_fins(
        &self,
        canvas: &mut Canvas,
        koi_look: &KoiLook,
        spine: &[SpinePoint],
        index: usize,
        fin_scale: f32,
        phase: f32,
    ) {
        let point = spine[index];
        let half_width = self.half_width(index as f32 / (SPINE_POINTS - 1) as f32);
        let size = fin_scale * self.koi_scale;
        for side in [-1.0_f32, 1.0] {
            let flap = (TAU * self.motion.tail_beats * 0.5 * phase
                + koi_look.phase_offset
                + index as f32
                + if side > 0.0 { 0.0 } else { 1.3 })
            .sin()
                * 0.3;
            let mut direction = (
                point.normal.0 * side * 0.8 - point.tangent.0 * 0.75,
                point.normal.1 * side * 0.8 - point.tangent.1 * 0.75,
            );
            let length = (direction.0 * direction.0 + direction.1 * direction.1).sqrt();
            direction = (direction.0 / length, direction.1 / length);
            let (sin_flap, cos_flap) = (flap * side).sin_cos();
            let direction = (
                direction.0 * cos_flap - direction.1 * sin_flap,
                direction.0 * sin_flap + direction.1 * cos_flap,
            );
            let base = add(point.position, point.normal, side * (half_width - 1.5));
            let along = |out: f32, forward: f32| {
                add(
                    add(base, direction, out * size),
                    point.tangent,
                    forward * size,
                )
            };
            let mut fin = Path::new();
            fin.move_to(along(0.0, 2.5));
            fin.quad_to(along(7.0, 5.0), along(12.0, 0.0));
            fin.quad_to(along(5.0, -2.0), along(0.0, -2.0));
            fin.close();
            canvas.fill(&fin, &Paint::Solid(koi_look.fin), Blend::Over);
            canvas.stroke(
                &smooth_open(&[
                    along(0.0, 2.5),
                    along(7.0, 5.0),
                    along(12.0, 0.0),
                    along(5.0, -2.0),
                ]),
                0.6,
                false,
                &Paint::Solid(koi_look.fin_edge),
                Blend::Over,
            );
            let mut rays = Path::new();
            for reach in [0.0, 0.5, 1.0] {
                rays.extend(&stroke_path(
                    &[base, along(9.0 + 2.0 * reach, 4.5 - 6.0 * reach)],
                    0.45,
                    false,
                ));
            }
            canvas.fill(&rays, &Paint::Solid(koi_look.ray), Blend::Over);
        }
    }

    fn draw_ripples(&self, canvas: &mut Canvas, phase: f32) {
        let motion = &self.motion;
        for (koi_index, koi_look) in self.koi.iter().enumerate() {
            for ripple in 0..motion.tail_ripples {
                let start = (ripple as f32 + 0.5 * koi_index as f32) / motion.tail_ripples as f32;
                let age = (phase - start).rem_euclid(1.0);
                if age < RIPPLE_LIFE {
                    let tail = self.spine(koi_look, start)[SPINE_POINTS - 1].position;
                    draw_ripple(canvas, tail, age / RIPPLE_LIFE, 16.0);
                }
            }
            for gulp in 0..motion.gulps {
                let start = (gulp as f32 + 0.25 + 0.5 * koi_index as f32) / motion.gulps as f32;
                let age = (phase - start).rem_euclid(1.0);
                if age < RIPPLE_LIFE {
                    let head = self.spine(koi_look, start)[0];
                    let mouth = add(head.position, head.tangent, 6.0);
                    draw_ripple(canvas, mouth, age / RIPPLE_LIFE, 9.0);
                    let rise = age / RIPPLE_LIFE;
                    for bubble in 0..2 {
                        let mut bubble_path = Path::new();
                        bubble_path.circle(
                            (
                                mouth.0 + (bubble as f32 - 0.5) * 3.0,
                                mouth.1 - rise * 4.0 - bubble as f32,
                            ),
                            0.9 + bubble as f32 * 0.4,
                        );
                        canvas.fill(
                            &bubble_path,
                            &Paint::Solid([1.0, 1.0, 1.0, 0.55 * (1.0 - rise)]),
                            Blend::Over,
                        );
                    }
                }
            }
        }
        for (ripple, start) in [0.08_f32, 0.33, 0.58, 0.83].into_iter().enumerate() {
            let age = (phase - start).rem_euclid(1.0);
            if age < RIPPLE_LIFE {
                let center = (
                    20.0 + random(500 + ripple as u32) * 360.0,
                    15.0 + random(520 + ripple as u32) * 178.0,
                );
                draw_ripple(canvas, center, age / RIPPLE_LIFE, 22.0);
            }
        }
    }
}

fn smooth_closed_points(points: &[Point]) -> Vec<Point> {
    let mut wrapped = points.to_vec();
    wrapped.push(points[0]);
    wrapped.push(points[1]);
    let smoothed = smooth_open(&wrapped);
    smoothed[1..smoothed.len().saturating_sub(1)].to_vec()
}

fn draw_ripple(canvas: &mut Canvas, center: Point, age: f32, reach: f32) {
    let radius = 3.0 + age * reach;
    let strength = (1.0 - age) * (age * 5.0).min(1.0);
    let ring = |radius: f32| -> Vec<Point> {
        (0..40)
            .map(|step| {
                let angle = step as f32 / 40.0 * TAU;
                (
                    center.0 + radius * angle.cos(),
                    center.1 + radius * 0.82 * angle.sin(),
                )
            })
            .collect()
    };
    canvas.stroke(
        &ring(radius),
        0.9,
        true,
        &Paint::Solid([1.0, 1.0, 1.0, strength * 0.4]),
        Blend::Over,
    );
    if radius > 6.0 {
        canvas.stroke(
            &ring(radius * 0.6),
            0.9,
            true,
            &Paint::Solid([1.0, 1.0, 1.0, strength * 0.2]),
            Blend::Over,
        );
    }
}

fn draw_sparkle(canvas: &mut Canvas, center: Point, length: f32, alpha: f32) {
    let paint = Paint::Solid([1.0, 1.0, 0.94, alpha.min(1.0)]);
    canvas.stroke(
        &[(center.0 - length, center.1), (center.0 + length, center.1)],
        0.7,
        false,
        &paint,
        Blend::Plus,
    );
    canvas.stroke(
        &[(center.0, center.1 - length), (center.0, center.1 + length)],
        0.7,
        false,
        &paint,
        Blend::Plus,
    );
    let mut dot = Path::new();
    dot.circle(center, 0.8);
    canvas.fill(
        &dot,
        &Paint::Solid([1.0, 1.0, 1.0, alpha.min(1.0)]),
        Blend::Plus,
    );
}

/// The water's center and edge colors by day.
fn water_colors(season: Season) -> [Rgba; 2] {
    match season {
        Season::Spring => [hex(0x4aa3a6), hex(0x1d5966)],
        Season::Summer => [hex(0x3aa596), hex(0x175a5c)],
        Season::Autumn => [hex(0x5f9488), hex(0x284f52)],
        Season::Winter => [hex(0x82b6d0), hex(0x3a6585)],
    }
}

/// The moonlight the pond is multiplied by at night.
fn night_tint(season: Season) -> Rgba {
    hex(match season {
        Season::Spring => 0x4f62a6,
        Season::Summer => 0x4a64a0,
        Season::Autumn => 0x55599a,
        Season::Winter => 0x6476b0,
    })
}

fn pond_rect() -> Path {
    let mut path = Path::new();
    path.rect(0.0, 0.0, WIDTH, POND_BOTTOM);
    path
}

/// Water, pond-floor pebbles, light, caustics, the edge shadow, and winter ice.
fn draw_background(season: Season, theme: PondTheme) -> Canvas {
    let mut canvas = Canvas::new(
        POND_WIDTH_PIXELS as usize,
        POND_HEIGHT_PIXELS as usize,
        SCALE,
    );
    let [center_color, edge_color] = water_colors(season);
    canvas.fill(
        &pond_rect(),
        &Paint::Radial {
            center: (SWIM_CENTER.0, SWIM_CENTER.1 - 10.0),
            radius: 260.0,
            stops: vec![(0.08, center_color), (1.0, edge_color)],
        },
        Blend::Over,
    );
    for pebble in 0..30 {
        let mut pebble_path = Path::new();
        pebble_path.ellipse(
            (
                random(10 + pebble) * WIDTH,
                random(50 + pebble) * POND_BOTTOM,
            ),
            2.0 + random(90 + pebble) * 5.0,
            1.5 + random(130 + pebble) * 3.5,
            random(170 + pebble) * 3.0,
        );
        let color = if random(210 + pebble) > 0.5 {
            [0.0, 0.1, 0.12, 0.1]
        } else {
            [1.0, 1.0, 0.94, 0.06]
        };
        canvas.fill(&pebble_path, &Paint::Solid(color), Blend::Over);
    }
    if theme == PondTheme::Light {
        canvas.fill(
            &pond_rect(),
            &Paint::Radial {
                center: (110.0, 30.0),
                radius: 240.0,
                stops: vec![
                    (0.0, [1.0, 0.98, 0.86, 0.16]),
                    (1.0, [1.0, 0.98, 0.86, 0.0]),
                ],
            },
            Blend::Over,
        );
    }
    let caustic_alpha = if theme == PondTheme::Dark {
        0.05
    } else {
        0.075
    };
    let mut caustic_lines: Vec<Vec<Point>> = Vec::new();
    for line in 0..12 {
        let base = line as f32 * 19.0 - 6.0;
        caustic_lines.push(
            (0..=70)
                .map(|step| {
                    let x = -10.0 + step as f32 * 6.0;
                    (
                        x,
                        base + (x * 0.035 + 1.2 + line as f32 * 1.7).sin() * 6.0
                            + (x * 0.09 - 1.7 + line as f32).sin() * 2.5,
                    )
                })
                .collect(),
        );
    }
    for line in 0..14 {
        let base = line as f32 * 31.0 - 10.0;
        caustic_lines.push(
            (0..=38)
                .map(|step| {
                    let y = -10.0 + step as f32 * 6.0;
                    (
                        base + (y * 0.05 + 0.9 + line as f32 * 2.3).sin() * 7.0
                            + (y * 0.11 + 1.4 + line as f32).sin() * 3.0,
                        y,
                    )
                })
                .collect(),
        );
    }
    let mut wide_caustics = Path::new();
    let mut fine_caustics = Path::new();
    for line in &caustic_lines {
        wide_caustics.extend(&stroke_path(line, 3.0, false));
        fine_caustics.extend(&stroke_path(line, 1.0, false));
    }
    let caustic_color = [0.82, 1.0, 0.96];
    canvas.fill(
        &wide_caustics,
        &Paint::Solid([
            caustic_color[0],
            caustic_color[1],
            caustic_color[2],
            caustic_alpha * 0.4,
        ]),
        Blend::Plus,
    );
    canvas.fill(
        &fine_caustics,
        &Paint::Solid([
            caustic_color[0],
            caustic_color[1],
            caustic_color[2],
            caustic_alpha,
        ]),
        Blend::Plus,
    );
    canvas.fill(
        &pond_rect(),
        &Paint::Radial {
            center: SWIM_CENTER,
            radius: 250.0,
            stops: vec![
                (0.0, [0.0, 0.08, 0.12, 0.0]),
                (0.36, [0.0, 0.08, 0.12, 0.0]),
                (1.0, [0.0, 0.06, 0.1, 0.4]),
            ],
        },
        Blend::Over,
    );
    if season == Season::Winter {
        draw_ice(&mut canvas);
    }
    canvas
}

/// Irregular ice shelves along every edge, frosted toward the open water.
fn draw_ice(canvas: &mut Canvas) {
    let depth = |position: f32, offset: f32| {
        7.0 + 5.0 * (position * 0.05 + offset).sin()
            + 3.0 * (position * 0.13 + 1.3 + offset).sin()
            + 1.8 * (position * 0.31 + offset * 2.0).sin()
    };
    let edge_points = |edge: usize, reach: f32| -> Vec<Point> {
        let along: Vec<f32> = if edge < 2 {
            (0..=82).map(|step| -5.0 + step as f32 * 5.0).collect()
        } else {
            (0..=44).map(|step| -5.0 + step as f32 * 5.0).collect()
        };
        along
            .into_iter()
            .map(|position| {
                let inset = depth(position, edge as f32 * 2.0) * reach;
                match edge {
                    0 => (position, inset),
                    1 => (position, POND_BOTTOM - inset * 0.9),
                    2 => (inset * 0.8, position),
                    _ => (WIDTH - inset * 0.8, position),
                }
            })
            .collect()
    };
    let corners: [[Point; 2]; 4] = [
        [(WIDTH + 5.0, -5.0), (-5.0, -5.0)],
        [(WIDTH + 5.0, POND_BOTTOM + 5.0), (-5.0, POND_BOTTOM + 5.0)],
        [(-5.0, POND_BOTTOM + 5.0), (-5.0, -5.0)],
        [(WIDTH + 5.0, POND_BOTTOM + 5.0), (WIDTH + 5.0, -5.0)],
    ];
    for (reach, alpha) in [(1.7, 0.28), (1.0, 0.95)] {
        for (edge, edge_corners) in corners.iter().enumerate() {
            let points = edge_points(edge, reach);
            let mut shelf = points.clone();
            shelf.extend(edge_corners);
            let mut shelf_path = Path::new();
            shelf_path.polygon(&shelf);
            canvas.fill(
                &shelf_path,
                &Paint::Solid([0.92, 0.96, 0.99, alpha]),
                Blend::Over,
            );
            if reach == 1.0 {
                canvas.stroke(
                    &points,
                    0.9,
                    false,
                    &Paint::Solid([0.63, 0.78, 0.89, 0.95]),
                    Blend::Over,
                );
            }
        }
    }
    for sparkle in 0..40 {
        let edge = sparkle % 4;
        let along = random(3000 + sparkle);
        let point = match edge {
            0 => (along * WIDTH, 1.0 + random(3100 + sparkle) * 4.0),
            1 => (
                along * WIDTH,
                POND_BOTTOM - 1.0 - random(3100 + sparkle) * 4.0,
            ),
            2 => (1.0 + random(3100 + sparkle) * 3.0, along * POND_BOTTOM),
            _ => (
                WIDTH - 1.0 - random(3100 + sparkle) * 3.0,
                along * POND_BOTTOM,
            ),
        };
        let mut dot = Path::new();
        dot.circle(point, 0.6);
        canvas.fill(&dot, &Paint::Solid([1.0, 1.0, 1.0, 0.9]), Blend::Over);
    }
}

/// A lily pad, and whether a lotus blooms on it.
struct Pad {
    center: Point,
    radius: f32,
    notch: f32,
    lotus: bool,
}

/// Lotus pads in bloom order, then plain pads, plus summer's extra pads.
fn lay_out_pads(season: Season, lotus_count: usize) -> Vec<Pad> {
    const LOTUS_SLOTS: [(f32, f32, f32, f32); MAX_LOTUS_COUNT as usize] = [
        (36.0, 34.0, 15.0, 0.7),
        (356.0, 40.0, 17.0, 3.5),
        (70.0, 176.0, 13.0, 2.3),
        // Sits above the signature corner, so the date beside the seal stays clear.
        (330.0, 164.0, 14.0, 5.0),
        (378.0, 140.0, 11.0, 1.3),
        (16.0, 112.0, 10.0, 4.1),
        (262.0, 194.0, 9.0, 0.3),
        (120.0, 20.0, 9.0, 2.8),
        (236.0, 14.0, 9.0, 4.4),
    ];
    const SUMMER_PADS: [(f32, f32, f32, f32); 3] = [
        (178.0, 196.0, 8.0, 1.6),
        (388.0, 92.0, 8.0, 3.1),
        (84.0, 64.0, 6.0, 0.9),
    ];
    let mut pads: Vec<Pad> = LOTUS_SLOTS
        .iter()
        .enumerate()
        .map(|(slot, &(x, y, radius, notch))| Pad {
            center: (x, y),
            radius,
            notch,
            lotus: slot < lotus_count,
        })
        .collect();
    if season == Season::Summer {
        pads.extend(SUMMER_PADS.iter().map(|&(x, y, radius, notch)| Pad {
            center: (x, y),
            radius,
            notch,
            lotus: false,
        }));
    }
    pads
}

fn pad_path(pad: &Pad) -> Path {
    let steps = 40;
    let mut points = vec![pad.center];
    for step in 0..=steps {
        let angle = pad.notch + 0.3 + (TAU - 0.6) * step as f32 / steps as f32;
        points.push((
            pad.center.0 + pad.radius * angle.cos(),
            pad.center.1 + pad.radius * angle.sin(),
        ));
    }
    let mut path = Path::new();
    path.polygon(&points);
    path
}

/// Pads with their soft shadows, and the lotus blooming on them.
fn draw_surface(pads: &[Pad], season: Season) -> Canvas {
    let mut canvas = Canvas::new(
        POND_WIDTH_PIXELS as usize,
        POND_HEIGHT_PIXELS as usize,
        SCALE,
    );
    let [base, rim, light, vein]: [Rgba; 4] = match season {
        Season::Spring => [
            hex(0x62b052),
            hex(0x2f6e2e),
            hex(0xa4dc88),
            [0.88, 1.0, 0.8, 0.35],
        ],
        Season::Summer => [
            hex(0x3f9c4a),
            hex(0x1d592b),
            hex(0x82cf77),
            [0.84, 1.0, 0.8, 0.3],
        ],
        Season::Autumn => [
            hex(0xa69c41),
            hex(0x6a5722),
            hex(0xd6c56c),
            [1.0, 0.94, 0.76, 0.35],
        ],
        Season::Winter => [
            hex(0x4c7a5d),
            hex(0x2c4e3c),
            hex(0x7ca78c),
            [0.88, 0.95, 0.91, 0.3],
        ],
    };
    let mut all_pads = Path::new();
    for pad in pads {
        all_pads.extend(&pad_path(pad));
    }
    let mut shadow = Mask::from_path(&all_pads, SCALE, &canvas, 12);
    shadow.blur(3);
    canvas.shade(
        &shadow,
        (2.0 * SCALE) as isize,
        (3.0 * SCALE) as isize,
        [0.0, 0.08, 0.1, 0.32],
        (POND_BOTTOM * SCALE) as usize,
    );
    for (pad_index, pad) in pads.iter().enumerate() {
        let path = pad_path(pad);
        let (x, y) = pad.center;
        canvas.fill(
            &path,
            &Paint::Radial {
                center: (x - pad.radius * 0.3, y - pad.radius * 0.35),
                radius: pad.radius * 1.15,
                stops: vec![(0.0, light), (0.55, base), (1.0, rim)],
            },
            Blend::Over,
        );
        let clip = Mask::from_path(&path, SCALE, &canvas, 0);
        let mut veins = Path::new();
        for vein_index in 0..9 {
            let angle = pad.notch + 0.55 + vein_index as f32 * (TAU - 1.1) / 8.0;
            veins.extend(&stroke_path(
                &[
                    pad.center,
                    (x + angle.cos() * pad.radius, y + angle.sin() * pad.radius),
                ],
                0.6,
                false,
            ));
        }
        canvas.fill_clipped(&veins, &Paint::Solid(vein), Blend::Over, Some(&clip));
        if season == Season::Autumn {
            for spot in 0..4 {
                let seed = pad_index as u32 * 13 + spot;
                let mut spot_path = Path::new();
                spot_path.circle(
                    (
                        x + (random(4000 + seed) - 0.5) * pad.radius * 1.4,
                        y + (random(4100 + seed) - 0.5) * pad.radius * 1.4,
                    ),
                    pad.radius * (0.08 + random(4200 + seed) * 0.12),
                );
                canvas.fill_clipped(
                    &spot_path,
                    &Paint::Solid([0.49, 0.27, 0.11, 0.55]),
                    Blend::Over,
                    Some(&clip),
                );
            }
        }
        if season == Season::Winter {
            let mut snow = Path::new();
            snow.ellipse(
                (x - pad.radius * 0.15, y - pad.radius * 0.5),
                pad.radius * 0.95,
                pad.radius * 0.6,
                -0.15,
            );
            canvas.fill_clipped(
                &snow,
                &Paint::Solid([0.97, 0.98, 1.0, 0.95]),
                Blend::Over,
                Some(&clip),
            );
        }
        let mut rim_points: Vec<Point> = vec![pad.center];
        for step in 0..=40 {
            let angle = pad.notch + 0.3 + (TAU - 0.6) * step as f32 / 40.0;
            rim_points.push((x + pad.radius * angle.cos(), y + pad.radius * angle.sin()));
        }
        canvas.stroke(
            &rim_points,
            0.7,
            true,
            &Paint::Solid([0.04, 0.16, 0.08, 0.35]),
            Blend::Over,
        );
        if pad.lotus {
            draw_lotus(
                &mut canvas,
                pad.center,
                (pad.radius / 14.0).clamp(0.7, 1.1),
                pad.notch,
                season,
            );
        }
    }
    canvas
}

/// A lotus seen from above: eight outer petals, six inner, and a seed pod.
fn draw_lotus(canvas: &mut Canvas, center: Point, size: f32, turn: f32, season: Season) {
    let frosted = season == Season::Winter;
    let (outer_base, outer_tip, inner_base, inner_tip) = if frosted {
        (hex(0xe7a1bd), hex(0xffffff), hex(0xefb6cb), hex(0xffffff))
    } else {
        (hex(0xe4689a), hex(0xffe0ec), hex(0xef8ab1), hex(0xfff2f7))
    };
    let petal =
        |canvas: &mut Canvas, angle: f32, length: f32, width: f32, base: Rgba, tip: Rgba| {
            let direction = (angle.cos(), angle.sin());
            let mut path = Path::new();
            path.ellipse(
                add(center, direction, length * 0.52),
                length * 0.52,
                width,
                angle,
            );
            canvas.fill(
                &path,
                &Paint::Linear {
                    from: center,
                    to: add(center, direction, length),
                    stops: vec![(0.0, base), (1.0, tip)],
                },
                Blend::Over,
            );
            let outline = {
                let mut points = Vec::new();
                for step in 0..32 {
                    let theta = step as f32 / 32.0 * TAU;
                    let (x, y) = (length * 0.52 * theta.cos(), width * theta.sin());
                    let middle = add(center, direction, length * 0.52);
                    points.push((
                        middle.0 + x * direction.0 - y * direction.1,
                        middle.1 + x * direction.1 + y * direction.0,
                    ));
                }
                points
            };
            canvas.stroke(
                &outline,
                0.45,
                true,
                &Paint::Solid([0.71, 0.24, 0.41, 0.28]),
                Blend::Over,
            );
        };
    for index in 0..8 {
        petal(
            canvas,
            index as f32 / 8.0 * TAU + turn,
            12.0 * size,
            4.4 * size,
            outer_base,
            outer_tip,
        );
    }
    for index in 0..6 {
        petal(
            canvas,
            index as f32 / 6.0 * TAU + turn + 0.45,
            8.2 * size,
            3.4 * size,
            inner_base,
            inner_tip,
        );
    }
    let pod_radius = 3.1 * size;
    let mut pod = Path::new();
    pod.circle(center, pod_radius);
    canvas.fill(&pod, &Paint::Solid(hex(0xf0c64a)), Blend::Over);
    for seed in 0..6 {
        let angle = seed as f32 * 1.05;
        let mut seed_path = Path::new();
        seed_path.circle(
            add(center, (angle.cos(), angle.sin()), pod_radius * 0.55),
            0.45,
        );
        canvas.fill(&seed_path, &Paint::Solid(hex(0xc18f22)), Blend::Over);
    }
}

/// The light added after the moonlight tint: the moon's reflection and each lotus's glow.
fn draw_night_glow(pads: &[Pad]) -> Canvas {
    let mut canvas = Canvas::new(
        POND_WIDTH_PIXELS as usize,
        POND_HEIGHT_PIXELS as usize,
        SCALE,
    );
    let glow = |canvas: &mut Canvas, center: Point, radius: f32, color: Rgba| {
        let mut path = Path::new();
        path.circle(center, radius);
        canvas.fill(
            &path,
            &Paint::Radial {
                center,
                radius,
                stops: vec![(0.0, color), (1.0, with_alpha(color, 0.0))],
            },
            Blend::Plus,
        );
    };
    glow(&mut canvas, MOON_CENTER, 34.0, [0.75, 0.8, 1.0, 0.22]);
    for pad in pads.iter().filter(|pad| pad.lotus) {
        glow(&mut canvas, pad.center, 20.0, [1.0, 0.51, 0.73, 0.2]);
    }
    canvas
}

const MOON_CENTER: Point = (300.0, 28.0);

fn draw_moon_shimmer(canvas: &mut Canvas, phase: f32) {
    for row in 0..10 {
        let y = MOON_CENTER.1 - 12.0 + row as f32 * 3.0;
        let half_width = if row < 7 {
            (1.0 - ((row as f32 - 3.0) / 4.0).powi(2)).max(0.0).sqrt() * 13.0 + 2.0
        } else {
            (10 - row) as f32 * 3.0
        };
        let drift = (TAU * 2.0 * phase + row as f32 * 1.3).sin() * 1.5;
        let mut dashes = Path::new();
        let mut x = -half_width;
        while x < half_width {
            let dash = 3.0 + 3.0 * random(5000 + row * 31 + (x + 40.0) as u32);
            if (x * 0.35 + TAU * 3.0 * phase + row as f32 * 1.9).sin() >= -0.35 {
                let end = (x + dash).min(half_width);
                dashes.extend(&stroke_path(
                    &[
                        (MOON_CENTER.0 + x + drift, y),
                        (MOON_CENTER.0 + end + drift, y),
                    ],
                    1.4,
                    false,
                ));
            }
            x += dash + 1.6;
        }
        canvas.fill(
            &dashes,
            &Paint::Solid([0.92, 0.94, 1.0, 0.5 - row as f32 * 0.035]),
            Blend::Plus,
        );
    }
}

fn draw_reflected_stars(canvas: &mut Canvas, phase: f32) {
    for star in 0..30 {
        let twinkle =
            0.5 + 0.5 * (TAU * ((2 + star % 4) as f32 * phase + random(1030 + star))).sin();
        let mut path = Path::new();
        path.circle(
            (random(900 + star) * WIDTH, random(950 + star) * POND_BOTTOM),
            0.4 + random(990 + star) * 0.6,
        );
        canvas.fill(
            &path,
            &Paint::Solid([0.78, 0.84, 1.0, 0.15 + twinkle * 0.35]),
            Blend::Plus,
        );
    }
}

fn draw_fireflies(canvas: &mut Canvas, season: Season, phase: f32) {
    let count = match season {
        Season::Spring => 7,
        Season::Summer => 14,
        Season::Autumn => 5,
        Season::Winter => 0,
    };
    for firefly in 0..count {
        let home = (
            20.0 + random(1100 + firefly) * 360.0,
            15.0 + random(1150 + firefly) * 178.0,
        );
        let center = (
            home.0
                + 18.0 * (TAU * (phase + random(1200 + firefly))).sin()
                + 4.0 * (TAU * (3.0 * phase + random(1250 + firefly))).sin(),
            home.1 + 10.0 * (TAU * (2.0 * phase + random(1300 + firefly))).sin(),
        );
        let brightness = (TAU * ((3 + firefly % 3) as f32 * phase + random(1350 + firefly)))
            .sin()
            .max(0.0);
        if brightness < 0.05 {
            continue;
        }
        let mut glow = Path::new();
        glow.circle(center, 7.0);
        canvas.fill(
            &glow,
            &Paint::Radial {
                center,
                radius: 7.0,
                stops: vec![
                    (0.0, [0.84, 1.0, 0.51, 0.55 * brightness]),
                    (1.0, [0.84, 1.0, 0.51, 0.0]),
                ],
            },
            Blend::Plus,
        );
        let mut core = Path::new();
        core.circle(center, 0.9);
        canvas.fill(
            &core,
            &Paint::Solid([0.98, 1.0, 0.84, brightness]),
            Blend::Plus,
        );
    }
}

fn draw_glints(canvas: &mut Canvas, phase: f32) {
    for glint in 0..10 {
        let cycles = (5 + glint % 4) as f32;
        let progress = phase * cycles + random(600 + glint);
        let cycle = (progress.floor() as i64).rem_euclid(cycles as i64) as u32;
        let brightness = ((progress - progress.floor()) * PI).sin();
        let center = (
            random(700 + glint * 13 + cycle * 7) * WIDTH,
            random(800 + glint * 17 + cycle * 5) * POND_BOTTOM,
        );
        draw_sparkle(canvas, center, 3.2 * brightness, 0.8 * brightness);
    }
}

fn draw_cherry_petals(canvas: &mut Canvas, phase: f32) {
    for petal in 0..18 {
        let home = (
            random(1400 + petal) * WIDTH,
            8.0 + random(1450 + petal) * (POND_BOTTOM - 16.0),
        );
        let turn = if petal % 2 == 0 { 1.0 } else { -1.0 };
        let angle = TAU * (turn * phase + random(1550 + petal));
        let orbit = 4.0 + random(1500 + petal) * 6.0;
        let center = (
            home.0 + orbit * angle.cos(),
            home.1 + orbit * 0.7 * angle.sin(),
        );
        let rotation = TAU * (random(1600 + petal) + (1 + petal % 2) as f32 * phase * turn);
        let mut path = Path::new();
        path.ellipse(center, 2.8, 1.7, rotation);
        canvas.fill(&path, &Paint::Solid(hex(0xf6bfd2)), Blend::Over);
        let mut highlight = Path::new();
        highlight.ellipse(
            add(center, (rotation.cos(), rotation.sin()), 0.6),
            1.3,
            0.8,
            rotation,
        );
        canvas.fill(&highlight, &Paint::Solid(hex(0xffe8f0)), Blend::Over);
    }
}

fn draw_maple_leaves(canvas: &mut Canvas, phase: f32) {
    let colors = [hex(0xd8492a), hex(0xe8892c), hex(0xc23a22), hex(0xe9b13a)];
    for leaf in 0..11 {
        let home = (
            random(1700 + leaf) * WIDTH,
            8.0 + random(1750 + leaf) * (POND_BOTTOM - 16.0),
        );
        let turn = if leaf % 2 == 0 { 1.0 } else { -1.0 };
        let angle = TAU * (turn * phase + random(1760 + leaf));
        let orbit = 3.0 + random(1770 + leaf) * 5.0;
        let center = (
            home.0 + orbit * angle.cos(),
            home.1 + orbit * 0.7 * angle.sin(),
        );
        let rotation = TAU * (random(1780 + leaf) + phase * turn);
        let size = 4.6 + random(1790 + leaf) * 2.0;
        let points: Vec<Point> = (0..10)
            .map(|corner| {
                let corner_angle = rotation + corner as f32 * PI / 5.0;
                let reach = if corner % 2 == 1 { size * 0.45 } else { size };
                (
                    center.0 + corner_angle.cos() * reach,
                    center.1 + corner_angle.sin() * reach,
                )
            })
            .collect();
        let mut path = Path::new();
        path.polygon(&points);
        let color = colors[leaf as usize % colors.len()];
        canvas.fill(&path, &Paint::Solid(color), Blend::Over);
        canvas.stroke(
            &points,
            0.4,
            true,
            &Paint::Solid([0.35, 0.12, 0.04, 0.35]),
            Blend::Over,
        );
        let stem_end = (
            center.0 - (rotation + 0.3).cos() * size * 1.3,
            center.1 - (rotation + 0.3).sin() * size * 1.3,
        );
        canvas.stroke(
            &[center, stem_end],
            0.6,
            false,
            &Paint::Solid([0.43, 0.22, 0.08, 0.7]),
            Blend::Over,
        );
    }
}

fn draw_snow(canvas: &mut Canvas, phase: f32, night: bool) {
    for flake in 0..55 {
        let fall = (1 + flake % 2) as f32;
        let y = (random(1800 + flake) * POND_BOTTOM + phase * fall * POND_BOTTOM)
            .rem_euclid(POND_BOTTOM);
        let x = random(1850 + flake) * WIDTH
            + 6.0 * (TAU * ((2 + flake % 2) as f32 * phase + random(1900 + flake))).sin();
        let mut path = Path::new();
        path.circle((x, y), 0.6 + random(1950 + flake) * 1.2);
        canvas.fill(
            &path,
            &Paint::Solid([1.0, 1.0, 1.0, if night { 0.8 } else { 0.88 }]),
            Blend::Over,
        );
    }
}

fn draw_dragonfly(canvas: &mut Canvas, phase: f32) {
    let position = (
        200.0 + 150.0 * (TAU * phase).sin(),
        104.0 + 78.0 * (TAU * 2.0 * phase + 1.0).sin(),
    );
    let velocity = (
        150.0 * (TAU * phase).cos(),
        156.0 * (TAU * 2.0 * phase + 1.0).cos(),
    );
    let heading = velocity.1.atan2(velocity.0);
    let (sin_heading, cos_heading) = heading.sin_cos();
    let place = |origin: Point, local: Point| -> Point {
        (
            origin.0 + local.0 * cos_heading - local.1 * sin_heading,
            origin.1 + local.0 * sin_heading + local.1 * cos_heading,
        )
    };
    let shadow_origin = (position.0 + 7.0, position.1 + 11.0);
    let mut shadow = Path::new();
    shadow.ellipse(place(shadow_origin, (-3.0, 0.0)), 8.0, 1.2, heading);
    shadow.ellipse(shadow_origin, 9.0, 1.5, heading + PI / 2.0);
    canvas.fill(&shadow, &Paint::Solid([0.0, 0.1, 0.12, 0.16]), Blend::Over);
    let flutter = 0.8 + 0.2 * (TAU * 40.0 * phase).sin().abs();
    for side in [-1.0_f32, 1.0] {
        for (local, length, width, tilt) in [
            ((1.5, side * 5.6 * flutter), 5.6 * flutter, 1.7, side * 0.22),
            (
                (-1.8, side * 5.1 * flutter),
                5.1 * flutter,
                1.5,
                -side * 0.25,
            ),
        ] {
            let center = place(position, local);
            let mut wing = Path::new();
            wing.ellipse(center, width, length, heading + tilt);
            canvas.fill(&wing, &Paint::Solid([0.92, 0.97, 1.0, 0.5]), Blend::Over);
            let outline: Vec<Point> = (0..24)
                .map(|step| {
                    let theta = step as f32 / 24.0 * TAU;
                    let (x, y) = (width * theta.cos(), length * theta.sin());
                    let (sin_tilt, cos_tilt) = (heading + tilt).sin_cos();
                    (
                        center.0 + x * cos_tilt - y * sin_tilt,
                        center.1 + x * sin_tilt + y * cos_tilt,
                    )
                })
                .collect();
            canvas.stroke(
                &outline,
                0.4,
                true,
                &Paint::Solid([0.67, 0.8, 0.88, 0.8]),
                Blend::Over,
            );
        }
    }
    canvas.stroke(
        &[place(position, (2.0, 0.0)), place(position, (-12.0, 0.0))],
        1.5,
        false,
        &Paint::Solid(hex(0x1e7f8e)),
        Blend::Over,
    );
    let mut thorax = Path::new();
    thorax.ellipse(place(position, (0.5, 0.0)), 2.6, 1.7, heading);
    thorax.circle(place(position, (4.0, 0.0)), 1.8);
    canvas.fill(&thorax, &Paint::Solid(hex(0x16606c)), Blend::Over);
}

/// One caption item: plain words around an emphasized number or name.
struct CaptionItem {
    parts: Vec<(String, CaptionTone)>,
}

#[derive(Clone, Copy, PartialEq)]
enum CaptionTone {
    Plain,
    Streak,
    Lotus,
}

impl CaptionItem {
    fn plain_text(&self) -> String {
        self.parts.iter().map(|(text, _)| text.as_str()).collect()
    }
}

/// Streak, main language, lotus count, and pace, in the order they give way when the
/// caption is too wide: the pace goes first, then the streak, then the language.
fn build_caption_items(
    koi_development: &KoiDevelopment,
    behavior: Behavior,
    upstream_merged_pull_requests: Option<u64>,
) -> Vec<CaptionItem> {
    let mut items = Vec::new();
    match &koi_development.streaks.recent {
        Some(RecentStreak::Current(weeks)) => items.push(CaptionItem {
            parts: vec![
                (weeks.len().to_string(), CaptionTone::Streak),
                ("-week streak".to_owned(), CaptionTone::Plain),
            ],
        }),
        _ if !koi_development.streaks.longest.is_empty() => items.push(CaptionItem {
            parts: vec![
                ("best streak ".to_owned(), CaptionTone::Plain),
                (
                    koi_development.streaks.longest.len().to_string(),
                    CaptionTone::Streak,
                ),
                (
                    if koi_development.streaks.longest.len() == 1 {
                        " week"
                    } else {
                        " weeks"
                    }
                    .to_owned(),
                    CaptionTone::Plain,
                ),
            ],
        }),
        _ => {}
    }
    let main_language = koi_development.language_shares.iter().fold(
        None,
        |best: Option<(&String, f64)>, (language, &share)| match best {
            Some((_, best_share)) if best_share >= share => best,
            _ => Some((language, share)),
        },
    );
    if let Some((language, _)) = main_language {
        items.push(CaptionItem {
            parts: vec![
                ("mostly ".to_owned(), CaptionTone::Plain),
                (language.clone(), CaptionTone::Plain),
            ],
        });
    }
    if let Some(count) = upstream_merged_pull_requests.filter(|&count| count > 0) {
        items.push(CaptionItem {
            parts: vec![
                (count.to_string(), CaptionTone::Lotus),
                (
                    if count == 1 {
                        " PR merged upstream"
                    } else {
                        " PRs merged upstream"
                    }
                    .to_owned(),
                    CaptionTone::Plain,
                ),
            ],
        });
    }
    let pace = match behavior {
        Behavior::Cruise => "circling",
        Behavior::Dart => "darting",
        Behavior::SurfaceFeeding => "feeding",
        Behavior::Rest => "resting",
        Behavior::DeepRest => "resting deep",
    };
    items.push(CaptionItem {
        parts: vec![(pace.to_owned(), CaptionTone::Plain)],
    });
    let font = caption_font();
    let separator = " \u{b7} ";
    let measure = |items: &[CaptionItem]| -> f32 {
        let mut width =
            font.measure(separator, TextWeight::Regular) * items.len().saturating_sub(1) as f32;
        for item in items {
            for (text, tone) in &item.parts {
                width += font.measure(text, caption_weight(*tone));
            }
        }
        width
    };
    let max_width = (WIDTH - 2.0 * CAPTION_MARGIN) * SCALE;
    for drop_order in [
        CaptionDrop::Pace,
        CaptionDrop::Streak,
        CaptionDrop::Language,
    ] {
        if measure(&items) <= max_width {
            break;
        }
        let position = match drop_order {
            CaptionDrop::Pace => items.len().checked_sub(1),
            CaptionDrop::Streak => items.iter().position(|item| {
                item.parts
                    .iter()
                    .any(|(_, tone)| *tone == CaptionTone::Streak)
            }),
            CaptionDrop::Language => items.iter().position(|item| {
                item.parts
                    .first()
                    .is_some_and(|(text, _)| text == "mostly ")
            }),
        };
        if let Some(position) = position {
            items.remove(position);
        }
    }
    items
}

enum CaptionDrop {
    Pace,
    Streak,
    Language,
}

fn caption_weight(tone: CaptionTone) -> TextWeight {
    match tone {
        CaptionTone::Plain => TextWeight::Regular,
        CaptionTone::Streak | CaptionTone::Lotus => TextWeight::Medium,
    }
}

/// A vermilion seal at the pond's lower right, where a rakkan signs a painting, carved
/// with a futatsu-domoe: two commas circling each other like the koi. The day the pond
/// shows is inscribed beside it in small capitals, as a painting is dated next to its seal.
fn draw_seal(canvas: &mut Canvas, dated: &str, night: bool) {
    const SIZE: f32 = 16.0;
    const MARGIN: f32 = 8.0;
    let left = WIDTH - MARGIN - SIZE;
    let top = POND_BOTTOM - MARGIN - SIZE;
    let center = (left + SIZE / 2.0, top + SIZE / 2.0);
    let mut ground = Path::new();
    ground.round_rect(left, top, SIZE, SIZE, 2.5);
    canvas.fill(
        &ground,
        &Paint::Solid(hex(if night { 0xd8664f } else { 0xc8553d })),
        Blend::Over,
    );
    let carving = Paint::Solid(hex(if night { 0x0c1322 } else { 0xfbf8f1 }));
    for half in 0..2 {
        let start = half as f32 * TAU / 2.0;
        let mut head = Path::new();
        head.circle(add(center, (start.cos(), start.sin()), 2.75), 2.4);
        canvas.fill(&head, &carving, Blend::Over);
        // The tail spirals outward from the head and tapers to nothing over 170 degrees.
        let edge = |t: f32, side: f32| {
            let angle = start + t * 170f32.to_radians();
            let radius = 2.75 + 3.0 * t + side * 2.4 * (1.0 - t).powf(0.8);
            add(center, (angle.cos(), angle.sin()), radius)
        };
        let steps = 24;
        let mut points: Vec<Point> = (0..=steps)
            .map(|step| edge(step as f32 / steps as f32, 1.0))
            .collect();
        points.extend(
            (0..=steps)
                .rev()
                .map(|step| edge(step as f32 / steps as f32, -1.0)),
        );
        let mut tail = Path::new();
        tail.polygon(&points);
        canvas.fill(&tail, &carving, Blend::Over);
    }
    // The date as an engraver would set it: small tracked capitals in diluted ink, about
    // two thirds of the seal's height, so the seal stays the mark and the date a whisper.
    let font = inscription_font();
    let inscription = dated.to_uppercase();
    let tracking = 2.0;
    let letters: Vec<String> = inscription
        .chars()
        .map(|letter| letter.to_string())
        .collect();
    let text_width = letters
        .iter()
        .map(|letter| font.measure(letter, TextWeight::Regular))
        .sum::<f32>()
        + tracking * letters.len().saturating_sub(1) as f32;
    let ink = if night {
        with_alpha(hex(0xdfe6f5), 0.72)
    } else {
        with_alpha(hex(0xfbf8f1), 0.78)
    };
    let mut pen_x = (left - 6.0) * SCALE - text_width;
    let baseline = (center.1 + 3.2) * SCALE;
    for letter in &letters {
        // A trace of shadow keeps the ink legible when a koi passes beneath it.
        canvas.draw_text(
            font,
            letter,
            TextWeight::Regular,
            pen_x + 0.3 * SCALE,
            baseline + 0.3 * SCALE,
            [0.0, 0.0, 0.0, 0.12],
        );
        pen_x =
            canvas.draw_text(font, letter, TextWeight::Regular, pen_x, baseline, ink) + tracking;
    }
}
