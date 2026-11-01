use std::ffi::OsString;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};
use std::{env, fs};

use chrono::{DateTime, NaiveDate};
use git_koi::behavior::{Behavior, select_behavior};
use git_koi::coat::KoiBody;
use git_koi::collect::{GhCli, collect_account_snapshot};
use git_koi::history::AccountSnapshot;
use git_koi::koi::derive_koi_development;
use git_koi::motion::frame_count;
use git_koi::output::{
    encode_gif, encode_png, encode_pond_gif, encode_pond_png, encode_pond_preview_html,
    encode_preview_html,
};
use git_koi::pond::{
    POND_FRAME_COUNT, PondFrame, PondScene, PondTheme, Season, pond_frame_delay_centiseconds,
    render_pond_frame,
};
use git_koi::render::{KoiFrame, render_koi, render_swim_frame};

const USAGE: &str = "Usage: git-koi collect LOGIN [--output FILE]
       git-koi render SNAPSHOT --output-dir DIR [--style STYLE] [--season SEASON] [--body BODY]
       git-koi --help | --version
Collect LOGIN's public GitHub snapshot as JSON through `gh api`: public owned and
contributed repositories, merged pull requests to repositories others own, and daily
contributions from the account's creation through yesterday (UTC), the last complete day.
Writes to standard output, or replaces FILE only after collection succeeds.
Requires an authenticated GitHub CLI; in GitHub Actions, set GH_TOKEN to github.token.
Render a collected SNAPSHOT into DIR in one STYLE, both swimming the behavior chosen for
its observation date:
  pond (the default): two koi circling under lily pads, a lotus for each merged upstream
    pull request, and a seal dated with the morning it is drawn. koi-light.gif and koi-dark.gif are for GitHub's light
    and dark themes, koi-light.png and koi-dark.png are stills, and preview.html shows
    them. SEASON is auto (the default, from the observation date in the northern
    hemisphere), spring, summer, autumn, or winter.
  classic: one pixel-art koi. koi.gif swims, koi.png is the still koi, and preview.html
    shows both on light and dark backgrounds. BODY is white (the default), black,
    asagi, tea, gold, or platinum.
Options may come in any order, as --option VALUE or --option=VALUE.
The files are replaced only after all of them render.
Errors go to stderr and exit 1; invalid usage exits 2.
Examples: git-koi collect octocat --output snapshot.json
          git-koi render snapshot.json --output-dir koi
          git-koi render snapshot.json --output-dir koi --style classic --body asagi";

/// Each season's name on the command line; `auto` follows the observation date.
const SEASON_NAMES: [(&str, Option<Season>); 5] = [
    ("auto", None),
    ("spring", Some(Season::Spring)),
    ("summer", Some(Season::Summer)),
    ("autumn", Some(Season::Autumn)),
    ("winter", Some(Season::Winter)),
];

/// How the day is drawn.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RenderStyle {
    Pond,
    Classic,
}

/// Everything `render` was asked for.
struct RenderRequest {
    snapshot_path: PathBuf,
    output_directory: PathBuf,
    style: RenderStyle,
    season: Option<Season>,
    koi_body: KoiBody,
}

/// Each body's name on the command line.
const BODY_NAMES: [(&str, KoiBody); 6] = [
    ("white", KoiBody::White),
    ("black", KoiBody::Black),
    ("asagi", KoiBody::Asagi),
    ("tea", KoiBody::Tea),
    ("gold", KoiBody::Gold),
    ("platinum", KoiBody::Platinum),
];

fn main() -> ExitCode {
    let program_arguments: Vec<OsString> = env::args_os().skip(1).collect();
    let argument_texts: Vec<Option<&str>> = program_arguments
        .iter()
        .map(|argument| argument.to_str())
        .collect();
    match argument_texts.as_slice() {
        [Some("--help")] | [Some("collect" | "render"), Some("--help")] => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        [Some("--version")] => {
            println!("git-koi {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        [Some("collect"), Some(login)] if !login.starts_with('-') => run_collect(login, None),
        [Some("collect"), Some(login), Some("--output"), _] if !login.starts_with('-') => {
            run_collect(login, Some(Path::new(&program_arguments[3])))
        }
        [Some("render"), ..] => match parse_render_request(&program_arguments[1..]) {
            Some(render_request) => run_render(&render_request),
            None => {
                eprintln!("{USAGE}");
                ExitCode::from(2)
            }
        },
        _ => {
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}

fn run_collect(login: &str, output_path: Option<&Path>) -> ExitCode {
    let collection_result = collect_account_snapshot(&GhCli, login, current_utc_date())
        .map_err(|collection_error| collection_error.to_string())
        .and_then(|account_snapshot| {
            serde_json::to_string_pretty(&account_snapshot)
                .map_err(|serialize_error| serialize_error.to_string())
        })
        .and_then(|snapshot_json| write_snapshot(&format!("{snapshot_json}\n"), output_path));
    match collection_result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error_message) => {
            eprintln!("git-koi: {error_message}");
            ExitCode::FAILURE
        }
    }
}

/// Read `SNAPSHOT --output-dir DIR` and the optional style, season, and body, in any
/// order, each at most once, as `--option VALUE` or `--option=VALUE`. Anything else is
/// a usage error.
fn parse_render_request(render_arguments: &[OsString]) -> Option<RenderRequest> {
    let (snapshot_argument, option_arguments) = render_arguments.split_first()?;
    if snapshot_argument
        .to_str()
        .is_none_or(|snapshot_text| snapshot_text.starts_with('-'))
    {
        return None;
    }
    let mut output_directory: Option<PathBuf> = None;
    let mut style: Option<RenderStyle> = None;
    let mut season: Option<Option<Season>> = None;
    let mut koi_body: Option<KoiBody> = None;
    let mut remaining = option_arguments.iter();
    while let Some(option_argument) = remaining.next() {
        let option_text = option_argument.to_str()?;
        let (option_name, value) = match option_text.split_once('=') {
            Some((option_name, inline_value)) => (option_name, OsString::from(inline_value)),
            None => (option_text, remaining.next()?.clone()),
        };
        let value_text = value.to_str();
        match option_name {
            "--output-dir" if output_directory.is_none() && !value.is_empty() => {
                output_directory = Some(PathBuf::from(value));
            }
            "--style" if style.is_none() => {
                style = Some(match value_text? {
                    "pond" => RenderStyle::Pond,
                    "classic" => RenderStyle::Classic,
                    _ => return None,
                });
            }
            "--season" if season.is_none() => season = Some(parse_season(value_text?)?),
            "--body" if koi_body.is_none() => koi_body = Some(parse_body(value_text?)?),
            _ => return None,
        }
    }
    Some(RenderRequest {
        snapshot_path: PathBuf::from(snapshot_argument),
        output_directory: output_directory?,
        style: style.unwrap_or(RenderStyle::Pond),
        season: season.flatten(),
        koi_body: koi_body.unwrap_or_default(),
    })
}

fn run_render(render_request: &RenderRequest) -> ExitCode {
    let output_directory = render_request.output_directory.as_path();
    let render_result = render_day(render_request).and_then(|rendered_day| {
        fs::create_dir_all(output_directory)
            .map_err(|create_error| format!("{}: {create_error}", output_directory.display()))?;
        let mut written_paths = Vec::new();
        for (file_name, file_bytes) in rendered_day.output_files {
            let output_path = output_directory.join(file_name);
            replace_file(&output_path, &file_bytes)?;
            written_paths.push(output_path.display().to_string());
        }
        println!(
            "{}: wrote {}",
            rendered_day.caption,
            written_paths.join(", ")
        );
        Ok(())
    });
    match render_result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error_message) => {
            eprintln!("git-koi: {error_message}");
            ExitCode::FAILURE
        }
    }
}

