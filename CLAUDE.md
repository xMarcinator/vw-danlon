# vw-danlon: notes for agents

A private tool for one employee: working hours from the work calendar and
car trips, a mileage logbook under Skat's rules, vacation, and a checklist
of what to type into the Danløn app. It never talks to Danløn. Rust
workspace: library + CLI at the root, the Dioxus desktop and Android app in
`app/`. Specs and tasks live in `.flow/` (flow-next).

## Building and publishing

Releases are built and published only by GitHub Actions
(`.github/workflows/release.yml`), never from a developer machine. The user
downloads the APK and the other files from the repository's GitHub
Releases page, on any computer.

- To publish: bump `version` in `app/Cargo.toml` (versionCode is derived
  from it, so it must grow), commit, push, then tag and push the tag:
  `git tag v0.5.0 && git push origin v0.5.0`. The workflow checks the tag
  matches `app/Cargo.toml`, runs the tests, builds and signs the arm64 APK,
  builds the CLI for Linux and Windows and, if it works, the desktop app,
  and attaches them to the release with `SHA256SUMS.txt`.
- A failed run can be rebuilt from the Actions tab (Run workflow, with the
  existing tag).
- APKs are never built or signed locally; there is no local build script.
  The APK build steps live in the workflow's `apk` job (dx bundle, then the
  generated Gradle project's release variant, then zipalign and apksigner).
- The workflow caches Cargo (registry, target, `~/.cargo/bin` incl. `dx`),
  the Android NDK/build-tools/platform and Gradle between runs.

## The chart crates

The vacation chart is drawn by `flow-next-charts` and `flow-next-draw` from
the private `xMarcinator/flow-next-tracker` repository, pinned by commit in
`app/Cargo.toml` (`.cargo/config.toml` makes cargo fetch with git, so local
SSH keys work). The release workflow reads that repository with the secret
`FLOW_NEXT_TRACKER_DEPLOY_KEY`, a read-only deploy key on flow-next-tracker
whose private half is stored in this repository's secrets; the user sets it,
never the agent. To move the pin, change both `rev`s and run `cargo update
-p flow-next-charts -p flow-next-draw`. The app paints the chart's scene
itself in `app/src/chart.rs`, mapping its theme roles to CSS tokens.

## The signing key

The Android release keystore is stored only in the repository's GitHub
secrets, so home and work computers publish with the same key:

- `ANDROID_KEYSTORE_BASE64`: `release.jks`, base64 encoded
- `ANDROID_KEYSTORE_PASSWORD`: its password

Never commit the keystore, its password or a decoded copy, never print them
in logs, and never set or change these secrets yourself; the user does
that. If the key changes, phones must uninstall the app before a build
signed with the new key installs.

## Working rules

- Mockups first: draw or update the UI mockups for a change before
  implementing it.
- The tool stays invisible to payroll: no Danløn API, integration or import
  files.
- Mileage follows Skat's rules (LL § 9 B); they are the law and are not
  made switchable.
- No JavaScript in the app: use Dioxus/Rust APIs and JNI; Dioxus's own
  internal JavaScript is fine. No `document::eval`, no `<script>`, no .js
  files (`app/tests/no_javascript.rs` checks it). Android receives files
  by "Share to vw-danlon" (intent-filter patched into the generated
  manifest by the release workflow), not a file picker.
- Never read or change the user's `config.toml`, `vw-danlon.db` or
  `connected-cars-token` in the repo root; test on copies. Never call
  Connected Cars from tests.
