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
TBD

## Evidence
- Commits:
- Tests:
- PRs:
