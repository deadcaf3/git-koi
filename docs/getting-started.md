# Getting Started

Three ways in: a koi on your profile README, the command line, and the library.
Each stands alone.

## On Your Profile README

GitHub shows the README of the repository named after your username, such as
`octocat/octocat`, on your profile page. To give it a pond that redraws every
night:

1. Fork this repository, so the Action runs from your copy.
2. Copy [examples/profile-workflow.yml](../examples/profile-workflow.yml) into
   your profile repository as `.github/workflows/koi.yml`, and replace
   `YOUR-USERNAME` with the account that holds the fork.
3. Embed the pond in its `README.md`, so GitHub shows the dark pond to readers
   using a dark theme:

   ```html
   <picture>
     <source media="(prefers-color-scheme: dark)" srcset="koi/koi-dark.gif" />
     <img src="koi/koi-light.gif" width="500" alt="Two koi circling a lily pond" />
   </picture>
   ```

   The GIFs are drawn at 1000 by 520, two and a half times the pond's 400 by 208
   layout, so `width="500"` stays sharp on high-density screens.

4. Run the workflow once from the repository's Actions tab, so the first pond
   appears without waiting for midnight.

The workflow runs just after midnight UTC, so each pond shows the day that just
ended, and again whenever the workflow file changes. Its `git-koi` step is this
repository's Action, which builds git-koi from its own source, collects your
public activity with the workflow's built-in token, and renders `koi-light.gif`,
`koi-dark.gif`, two stills, and `preview.html` into `koi/`. The Action never
commits. The workflow's last step commits the two GIFs only when they changed,
so an unchanged pond adds no commit.

| Input        | Default              | Meaning                                                                   |
| ------------ | -------------------- | ------------------------------------------------------------------------- |
| `login`      | The repository owner | Account whose koi to draw                                                 |
| `style`      | `pond`               | `pond`, or `classic` for one pixel-art koi                                |
| `season`     | `auto`               | `auto`, `spring`, `summer`, `autumn`, or `winter`                         |
| `body`       | `white`              | The classic koi's `white`, `black`, `asagi`, `tea`, `gold`, or `platinum` |
| `output-dir` | `koi`                | Folder that receives the files                                            |
| `token`      | The built-in token   | Token for reading the account's public data                               |

The pond's pair takes its colors from your languages: the white koi carries the
top one as plates and the second as spots, and the black koi carries white
plates with your first colored language as accents. A lotus blooms for each pull
request merged into a public repository someone else owns, up to nine; the
caption counts them all. `auto` picks the season from the observation date in
the northern hemisphere, and the day/night cycle follows the reader's GitHub
theme rather than any clock, so the pond does not reveal your time zone.

For the classic koi, set `style: classic` and commit `koi/koi.gif` instead, and
embed it with `<img src="koi/koi.gif" width="256" alt="My koi" />`; that GIF is
drawn at exactly twice the koi's pixel size. If you change `output-dir`, change
the commit step's paths and the README images to match. Pin the Action to a full
commit SHA of your fork to choose when you update.

GitHub pauses scheduled workflows in a public repository after 60 days without
activity such as a push. A koi that stops changing during a long quiet stretch
adds no commits, so its schedule can pause and does not resume on its own.
Re-enable it from the Actions tab or with `gh workflow enable koi.yml`.

## From the Command Line

Install one of two ways. With Rust at or above the `rust-version` in
[Cargo.toml](../Cargo.toml):

```sh
cargo install git-koi --locked
```

