use std::borrow::Cow;

use chrono::{Days, NaiveDate};
use git_koi::behavior::{Behavior, select_behavior};
use git_koi::history::{AccountSnapshot, ContributionDay, LanguageColor};
use git_koi::koi::{KoiDevelopment, derive_koi_development};
use git_koi::output::{GifEncodingError, encode_pond_gif, encode_pond_png};
use git_koi::pond::{
    MAX_LOTUS_COUNT, POND_FRAME_COUNT, POND_HEIGHT_PIXELS, POND_WIDTH_PIXELS, PondFrame, PondScene,
    PondTheme, Season, pond_frame_delay_centiseconds, render_pond_frame,
};

const HISTORY_FIXTURE: &str = include_str!("fixtures/account-with-history.json");
/// The caption strip starts this many rows down.
const POND_BOTTOM_ROW: usize = 416;

#[test]
fn seasons_follow_the_northern_hemisphere_calendar() {
    for (month, season) in [
        (1, Season::Winter),
        (2, Season::Winter),
        (3, Season::Spring),
        (5, Season::Spring),
        (6, Season::Summer),
        (8, Season::Summer),
        (9, Season::Autumn),
        (11, Season::Autumn),
        (12, Season::Winter),
    ] {
        let date = NaiveDate::from_ymd_opt(2026, month, 15).unwrap();
        assert_eq!(Season::of_date(date), season, "{date}");
    }
}

#[test]
fn frames_fill_the_pond_with_transparent_corners() {
    let pond_frame =
        render_pond_frame(&fixture_scene(Some(4), Season::Autumn, PondTheme::Light), 0);
    assert_eq!(
        (pond_frame.width, pond_frame.height),
        (POND_WIDTH_PIXELS, POND_HEIGHT_PIXELS)
    );
    assert_eq!(
        pond_frame.rgba.len(),
        (POND_WIDTH_PIXELS * POND_HEIGHT_PIXELS * 4) as usize
    );
    let alpha_at = |x: u32, y: u32| pond_frame.rgba[((y * POND_WIDTH_PIXELS + x) * 4 + 3) as usize];
    for (x, y) in [
        (0, 0),
        (POND_WIDTH_PIXELS - 1, 0),
        (0, POND_HEIGHT_PIXELS - 1),
        (POND_WIDTH_PIXELS - 1, POND_HEIGHT_PIXELS - 1),
    ] {
        assert_eq!(alpha_at(x, y), 0, "corner {x},{y}");
    }
    for (x, y) in [(400, 200), (40, 40), (400, 460)] {
        assert_eq!(alpha_at(x, y), 255, "inside {x},{y}");
    }
}

#[test]
fn rendering_is_deterministic_and_frame_indices_wrap() {
    let pond_scene = fixture_scene(Some(2), Season::Spring, PondTheme::Dark);
    assert_eq!(
        render_pond_frame(&pond_scene, 7),
        render_pond_frame(&pond_scene, 7)
    );
    assert_eq!(
        render_pond_frame(&pond_scene, POND_FRAME_COUNT + 7),
        render_pond_frame(&pond_scene, 7)
    );
}

#[test]
fn the_loop_has_no_jump_from_its_last_frame_to_its_first() {
    for season in [
        Season::Spring,
        Season::Summer,
        Season::Autumn,
        Season::Winter,
    ] {
        let pond_scene = fixture_scene(Some(3), season, PondTheme::Light);
        let frame = |frame_index| render_pond_frame(&pond_scene, frame_index);
        let (first, second, last) = (frame(0), frame(1), frame(POND_FRAME_COUNT - 1));
        let ordinary_step = mean_difference(&first, &second);
        let wrap_step = mean_difference(&last, &first);
        assert!(
            wrap_step <= ordinary_step * 1.5 + 0.05,
            "{season:?}: wrap {wrap_step}, step {ordinary_step}"
        );
    }
}

#[test]
fn lotus_beyond_the_cap_change_only_the_caption() {
    let at_cap = render_pond_frame(
        &fixture_scene(Some(MAX_LOTUS_COUNT), Season::Summer, PondTheme::Light),
        0,
    );
    let beyond_cap = render_pond_frame(
        &fixture_scene(Some(MAX_LOTUS_COUNT + 6), Season::Summer, PondTheme::Light),
        0,
    );
    assert_eq!(at_cap.rgba, beyond_cap.rgba);
    assert_ne!(
        fixture_scene(Some(MAX_LOTUS_COUNT), Season::Summer, PondTheme::Light).caption(),
        fixture_scene(Some(MAX_LOTUS_COUNT + 6), Season::Summer, PondTheme::Light).caption()
    );
    let without_lotus =
        render_pond_frame(&fixture_scene(Some(0), Season::Summer, PondTheme::Light), 0);
    let one_lotus = render_pond_frame(&fixture_scene(Some(1), Season::Summer, PondTheme::Light), 0);
    assert_ne!(without_lotus.rgba, one_lotus.rgba);
}

