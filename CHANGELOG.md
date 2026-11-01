# Changelog

Notable changes to this project are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project
follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- The pond style: a white koi and a black koi circling a lily pond, drawn smooth
  at 800 by 480 for a 400-wide README image, in `koi-light.gif` and
  `koi-dark.gif` for GitHub's light and dark themes, with stills and a preview.
  The pair takes its colors from the account's languages through the coat's
  pigment rules, growth sets its length, the streaks gild its fins and add
  gin-rin glints, and the day's behavior sets the pace of the whole pond.
- A lotus for each pull request merged into a public repository another account
  owns, up to nine, and a caption naming the streak, the main language, the
  lotus count, and the pace.
- Four seasons, from the observation date or `--season`: cherry petals, a
  dragonfly and extra pads, maple leaves and yellowing pads, or ice and snow.
  The dark pond is lit by the moon, with fireflies outside winter.
- Snapshot version 4, which records the merged upstream pull request count;
  `git-koi collect` counts them through GitHub's search and refuses a login that
  is not a valid GitHub login before sending any request.
- `--style` and `--season` on `git-koi render`, and the Action inputs `style`
  and `season`. Render options may come in any order, as `--option value` or
  `--option=value` (#42).

### Changed

- `git-koi render` draws the pond unless given `--style classic`, which keeps
  the previous files and behavior.
- The Action builds git-koi from its own source instead of downloading a
  release, so a fork works without publishing releases.

## [1.0.2] - 2026-10-31

### Fixed

- `git-koi collect` requests again any contribution days GitHub leaves out of an
  answer, instead of failing with "missing contribution date". GitHub left out
  days for some accounts whose history crosses a February 29 (#52).

## [1.0.1] - 2026-10-28

### Changed

- The Action is listed as `git-profile-koi`, since a GitHub user already holds
  the name `git-koi`, and its description fits the Marketplace limit.

## [1.0.0] - 2026-10-28

### Added

- `git-koi collect`, which gathers an account's public snapshot through the
  GitHub CLI, and `--help` and `--version` on the command.
- `git-koi render`, which draws the day's koi as a looping GIF, a still PNG, and
  a preview page, in six bodies.
- A GitHub Action that downloads the release binary and runs both inside a
  profile repository's workflow.
- The `git-koi` library: account history, koi development, behavior, motion,
  markings, rendering, and encoding.

[Unreleased]: https://github.com/yalongwastaken/git-koi/compare/v1.0.2...HEAD
[1.0.2]: https://github.com/yalongwastaken/git-koi/compare/v1.0.1...v1.0.2
[1.0.1]: https://github.com/yalongwastaken/git-koi/compare/v1.0.0...v1.0.1
[1.0.0]: https://github.com/yalongwastaken/git-koi/releases/tag/v1.0.0
