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
Signed arm64 release APK via scripts/build-apk.sh: dx bundle --release, then the generated Gradle project's release variant (dx 0.7.10 only packages the debuggable debug variant) with libmain.so stripped and R8 off, zipalign, apksigner sign with ~/.config/vw-danlon/release.jks (RSA 4096, alias vw-danlon, password in release.jks.pass 0600, both outside the repo), apksigner verify and a not-debuggable check. Output dist/vw-danlon-0.1.0-arm64.apk (14.9 MB, v2+v3 signed, signer SHA-256 ca2bcf49355953ad8b6a03241630a2545791e808089aa438daba47c7e7eb53d7); dist/ git-ignored; README "Android phone app" covers prerequisites, keystore creation/backup/loss, build, adb install -r, USB debugging and unknown-apps install. Not installed on a device (none attached). Dioxus.toml unchanged: its signing config would need the password in a committed file. Follow-up: R8 keep rules if a smaller APK is wanted.

baseline: none (no Quick commands for packaging)
stage: impl-review - skipped(config: REVIEW_MODE=none)

stage: plan-sync - skipped(config: planSync.enabled != true)
## Evidence
- Commits: 00c22d4597705bbf6fb74b8c574b23be3920cb48
- Tests: scripts/build-apk.sh (rc=0), apksigner verify -v dist/vw-danlon-0.1.0-arm64.apk (Verifies, v2+v3), aapt2 dump badging: not debuggable
- PRs: