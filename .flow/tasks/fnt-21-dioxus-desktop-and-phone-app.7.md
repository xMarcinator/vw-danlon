---
satisfies: [R1]
---
# fnt-21-dioxus-desktop-and-phone-app.7 Signed release APK and install docs

## Description
A signed release APK the user can sideload.

**Size:** S
**Files:** `app/Dioxus.toml`, `README.md`, `.gitignore`
**Touches:** [app/Dioxus.toml, README.md, .gitignore]

### Approach
- `dx bundle --platform android --release` for arm64; sign with a local release keystore created once outside git (e.g. `~/.config/vw-danlon/release.jks`, password in a local file outside the repo), so later updates install over it.
- README: build, sign and `adb install -r` steps; where the keystore lives and that losing it means uninstalling before the next install.

## Acceptance
- [ ] A release APK (arm64) is built and signed with the local keystore; `apksigner verify` passes
- [ ] The keystore and its password are outside the repo and git-ignored paths
- [ ] README documents building and installing


## Done summary
TBD

## Evidence
- Commits:
- Tests:
- PRs:
