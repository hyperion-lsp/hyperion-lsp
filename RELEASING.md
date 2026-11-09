# Releasing

Releases are built by [dist](https://opensource.axo.dev/cargo-dist/) from a
version tag. The configuration is `dist-workspace.toml`; the workflow
`.github/workflows/release.yml` is generated from it.

## Cut a release

1. Bump `version` under `[workspace.package]` in `Cargo.toml` and
   `editors/vscode/package.json` (plus its lockfile entry). Refresh
   `Cargo.lock` with `cargo update -w` if it changes.
2. Add a section for the version to `CHANGELOG.md`.
3. Open a pull request, wait for CI (and the release workflow's `plan` job,
   which runs on pull requests), and merge it.
4. Tag the merge commit and push the tag. The tag must equal the Cargo version
   with a leading `v`:

   ```bash
   git tag v0.1.1
   git push origin v0.1.1
   ```

   A version with a pre-release suffix (`v0.2.0-rc.1`) publishes a GitHub
   pre-release.

## What the pipeline does

On a tag push, the `Release` workflow:

1. **plan** computes the artifacts and checks the tag against the Cargo version.
2. **build-local-artifacts** builds `hyperion-lsp` natively on each runner for
   `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`,
   `x86_64-apple-darwin` and `aarch64-apple-darwin`. Before each build it
   generates `THIRD_PARTY_LICENSES` with cargo-about (`about.toml`,
   `packaging/third-party-licenses.hbs`); the file lists every bundled
   crate's license text and the source locations of the MPL-2.0 crates. Each
   `.tar.xz` archive contains the binary, `LICENSE`, `LICENSE-APACHE-2.0`,
   `NOTICE`, `THIRD_PARTY_LICENSES`, `README.md` and `CHANGELOG.md`.
3. **build-global-artifacts** produces the shell installer
   (`hyperion-lsp-installer.sh`), the Homebrew formula (`hyperion-lsp.rb`),
   `sha256.sum` and the source tarball.
4. **host** creates the GitHub Release (immediately public, not a draft) and
   uploads everything.

If a dependency picks up a license that `about.toml` does not list under
`accepted`, the build fails (CI's `third-party licenses` job catches this
earlier). Review the license, then add it to `accepted`.

After changing `dist-workspace.toml`, run `dist generate` and commit the result;
`dist plan` shows what a tag would produce. To build one target locally, run
`dist build --artifacts=local --target <triple>` (generate
`THIRD_PARTY_LICENSES` first, as the release workflow does).

## Homebrew tap

The release workflow does not push to the tap, because that would need a
cross-repository token. Instead the tap
([hyperion-lsp/homebrew-tap](https://github.com/hyperion-lsp/homebrew-tap))
polls: its `Update formula` workflow runs hourly, downloads the latest release's
`hyperion-lsp.rb`, writes `Formula/hyperion-lsp.rb` and commits when it changed,
then runs `brew style` and `brew audit`. Pre-releases are not picked up.

To sync right away after a release, run the workflow by hand:

```bash
gh workflow run update.yml -R hyperion-lsp/homebrew-tap
gh run watch -R hyperion-lsp/homebrew-tap
```

GitHub disables scheduled workflows in a repository after 60 days without
activity; if the formula is not following releases, re-enable the workflow in the
tap's Actions tab.

## VS Code extension

The release workflow does not package the extension. Build it from
`editors/vscode` (see the README) and publish it separately.