#[test]
fn the_caption_names_the_streak_language_lotus_and_pace() {
    assert_eq!(
        fixture_scene(Some(4), Season::Autumn, PondTheme::Light).caption(),
        "1-week streak \u{b7} mostly Rust \u{b7} 4 PRs merged upstream \u{b7} circling"
    );
    assert_eq!(
        fixture_scene(Some(1), Season::Autumn, PondTheme::Light).caption(),
        "1-week streak \u{b7} mostly Rust \u{b7} 1 PR merged upstream \u{b7} circling"
    );
    assert_eq!(
        fixture_scene(None, Season::Autumn, PondTheme::Light).caption(),
        "1-week streak \u{b7} mostly Rust \u{b7} circling"
    );
}

#[test]
fn a_caption_too_wide_for_the_strip_gives_up_the_pace_first() {
    let mut account_snapshot: AccountSnapshot = serde_json::from_str(HISTORY_FIXTURE).unwrap();
    account_snapshot.repositories[0].primary_language =
        Some("Jupyter Notebook With A Long Name".to_owned());
    account_snapshot.language_colors = Some(
        [(
            "Jupyter Notebook With A Long Name".to_owned(),
            Some(LanguageColor([0xda, 0x5b, 0x0b])),
        )]
        .into(),
    );
    let contribution_history = account_snapshot.contribution_history.as_mut().unwrap();
    for day_offset in 1..=70 {
        contribution_history.days.push(ContributionDay {
            date: contribution_history.observed_on + Days::new(day_offset),
            contribution_count: 0,
        });
    }
    contribution_history.observed_on = contribution_history.days.last().unwrap().date;
    let koi_development = derive_koi_development(&account_snapshot).unwrap();
    let behavior = select_behavior(&koi_development);
    assert_eq!(behavior, Behavior::DeepRest);
    let caption = PondScene::new(
        &koi_development,
        behavior,
        Some(12),
        Season::Winter,
        PondTheme::Dark,
    )
    .caption()
    .to_owned();
    assert!(!caption.contains("resting deep"), "{caption}");
    assert!(caption.contains("12 PRs merged upstream"), "{caption}");
}

#[test]
fn the_dark_pond_is_darker_than_the_light_one() {
    let mean_lightness = |pond_frame: &PondFrame| -> f64 {
        let pond_pixels = &pond_frame.rgba
            [POND_WIDTH_PIXELS as usize * 4 * 40..POND_BOTTOM_ROW * POND_WIDTH_PIXELS as usize * 4];
        pond_pixels
            .chunks(4)
            .map(|pixel| f64::from(pixel[0]) + f64::from(pixel[1]) + f64::from(pixel[2]))
            .sum::<f64>()
            / pond_pixels.len() as f64
    };
    for season in [Season::Spring, Season::Winter] {
        let light = render_pond_frame(&fixture_scene(Some(3), season, PondTheme::Light), 0);
        let dark = render_pond_frame(&fixture_scene(Some(3), season, PondTheme::Dark), 0);
        assert!(
            mean_lightness(&dark) < mean_lightness(&light) * 0.6,
            "{season:?}"
        );
    }
}

#[test]
fn slower_behaviors_show_each_frame_longer() {
    assert!(
        pond_frame_delay_centiseconds(Behavior::Cruise)
            < pond_frame_delay_centiseconds(Behavior::Rest)
    );
    assert!(
        pond_frame_delay_centiseconds(Behavior::Rest)
            < pond_frame_delay_centiseconds(Behavior::DeepRest)
    );
    assert_eq!(
        pond_frame_delay_centiseconds(Behavior::Dart),
        pond_frame_delay_centiseconds(Behavior::Cruise)
    );
}

#[test]
fn the_pond_gif_shows_every_frame_close_to_its_render() {
    let pond_scene = fixture_scene(Some(4), Season::Winter, PondTheme::Dark);
    let pond_frames: Vec<PondFrame> = [0, 1, 2, 60, 61]
        .into_iter()
        .map(|frame_index| render_pond_frame(&pond_scene, frame_index))
        .collect();
    let gif_bytes = encode_pond_gif(&pond_frames, 8).unwrap();
    let decoded_frames = decode_composited(&gif_bytes);
    assert_eq!(decoded_frames.len(), pond_frames.len());
    for (frame_index, (decoded, rendered)) in decoded_frames.iter().zip(&pond_frames).enumerate() {
        let mut total_error = 0.0;
        let mut opaque_pixels = 0.0;
        for (decoded_pixel, rendered_pixel) in decoded.chunks(4).zip(rendered.rgba.chunks(4)) {
            assert_eq!(
                decoded_pixel[3] == 255,
                rendered_pixel[3] >= 128,
                "frame {frame_index} transparency"
            );
            if decoded_pixel[3] == 255 {
                total_error += (0..3)
                    .map(|channel| {
                        f64::from(decoded_pixel[channel].abs_diff(rendered_pixel[channel]))
                    })
                    .sum::<f64>();
                opaque_pixels += 3.0;
            }
        }
        let mean_error = total_error / opaque_pixels;
        assert!(
            mean_error < 5.0,
            "frame {frame_index} mean error {mean_error}"
        );
    }
}