/// The day's files, by name, with a caption naming the behavior, observation date, and
/// body or season.
struct RenderedDay {
    caption: String,
    output_files: Vec<(&'static str, Vec<u8>)>,
}

/// Render and encode the day's files in memory, so none is written unless all succeed.
fn render_day(render_request: &RenderRequest) -> Result<RenderedDay, String> {
    let snapshot_path = render_request.snapshot_path.as_path();
    let snapshot_json = fs::read_to_string(snapshot_path)
        .map_err(|read_error| format!("{}: {read_error}", snapshot_path.display()))?;
    let account_snapshot: AccountSnapshot = serde_json::from_str(&snapshot_json)
        .map_err(|parse_error| format!("{}: {parse_error}", snapshot_path.display()))?;
    let koi_development = derive_koi_development(&account_snapshot)
        .map_err(|development_error| development_error.to_string())?;
    let behavior = select_behavior(&koi_development);
    if render_request.style == RenderStyle::Pond {
        return render_pond_day(
            &account_snapshot,
            &koi_development,
            behavior,
            render_request.season,
        );
    }
    let koi_body = render_request.koi_body;
    let swim_frames: Vec<KoiFrame> = (0..frame_count(behavior))
        .map(|frame_index| render_swim_frame(&koi_development, koi_body, behavior, frame_index))
        .collect();
    let gif_bytes =
        encode_gif(&swim_frames).map_err(|encoding_error| encoding_error.to_string())?;
    let png_bytes = encode_png(&render_koi(&koi_development, koi_body))
        .map_err(|encoding_error| encoding_error.to_string())?;
    let caption = format!(
        "{} for {}, {} body",
        describe_behavior(behavior),
        koi_development.observed_on,
        name_body(koi_body)
    );
    let preview_html = encode_preview_html(&caption);
    Ok(RenderedDay {
        caption,
        output_files: vec![
            ("koi.gif", gif_bytes),
            ("koi.png", png_bytes),
            ("preview.html", preview_html.into_bytes()),
        ],
    })
}

/// Render the light and dark ponds side by side, each on its own thread.
fn render_pond_day(
    account_snapshot: &AccountSnapshot,
    koi_development: &git_koi::koi::KoiDevelopment,
    behavior: Behavior,
    requested_season: Option<Season>,
) -> Result<RenderedDay, String> {
    let season = requested_season.unwrap_or_else(|| Season::of_date(koi_development.observed_on));
    let delay_centiseconds = pond_frame_delay_centiseconds(behavior);
    let render_theme = |pond_theme: PondTheme| -> Result<(String, Vec<u8>, Vec<u8>), String> {
        let pond_scene = PondScene::new(
            koi_development,
            behavior,
            account_snapshot.upstream_merged_pull_requests,
            season,
            pond_theme,
        );
        let pond_frames: Vec<PondFrame> = (0..POND_FRAME_COUNT)
            .map(|frame_index| render_pond_frame(&pond_scene, frame_index))
            .collect();
        let gif_bytes = encode_pond_gif(&pond_frames, delay_centiseconds)
            .map_err(|encoding_error| encoding_error.to_string())?;
        let png_bytes = encode_pond_png(&pond_frames[0])
            .map_err(|encoding_error| encoding_error.to_string())?;
        Ok((pond_scene.caption().to_owned(), gif_bytes, png_bytes))
    };
    let (light_result, dark_result) = thread::scope(|render_scope| {
        let light_render = render_scope.spawn(|| render_theme(PondTheme::Light));
        let dark_result = render_theme(PondTheme::Dark);
        (
            light_render
                .join()
                .expect("the light pond renders without panicking"),
            dark_result,
        )
    });
    let (pond_caption, light_gif, light_png) = light_result?;
    let (_, dark_gif, dark_png) = dark_result?;
    let caption = format!(
        "{} for {}, {} pond: {pond_caption}",
        describe_behavior(behavior),
        koi_development.observed_on,
        season.name()
    );
    let preview_html = encode_pond_preview_html(&caption);
    Ok(RenderedDay {
        caption,
        output_files: vec![
            ("koi-light.gif", light_gif),
            ("koi-dark.gif", dark_gif),
            ("koi-light.png", light_png),
            ("koi-dark.png", dark_png),
            ("preview.html", preview_html.into_bytes()),
        ],
    })
}

fn parse_season(season_name: &str) -> Option<Option<Season>> {
    SEASON_NAMES
        .iter()
        .find(|(named_season_name, _)| *named_season_name == season_name)
        .map(|(_, season)| *season)
}

fn parse_body(body_name: &str) -> Option<KoiBody> {
    BODY_NAMES
        .iter()
        .find(|(named_body_name, _)| *named_body_name == body_name)
        .map(|(_, koi_body)| *koi_body)
}

fn name_body(koi_body: KoiBody) -> &'static str {
    BODY_NAMES
        .iter()
        .find(|(_, named_body)| *named_body == koi_body)
        .map(|(body_name, _)| *body_name)
        .expect("every body has a name")
}

fn describe_behavior(behavior: Behavior) -> &'static str {
    match behavior {
        Behavior::DeepRest => "deep rest",
        Behavior::Rest => "rest",
        Behavior::SurfaceFeeding => "surface feeding",
        Behavior::Dart => "dart",
        Behavior::Cruise => "cruise",
    }
}

// GitHub dates its contribution calendar in UTC.
fn current_utc_date() -> NaiveDate {
    let seconds_since_epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("the system clock is after 1970")
        .as_secs();
    DateTime::from_timestamp(
        i64::try_from(seconds_since_epoch).expect("the system clock is in range"),
        0,
    )
    .expect("the system clock is in range")
    .date_naive()
}

/// Write to standard output, or replace the file.
fn write_snapshot(snapshot_json: &str, output_path: Option<&Path>) -> Result<(), String> {
    let Some(output_path) = output_path else {
        return io::stdout()
            .write_all(snapshot_json.as_bytes())
            .map_err(|write_error| format!("standard output: {write_error}"));
    };
    replace_file(output_path, snapshot_json.as_bytes())
}

/// Replace the file through a sibling temporary file named after it, so a failed write
/// never leaves a partial file in place.
fn replace_file(output_path: &Path, file_bytes: &[u8]) -> Result<(), String> {
    let mut temporary_name = output_path.file_name().unwrap_or_default().to_owned();
    temporary_name.push(".tmp");
    let temporary_path = output_path.with_file_name(temporary_name);
    fs::write(&temporary_path, file_bytes)
        .and_then(|()| fs::rename(&temporary_path, output_path))
        .map_err(|write_error| {
            let _ = fs::remove_file(&temporary_path);
            format!("{}: {write_error}", output_path.display())
        })
}