Or download the archive for your platform from the
[releases page](https://github.com/yalongwastaken/git-koi/releases), a `.tar.gz`
for Linux and macOS or a `.zip` for Windows. Each archive has a `.sha256` file
beside it; check the download with `shasum -a 256 -c` on macOS, `sha256sum -c`
on Linux, or `certutil -hashfile` on Windows, then put `git-koi` on your `PATH`.
macOS may refuse to open a binary it downloaded;
`xattr -d com.apple.quarantine git-koi` clears that flag.

Collection goes through the [GitHub CLI](https://cli.github.com), so install it
and sign in once with `gh auth login`. Then:

```sh
git-koi collect octocat --output snapshot.json
git-koi render snapshot.json --output-dir koi
git-koi render snapshot.json --output-dir classic --style classic --body asagi
```

Open `koi/preview.html` in a browser to see the light pond on GitHub's light
page color and the dark pond on its dark one. `--season` takes `auto`, the
default, `spring`, `summer`, `autumn`, or `winter`. With `--style classic`,
`preview.html` shows the classic GIF and still instead, and `--body` takes
`white`, the default, `black`, `asagi`, `tea`, `gold`, or `platinum`. Options
may come in any order, as `--option value` or `--option=value`.

`collect` reads only public data: the account's public repositories, the public
repositories it has contributed to, GitHub's color for each language, how many
of its pull requests were merged into public repositories other accounts own,
and its daily contributions from the account's creation through the last
complete UTC day. It refuses a login that is not a valid GitHub login before
sending any request. It writes the snapshot to standard output, or with `--output` replaces the
file only after collection succeeds. In GitHub Actions, `GH_TOKEN` set to the
workflow's token is enough. `render` draws everything in memory first and
replaces each file through a temporary file, so a failed run keeps the previous
files. Both print errors to standard error and exit 1; invalid usage exits 2.
`git-koi --help` prints the full usage and `git-koi --version` the version.

## As a Library

The same crate is a library, documented on [docs.rs](https://docs.rs/git-koi).
Add it beside `serde_json`, which reads the snapshot:

```sh
cargo add git-koi serde_json
```

The pipeline runs one way. Read or collect an `AccountSnapshot`, derive the
koi's development, choose the day's behavior, render frames, and encode them:

```rust
use git_koi::behavior::select_behavior;
use git_koi::coat::KoiBody;
use git_koi::history::AccountSnapshot;
use git_koi::koi::derive_koi_development;
use git_koi::motion::frame_count;
use git_koi::output::{encode_gif, encode_png};
use git_koi::render::{render_koi, render_swim_frame};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let snapshot_json = std::fs::read_to_string("snapshot.json")?;
    let account_snapshot: AccountSnapshot = serde_json::from_str(&snapshot_json)?;
    let koi_development = derive_koi_development(&account_snapshot)?;
    let koi_body = KoiBody::Asagi;
    let behavior = select_behavior(&koi_development);
    let swim_frames: Vec<_> = (0..frame_count(behavior))
        .map(|frame_index| render_swim_frame(&koi_development, koi_body, behavior, frame_index))
        .collect();
    std::fs::write("koi.gif", encode_gif(&swim_frames)?)?;
    std::fs::write("koi.png", encode_png(&render_koi(&koi_development, koi_body))?)?;
    Ok(())
}
```

The pond takes the same development and behavior, plus the snapshot's count of
merged upstream pull requests, a season, and a theme:

```rust
use git_koi::output::encode_pond_gif;
use git_koi::pond::{
    POND_FRAME_COUNT, PondScene, PondTheme, Season, pond_frame_delay_centiseconds,
    render_pond_frame,
};

let season = Season::of_date(koi_development.observed_on);
let pond_scene = PondScene::new(
    &koi_development,
    behavior,
    account_snapshot.upstream_merged_pull_requests,
    season,
    PondTheme::Dark,
);
let pond_frames: Vec<_> = (0..POND_FRAME_COUNT)
    .map(|frame_index| render_pond_frame(&pond_scene, frame_index))
    .collect();
let delay = pond_frame_delay_centiseconds(behavior);
std::fs::write("koi-dark.gif", encode_pond_gif(&pond_frames, delay)?)?;
```

To collect in-process instead of reading a file, call
`collect::collect_account_snapshot` with `collect::GhCli` and today's UTC date.
Everything after collection is deterministic and performs no I/O, so the same
snapshot always draws the same koi. Breaking changes to the API bump the major
version.