#[test]
fn the_pond_gif_loops_forever_and_stores_only_changes_after_the_first_frame() {
    let pond_scene = fixture_scene(Some(4), Season::Spring, PondTheme::Light);
    let pond_frames: Vec<PondFrame> = (0..3)
        .map(|frame_index| render_pond_frame(&pond_scene, frame_index))
        .collect();
    let gif_bytes = encode_pond_gif(&pond_frames, 12).unwrap();
    let mut decode_options = gif::DecodeOptions::new();
    decode_options.set_color_output(gif::ColorOutput::Indexed);
    let mut gif_decoder = decode_options.read_info(gif_bytes.as_slice()).unwrap();
    assert_eq!(gif_decoder.repeat(), gif::Repeat::Infinite);
    let mut frame_areas = Vec::new();
    while let Some(frame) = gif_decoder.read_next_frame().unwrap() {
        assert_eq!(frame.delay, 12);
        assert_eq!(frame.dispose, gif::DisposalMethod::Keep);
        frame_areas.push(u32::from(frame.width) * u32::from(frame.height));
    }
    assert_eq!(frame_areas[0], POND_WIDTH_PIXELS * POND_HEIGHT_PIXELS);
    assert!(frame_areas[1..].iter().all(|&area| area < frame_areas[0]));
    assert_eq!(encode_pond_gif(&pond_frames, 12).unwrap(), gif_bytes);
}

#[test]
fn the_pond_gif_refuses_empty_and_wrongly_sized_frames() {
    assert!(matches!(
        encode_pond_gif(&[], 8),
        Err(GifEncodingError::NoFrames)
    ));
    let wrong_size = PondFrame {
        width: 2,
        height: 2,
        rgba: vec![0; 16],
    };
    assert!(matches!(
        encode_pond_gif(&[wrong_size], 8),
        Err(GifEncodingError::WrongFrameSize { frame_index: 0 })
    ));
}

#[test]
fn the_pond_png_keeps_the_frame_exactly() {
    let pond_frame = render_pond_frame(
        &fixture_scene(Some(4), Season::Summer, PondTheme::Light),
        30,
    );
    let png_bytes = encode_pond_png(&pond_frame).unwrap();
    let png_decoder = png::Decoder::new(std::io::Cursor::new(png_bytes));
    let mut png_reader = png_decoder.read_info().unwrap();
    let mut decoded = vec![0; png_reader.output_buffer_size().unwrap()];
    png_reader.next_frame(&mut decoded).unwrap();
    assert_eq!(decoded, pond_frame.rgba);
}

fn fixture_development() -> KoiDevelopment {
    derive_koi_development(&serde_json::from_str(HISTORY_FIXTURE).unwrap()).unwrap()
}

fn fixture_scene(
    upstream_merged_pull_requests: Option<u64>,
    season: Season,
    pond_theme: PondTheme,
) -> PondScene {
    let koi_development = fixture_development();
    let behavior = select_behavior(&koi_development);
    PondScene::new(
        &koi_development,
        behavior,
        upstream_merged_pull_requests,
        season,
        pond_theme,
    )
}

/// Mean absolute channel difference between two frames, out of 255.
fn mean_difference(left: &PondFrame, right: &PondFrame) -> f64 {
    let total: u64 = left
        .rgba
        .iter()
        .zip(&right.rgba)
        .map(|(left, right)| u64::from(left.abs_diff(*right)))
        .sum();
    total as f64 / left.rgba.len() as f64
}

/// Decode a GIF into full RGBA frames, keeping each frame under the next as the GIF says.
fn decode_composited(gif_bytes: &[u8]) -> Vec<Vec<u8>> {
    let mut decode_options = gif::DecodeOptions::new();
    decode_options.set_color_output(gif::ColorOutput::Indexed);
    let mut gif_decoder = decode_options.read_info(gif_bytes).unwrap();
    let global_palette = gif_decoder.global_palette().unwrap().to_vec();
    let (width, height) = (gif_decoder.width() as usize, gif_decoder.height() as usize);
    let mut canvas = vec![0_u8; width * height * 4];
    let mut composited = Vec::new();
    while let Some(frame) = gif_decoder.read_next_frame().unwrap() {
        let palette: Cow<[u8]> = frame
            .palette
            .as_deref()
            .map_or(Cow::Borrowed(global_palette.as_slice()), Cow::Borrowed);
        for row in 0..usize::from(frame.height) {
            for column in 0..usize::from(frame.width) {
                let palette_index = frame.buffer[row * usize::from(frame.width) + column];
                if Some(palette_index) == frame.transparent {
                    continue;
                }
                let (x, y) = (
                    usize::from(frame.left) + column,
                    usize::from(frame.top) + row,
                );
                let pixel = &mut canvas[(y * width + x) * 4..(y * width + x) * 4 + 4];
                pixel[..3].copy_from_slice(
                    &palette[usize::from(palette_index) * 3..usize::from(palette_index) * 3 + 3],
                );
                pixel[3] = 255;
            }
        }
        composited.push(canvas.clone());
        assert_eq!(frame.dispose, gif::DisposalMethod::Keep);
    }
    let _ = height;
    composited
}
