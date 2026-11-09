# Contributing

## Bugs

In Guild, Settings → About → Report a problem opens a GitHub issue with your app version and OS filled in. You can also [open a bug report](https://github.com/TheBiemGamer/guild/issues/new?template=bug_report.yml) directly. For security problems, follow [SECURITY.md](SECURITY.md) instead.

## Ideas

Open an issue before you write code for a new feature, so we can agree on it first. [PRODUCT.md](PRODUCT.md) describes who Guild is for and what it must stay.

## Pull requests

Pull requests are welcome if they're high quality. That means one change per pull request, tested on the OS it touches, with a description of what it fixes or adds and why. I'll close ones that don't build, mix unrelated changes, or rewrite code that didn't need it.

[DEVELOPMENT.md](DEVELOPMENT.md) covers building Guild, the code layout and the checks. Before you open a pull request:

- Every commit passes the checks listed in DEVELOPMENT.md and is formatted with `npm run format` and `cargo fmt`.
- Commit messages follow [Conventional Commits](https://www.conventionalcommits.org).
- UI changes follow [DESIGN.md](DESIGN.md), and update it when the UI changes.
- UI text goes through `t()` with a key in `src/lib/i18n/en.json`. Add the Dutch text to `nl.json` too, or say in the pull request that it's missing.
- Secrets stay in the OS keyring. Never write one to the database, a file, a log or the UI.
- Database changes are new migration steps at the end of `STEPS` in `src-tauri/src/cache/db.rs`. Never edit a step that already shipped.
- Ask in an issue before adding a crate or npm package.

Guild is licensed under the [GPL-3.0](LICENSE). By contributing, you agree that your contribution is released under the same license.

## Conduct

Everyone taking part follows the [code of conduct](CODE_OF_CONDUCT.md).
