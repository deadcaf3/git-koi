use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use chrono::Days;
use git_koi::behavior::{Behavior, select_behavior};
use git_koi::coat::KoiBody;
use git_koi::history::{AccountSnapshot, ContributionDay};
use git_koi::koi::{KoiDevelopment, derive_koi_development};
use git_koi::motion::frame_count;
use git_koi::output::{encode_gif, encode_png, encode_pond_png};
use git_koi::pond::{PondScene, PondTheme, Season, render_pond_frame};
use git_koi::render::{KoiFrame, render_koi, render_swim_frame};

const HISTORY_FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/account-with-history.json"
);

#[test]
fn render_draws_the_pond_by_default_for_both_themes() {
    let mut account_snapshot = read_fixture_snapshot();
    account_snapshot.upstream_merged_pull_requests = Some(3);
    let scratch_directory = fresh_directory("render-pond");
    let snapshot_path = scratch_directory.join("snapshot.json");
    fs::write(
        &snapshot_path,
        serde_json::to_string(&account_snapshot).unwrap(),
    )
    .unwrap();
    let output_directory = scratch_directory.join("koi");
    let command_output = run_git_koi(&[
        "render",
        snapshot_path.to_str().unwrap(),
        "--season=summer",
        "--output-dir",
        output_directory.to_str().unwrap(),
    ]);
    assert_eq!(
        command_output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&command_output.stderr)
    );
    let mut written: Vec<String> = fs::read_dir(&output_directory)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    written.sort();
    assert_eq!(
        written,
        [
            "koi-dark.gif",
            "koi-dark.png",
            "koi-light.gif",
            "koi-light.png",
            "preview.html"
        ]
    );
    let koi_development = read_fixture_development();
    let behavior = select_behavior(&koi_development);
    for (pond_theme, theme_name) in [(PondTheme::Light, "light"), (PondTheme::Dark, "dark")] {
        let pond_scene = PondScene::new(
            &koi_development,
            behavior,
            Some(3),
            Season::Summer,
            pond_theme,
        );
        assert_eq!(
            fs::read(output_directory.join(format!("koi-{theme_name}.png"))).unwrap(),
            encode_pond_png(&render_pond_frame(&pond_scene, 0)).unwrap()
        );
        let gif_bytes = fs::read(output_directory.join(format!("koi-{theme_name}.gif"))).unwrap();
        assert!(gif_bytes.starts_with(b"GIF89a"));
        assert!(
            gif_bytes.len() < 6_000_000,
            "{theme_name} GIF is {} bytes",
            gif_bytes.len()
        );
    }
    let preview_html = fs::read_to_string(output_directory.join("preview.html")).unwrap();
    assert!(preview_html.contains(r#"src="koi-light.gif""#));
    assert!(preview_html.contains(r#"src="koi-dark.gif""#));
    assert!(!preview_html.contains("<script"));
    let summary_line = String::from_utf8_lossy(&command_output.stdout);
    assert!(summary_line.contains("summer pond"));
    assert!(summary_line.contains("3 PRs merged upstream"));
    assert!(summary_line.contains(&koi_development.observed_on.to_string()));
}

#[test]
fn render_writes_the_days_loop_still_and_preview_as_the_renderer_draws_them() {
    // Two quiet weeks make the koi rest, so the loop can't pass as the fallback cruise.
    let mut account_snapshot = read_fixture_snapshot();
    let contribution_history = account_snapshot.contribution_history.as_mut().unwrap();
    for day_offset in 1..=14 {
        contribution_history.days.push(ContributionDay {
            date: contribution_history.observed_on + Days::new(day_offset),
            contribution_count: 0,
        });
    }
    contribution_history.observed_on = contribution_history.days.last().unwrap().date;
    let scratch_directory = fresh_directory("render-default");
    let snapshot_path = scratch_directory.join("snapshot.json");
    fs::write(
        &snapshot_path,
        serde_json::to_string(&account_snapshot).unwrap(),
    )
    .unwrap();
    let output_directory = scratch_directory.join("koi");
    let command_output = run_git_koi(&[
        "render",
        snapshot_path.to_str().unwrap(),
        "--output-dir",
        output_directory.to_str().unwrap(),
        "--style",
        "classic",
    ]);
    assert_eq!(
        command_output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&command_output.stderr)
    );

    let koi_development = derive_koi_development(&account_snapshot).unwrap();
    let behavior = select_behavior(&koi_development);
    assert_eq!(behavior, Behavior::Rest);
    let swim_frames: Vec<KoiFrame> = (0..frame_count(behavior))
        .map(|frame_index| {
            render_swim_frame(&koi_development, KoiBody::White, behavior, frame_index)
        })
        .collect();
    assert_eq!(
        fs::read(output_directory.join("koi.gif")).unwrap(),
        encode_gif(&swim_frames).unwrap()
    );
    assert_eq!(
        fs::read(output_directory.join("koi.png")).unwrap(),
        encode_png(&render_koi(&koi_development, KoiBody::White)).unwrap()
    );
    let preview_html = fs::read_to_string(output_directory.join("preview.html")).unwrap();
    assert!(preview_html.contains(r#"src="koi.gif""#));
    assert!(preview_html.contains(r#"src="koi.png""#));
    assert!(!preview_html.contains("<script"));
    let summary_line = String::from_utf8_lossy(&command_output.stdout);
    let observation_date = koi_development.observed_on.to_string();
    assert!(summary_line.contains(&observation_date));
    assert!(preview_html.contains(&observation_date));
}

#[test]
fn the_body_option_redraws_the_koi_in_that_body() {
    let output_directory = fresh_directory("render-black");
    let command_output = run_git_koi(&[
        "render",
        HISTORY_FIXTURE,
        "--body",
        "black",
        "--style=classic",
        "--output-dir",
        output_directory.to_str().unwrap(),
    ]);
    assert_eq!(command_output.status.code(), Some(0));
    let koi_development = read_fixture_development();
    let black_png = encode_png(&render_koi(&koi_development, KoiBody::Black)).unwrap();
    assert_ne!(
        black_png,
        encode_png(&render_koi(&koi_development, KoiBody::White)).unwrap()
    );
    assert_eq!(
        fs::read(output_directory.join("koi.png")).unwrap(),
        black_png
    );
    assert!(String::from_utf8_lossy(&command_output.stdout).contains("black body"));
}

#[test]
fn a_failed_render_exits_one_and_keeps_the_previous_files() {
    let output_directory = fresh_directory("render-failure");
    for file_name in ["koi.gif", "koi.png", "preview.html"] {
        fs::write(
            output_directory.join(file_name),
            format!("previous {file_name}"),
        )
        .unwrap();
    }
    let command_output = run_git_koi(&[
        "render",
        concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/account.json"),
        "--output-dir",
        output_directory.to_str().unwrap(),
    ]);
    assert_eq!(command_output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&command_output.stderr).starts_with("git-koi: "));
    for file_name in ["koi.gif", "koi.png", "preview.html"] {
        assert_eq!(
            fs::read_to_string(output_directory.join(file_name)).unwrap(),
            format!("previous {file_name}")
        );
    }
    assert_eq!(fs::read_dir(&output_directory).unwrap().count(), 3);
}

#[test]
fn render_usage_errors_exit_two_and_help_exits_zero() {
    for invalid_arguments in [
        &["render", HISTORY_FIXTURE][..],
        &["render", "--output-dir", "koi"],
        &[
            "render",
            HISTORY_FIXTURE,
            "--output-dir",
            "koi",
            "--body",
            "red",
        ],
        &[
            "render",
            HISTORY_FIXTURE,
            "--output-dir",
            "koi",
            "--style",
            "fancy",
        ],
        &[
            "render",
            HISTORY_FIXTURE,
            "--output-dir",
            "koi",
            "--season",
            "monsoon",
        ],
        &[
            "render",
            HISTORY_FIXTURE,
            "--output-dir",
            "koi",
            "--output-dir",
            "fish",
        ],
        &[
            "render",
            HISTORY_FIXTURE,
            "--output-dir=",
            "--style",
            "pond",
        ],
        &["render", HISTORY_FIXTURE, "--season", "winter"],
        &["render", HISTORY_FIXTURE, "--output-dir", "koi", "--style"],
    ] {
        let command_output = run_git_koi(invalid_arguments);
        assert_eq!(
            command_output.status.code(),
            Some(2),
            "{invalid_arguments:?}"
        );
        assert!(String::from_utf8_lossy(&command_output.stderr).starts_with("Usage:"));
    }
    let help_output = run_git_koi(&["render", "--help"]);
    assert_eq!(help_output.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&help_output.stdout).contains("platinum"));
}

#[test]
fn version_prints_the_manifest_version_and_exits_zero() {
    let version_output = run_git_koi(&["--version"]);
    assert_eq!(version_output.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&version_output.stdout),
        format!("git-koi {}\n", env!("CARGO_PKG_VERSION"))
    );
}

fn read_fixture_snapshot() -> AccountSnapshot {
    serde_json::from_str(&fs::read_to_string(HISTORY_FIXTURE).unwrap()).unwrap()
}

fn read_fixture_development() -> KoiDevelopment {
    derive_koi_development(&read_fixture_snapshot()).unwrap()
}

/// An empty directory of the given name under Cargo's scratch space for tests.
fn fresh_directory(directory_name: &str) -> PathBuf {
    let directory_path = Path::new(env!("CARGO_TARGET_TMPDIR")).join(directory_name);
    let _ = fs::remove_dir_all(&directory_path);
    fs::create_dir_all(&directory_path).unwrap();
    directory_path
}

fn run_git_koi(arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_git-koi"))
        .args(arguments)
        .output()
        .unwrap()
}
