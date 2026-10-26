---
satisfies: [R1]
---
# fnt-27-one-period-selector-on-every-page-day.3 No JavaScript in the app: Rust copy, scroll, resume; Android import by share

## Description
Remove all JavaScript from the app (user: "I do want to avoid any Javascript in the src" / "I meant at all"). Decisions by the user: no Paste in text fields (keep Dioxus's default context-menu blocking, which is JavaScript inside Dioxus, not ours; do NOT enable Dioxus devtools); the Android file picker is replaced by "Share to vw-danlon" from Min Volkswagen.

**Files:** app/src/main.rs, app/src/import.rs, app/src/screens/mod.rs, app/src/screens/danlon.rs, app/src/screens/month.rs, app/src/platform.rs (JNI), app/Cargo.toml, .github/workflows/release.yml (manifest patch), CLAUDE.md

- Remove ALLOW_FIELD_MENU_JS (no paste, per the user).
- Copy to clipboard in Rust: desktop with a clipboard crate (e.g. arboard), Android via JNI ClipboardManager.setPrimaryClip (jni/ndk-context are already Android deps).
- Scrolling (⋯ menu into view; scroll to top on period change) with Dioxus's MountedData scroll APIs (onmounted), not eval.
- Resume detection (refresh on return) in Rust: the window/app lifecycle events Dioxus exposes (desktop Config::with_custom_event_handler: WindowEvent::Focused / Event::Resumed on Android), not a JS visibilitychange listener.
- Android import by share: add an intent-filter (ACTION_SEND with application/vnd.openxmlformats-officedocument.spreadsheetml.sheet, text/csv, application/json and */* fallback; and ACTION_VIEW for the same) to the generated AndroidManifest in the release workflow's "Prepare the generated Gradle project" step; at startup (and on return) read the activity's intent through JNI (getIntent, EXTRA_STREAM / getData, ContentResolver.openInputStream) and import the bytes with trip_store::import_bytes; show the result like the desktop import. Remove the Android picker button and PICK_JS; desktop keeps Dioxus's file input.
- No `document::eval` and no JavaScript strings or .js files anywhere in the repo (tests may assert this). Add the rule to CLAUDE.md.

## Acceptance
- [ ] `grep -rn "eval(\|<script\|\.js\"" app/src` finds nothing; no .js files in the repo
- [ ] Copy works on desktop (checked) and compiles for Android (JNI)
- [ ] Scroll and resume refresh work without JavaScript (desktop checked)
- [ ] Workflow adds the share intent-filter; the Rust intent reader compiles for Android (verified by Android clippy); no picker on Android
- [ ] cargo test, clippy desktop and Android clean; CLAUDE.md rule added


## Done summary
The app has no JavaScript of its own: copy uses arboard (desktop) and the Android ClipboardManager over JNI, the Danløn ⋯ menu and Hours' scroll-to-top use Dioxus's MountedData, refresh on return comes from tao's Focused(true)/Resumed events through use_wry_event_handler (counted once per second), and desktop print calls the webview's print. The Paste context-menu script is gone (per the user). Android has no file picker: the release workflow adds SEND/VIEW intent filters (xlsx, csv, json, */*), makes the activity singleTask and patches MainActivity.onNewIntent to keep the new intent; the app reads the intent over JNI at start and on each return, clears it, and imports the file on Settings (a shared backup waits for Import backup). Tests: `a_shared_file_is_named_for_the_import`, `coming_back_twice_at_once_counts_once`, `app/tests/no_javascript.rs`. CLAUDE.md and README updated.

Verified live on the desktop (scratch data, offline, 390 px): copy in the Danløn ⋯ menu (wl-paste returned "Kunde (test)"), the menu scrolled into view above the tab bar, Hours scrolled to the top on opening a day, and refocusing the window bumped Resumed and recomputed the month (temporary log, removed). Android is compiled and clippy-clean only; the share flow, JNI clipboard and the workflow manifest patch (dry-run on a copy of dx's generated manifest and MainActivity.kt) are not run on a phone.

Follow-ups: the shared-file flow and launchMode singleTask need a check on the phone with the next APK.

Tier: not reported (model: claude-opus-5-5)

stage: impl-review - skipped(config: REVIEW_MODE=none)

stage: plan-sync - skipped(config: planSync.enabled != true)
## Evidence
- Commits: 1ff503c137d90232051c51b0da5532b62e1611f9
- Tests: cargo test --workspace, cargo clippy --workspace --all-targets, cargo clippy -p vw-danlon-app -p vw-danlon --target aarch64-linux-android --no-default-features --features vw-danlon-app/mobile
- PRs: