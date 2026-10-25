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
TBD

## Evidence
- Commits:
- Tests:
- PRs:

