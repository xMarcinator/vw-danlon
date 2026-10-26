//! Reaching outside the app: opening the Danløn app (Android) or its web
//! site (desktop), and printing the logbook: the desktop webview's print
//! dialog, or on Android (whose webview does not print) the share sheet.

use anyhow::Result;

/// The Danløn app's Android package.
#[cfg(target_os = "android")]
const DANLON_PACKAGE: &str = "dk.danlon.app";

/// Where the desktop opens Danløn.
#[cfg(not(target_os = "android"))]
const DANLON_URL: &str = "https://danlon.dk/";

/// Opens Danløn: on Android its app, else its Play Store page; on the
/// desktop danlon.dk in the browser. Nothing is sent. The text says what
/// was opened.
#[cfg(not(target_os = "android"))]
pub fn open_danlon() -> Result<String> {
    open_url(DANLON_URL)?;
    Ok(format!("Opened {DANLON_URL} in the browser"))
}

/// The print view's button.
#[cfg(not(target_os = "android"))]
pub const PRINT_LABEL: &str = "Print";
#[cfg(target_os = "android")]
pub const PRINT_LABEL: &str = "Share to print";

/// Prints the page shown with the webview's print dialog; `page` (the
/// same as a standalone page titled `title`) is for Android. The text, if
/// any, says what was done.
#[cfg(not(target_os = "android"))]
pub fn print_page(_title: &str, _page: &str) -> Result<Option<String>> {
    dioxus::prelude::document::eval("window.print()");
    Ok(None)
}

/// Shares `page` through the share sheet, so it can be saved or printed.
#[cfg(target_os = "android")]
pub fn print_page(title: &str, page: &str) -> Result<Option<String>> {
    share(title, "text/html", page, Some(page))?;
    Ok(Some("Shared the printable logbook".into()))
}

/// Opens `url` with the desktop's default handler.
#[cfg(not(target_os = "android"))]
fn open_url(url: &str) -> Result<()> {
    use anyhow::Context;
    #[cfg(target_os = "macos")]
    let mut cmd = std::process::Command::new("open");
    #[cfg(target_os = "windows")]
    let mut cmd = {
        let mut c = std::process::Command::new("cmd");
        c.args(["/C", "start", ""]);
        c
    };
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let mut cmd = std::process::Command::new("xdg-open");
    let mut child = cmd
        .arg(url)
        .spawn()
        .with_context(|| format!("can't open {url}"))?;
    // Reap the opener when it exits.
    std::thread::spawn(move || child.wait());
    Ok(())
}

/// The Android intents, called over JNI on the app's context.
#[cfg(target_os = "android")]
mod android {
    use anyhow::{Result, anyhow};
    use jni::JNIEnv;
    use jni::objects::{JObject, JValue};

    const FLAG_ACTIVITY_NEW_TASK: i32 = 0x1000_0000;

    /// Runs `f` with the JNI environment and the app's context, in a local
    /// frame so its references are freed. A Java exception is cleared and
    /// returned as the error.
    pub fn with_context<T>(
        f: impl FnOnce(&mut JNIEnv, &JObject) -> jni::errors::Result<T>,
    ) -> Result<T> {
        let ctx = ndk_context::android_context();
        // SAFETY: the pointers come from ndk-context, which the Android
        // launcher initialises with the process's VM and the app context.
        let vm = unsafe { jni::JavaVM::from_raw(ctx.vm().cast()) }?;
        let mut env = vm.attach_current_thread()?;
        let context = unsafe { JObject::from_raw(ctx.context().cast()) };
        let result = env.with_local_frame(16, |env| f(env, &context));
        match result {
            Ok(v) => Ok(v),
            Err(e) => {
                if env.exception_check().unwrap_or(false) {
                    let _ = env.exception_describe();
                    let _ = env.exception_clear();
                }
                Err(anyhow!("{e}"))
            }
        }
    }

    fn string<'a>(env: &mut JNIEnv<'a>, s: &str) -> jni::errors::Result<JObject<'a>> {
        Ok(env.new_string(s)?.into())
    }

    pub fn new_intent<'a>(env: &mut JNIEnv<'a>, action: &str) -> jni::errors::Result<JObject<'a>> {
        let action = string(env, action)?;
        env.new_object(
            "android/content/Intent",
            "(Ljava/lang/String;)V",
            &[JValue::Object(&action)],
        )
    }

    /// Calls a `String`-taking `Intent` builder method such as `setPackage`.
    pub fn set(
        env: &mut JNIEnv,
        intent: &JObject,
        method: &str,
        value: &str,
    ) -> jni::errors::Result<()> {
        let value = string(env, value)?;
        env.call_method(
            intent,
            method,
            "(Ljava/lang/String;)Landroid/content/Intent;",
            &[JValue::Object(&value)],
        )?;
        Ok(())
    }

    pub fn put_extra(
        env: &mut JNIEnv,
        intent: &JObject,
        key: &str,
        value: &str,
    ) -> jni::errors::Result<()> {
        let (key, value) = (string(env, key)?, string(env, value)?);
        env.call_method(
            intent,
            "putExtra",
            "(Ljava/lang/String;Ljava/lang/String;)Landroid/content/Intent;",
            &[JValue::Object(&key), JValue::Object(&value)],
        )?;
        Ok(())
    }

    /// Starts `intent` as a new task; an error when nothing handles it.
    pub fn start(env: &mut JNIEnv, context: &JObject, intent: &JObject) -> jni::errors::Result<()> {
        env.call_method(
            intent,
            "addFlags",
            "(I)Landroid/content/Intent;",
            &[JValue::Int(FLAG_ACTIVITY_NEW_TASK)],
        )?;
        env.call_method(
            context,
            "startActivity",
            "(Landroid/content/Intent;)V",
            &[JValue::Object(intent)],
        )?;
        Ok(())
    }

    /// Opens `url` with the app that handles it.
    pub fn view(env: &mut JNIEnv, context: &JObject, url: &str) -> jni::errors::Result<()> {
        let url = string(env, url)?;
        let uri = env
            .call_static_method(
                "android/net/Uri",
                "parse",
                "(Ljava/lang/String;)Landroid/net/Uri;",
                &[JValue::Object(&url)],
            )?
            .l()?;
        let action = string(env, "android.intent.action.VIEW")?;
        let intent = env.new_object(
            "android/content/Intent",
            "(Ljava/lang/String;Landroid/net/Uri;)V",
            &[JValue::Object(&action), JValue::Object(&uri)],
        )?;
        start(env, context, &intent)
    }
}

/// The ways Danløn is opened on Android, in order: the app's launcher
/// activity, its Play Store page in the Play Store app, then on the web.
#[cfg(target_os = "android")]
#[derive(Debug, PartialEq, Eq)]
enum Opener {
    App(&'static str),
    Url(String),
}

#[cfg(target_os = "android")]
fn danlon_openers() -> [Opener; 3] {
    [
        Opener::App(DANLON_PACKAGE),
        Opener::Url(format!("market://details?id={DANLON_PACKAGE}")),
        Opener::Url(format!(
            "https://play.google.com/store/apps/details?id={DANLON_PACKAGE}"
        )),
    ]
}

#[cfg(target_os = "android")]
pub fn open_danlon() -> Result<String> {
    let mut last = None;
    for opener in danlon_openers() {
        let result = android::with_context(|env, context| match &opener {
            Opener::App(package) => {
                let intent = android::new_intent(env, "android.intent.action.MAIN")?;
                android::set(
                    env,
                    &intent,
                    "addCategory",
                    "android.intent.category.LAUNCHER",
                )?;
                android::set(env, &intent, "setPackage", package)?;
                android::start(env, context, &intent)
            }
            Opener::Url(url) => android::view(env, context, url),
        });
        match (result, &opener) {
            (Ok(()), Opener::App(_)) => return Ok("Opened the Danløn app".into()),
            (Ok(()), Opener::Url(_)) => {
                return Ok("The Danløn app isn't installed; opened its Play Store page".into());
            }
            (Err(e), _) => last = Some(e),
        }
    }
    Err(last
        .unwrap_or_else(|| anyhow::anyhow!("nothing opened"))
        .context("can't open the Danløn app or its Play Store page"))
}

/// Shares `text` (and `html`, for apps that take it) through the Android
/// share sheet, so it can be saved, mailed or printed.
#[cfg(target_os = "android")]
pub fn share(title: &str, mime: &str, text: &str, html: Option<&str>) -> Result<()> {
    use jni::objects::JValue;
    android::with_context(|env, context| {
        let intent = android::new_intent(env, "android.intent.action.SEND")?;
        android::set(env, &intent, "setType", mime)?;
        android::put_extra(env, &intent, "android.intent.extra.SUBJECT", title)?;
        android::put_extra(env, &intent, "android.intent.extra.TITLE", title)?;
        android::put_extra(env, &intent, "android.intent.extra.TEXT", text)?;
        if let Some(html) = html {
            android::put_extra(env, &intent, "android.intent.extra.HTML_TEXT", html)?;
        }
        let title = env.new_string(title)?;
        let chooser = env
            .call_static_method(
                "android/content/Intent",
                "createChooser",
                "(Landroid/content/Intent;Ljava/lang/CharSequence;)Landroid/content/Intent;",
                &[JValue::Object(&intent), JValue::Object(&title)],
            )?
            .l()?;
        android::start(env, context, &chooser)
    })
    .map_err(|e| e.context("the share sheet can't be opened"))
}
