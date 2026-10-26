---
satisfies: [R1, R2, R6, R8]
---
# fnt-21-dioxus-desktop-and-phone-app.2 Dioxus app shell: responsive layout, theme, secrets, Settings

## Description
Create the Dioxus app crate building for Linux desktop and Android, with light/dark/system theme from the design-system tokens, one responsive layout (sidebar when wide, bottom tabs when narrow, on any platform), and a Settings screen that stores secrets in the platform secret store.

**Size:** M
**Files:** `app/Cargo.toml`, `app/src/**`, `app/assets/**`, `Dioxus.toml`
**Touches:** [app/**, Cargo.toml]

### Approach
- Tokens from the mockup palette (light/dark) as CSS variables; IBM Plex Sans bundled as an asset.
- Secret store behind one trait: OS keychain on desktop, Android Keystore-backed storage on Android; no plain-file fallback.
- Missing config/calendar/trips → open on Settings with the problem named.

### Design context
Mockup: https://claude.ai/artifact/K5zwEk1uPeNuifnnaZjZ2W (Sidebar, PhoneNav, Settings boards). Design system: https://claude.ai/artifact/UeddNBPX2SVMhcmm5AFWXV.

### Key context
Check current Dioxus release docs for Android build (`dx build --platform android`) and asset handling before coding; versions move fast.

## Acceptance
- [ ] `dx build` succeeds for desktop and Android
- [ ] Theme switch light/dark/system works on desktop
- [ ] Secrets saved and read back via the secret store; store failure shown, no file fallback
- [ ] Missing config opens Settings with the problem named
- [ ] Narrowing the desktop window switches sidebar to bottom tabs; Android shows bottom tabs

## Done summary
Dioxus 0.7.10 app shell in `app/` (dioxus-cli upgraded 0.6.3 -> 0.7.10): one set of screens with a CSS width breakpoint (sidebar >= 720px, bottom tabs below, Settings via the top-bar gear when narrow), light/dark/system theme from the mockup tokens with bundled IBM Plex (OFL), and a Settings screen. Secrets (calendar link, Connected Cars device token) go through a `SecretStore` trait: system keychain on desktop (device token shares the CLI's `vw-danlon` keychain entry), Android Keystore via android-native-keyring-store on Android, and an opt-in 0600 file (`app.toml` `secret_file`, toggle in Settings) like the CLI's `token_file`; store failures are shown with the opt-in named, never a silent fallback. Settings saves the link to the store and workplace / `mileage.home` into config.toml with toml_edit (comments kept, created from a minimal starter when missing, validated by `Config::from_text` before anything is written). Missing/bad config, no calendar, no/bad trips and store failures open the app on Settings with the problem named. Library: `Config::from_text` split out of `Config::load` (CLI behaviour unchanged). Data dir: `VW_DANLON_DIR`, else `~/.config/vw-danlon` (desktop) / app files dir via JNI (Android).

Live run (scratch VW_DANLON_DIR, GDK_BACKEND=x11): opened on Settings naming the keychain failure (no Secret Service), missing config.toml and missing calendar; opt-in file -> secrets.json mode 0600; Light/Dark/System switch and persisted in app.toml; save created config.toml with workplace, link only in secrets.json; window resized to 390px -> bottom tabs, back to 1000px -> sidebar.
Builds: `dx build --platform desktop` ok; `dx build --platform android --target aarch64-linux-android` ok -> target/dx/vw-danlon-app/debug/android/app/app/build/outputs/apk/debug/app-debug.apk (debug, not run on a device: none attached). clippy clean for host and aarch64-linux-android.

Follow-ups: Home address needs an existing [mileage] section (field disabled otherwise; mileage setup comes with the Mileage screen); calendar fetch / account sync on Android will need rustls-platform-verifier's Android init; Android Keystore path compiled but unverified on a device.

baseline: green (cargo test --workspace, 69 passed at ae9c042)
Tier: not reported (model: claude-opus-5-5)

stage: impl-review - skipped(config: REVIEW_MODE=none - no review mode given in the dispatch)

stage: plan-sync - skipped(config: planSync.enabled != true)
## Evidence
- Commits: 165b2ca3fddc1e9d172989c91721aa3187864ab4
- Tests: cargo test --workspace (77 passed: 69 lib + 8 app), cargo clippy --workspace --all-targets (clean), dx build --platform desktop (ok), dx build --platform android --target aarch64-linux-android (ok, app-debug.apk)
- PRs: