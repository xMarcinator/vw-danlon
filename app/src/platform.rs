//! Reaching outside the app: opening the Danløn app (Android) or its web
//! site (desktop), printing the logbook (the desktop webview's print
//! dialog, or on Android, whose webview does not print, the share sheet),
//! the clipboard, and on Android the file shared to the app. All in Rust
//! (JNI on Android), no JavaScript.

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
    dioxus::desktop::window().print();
    Ok(None)
}

/// Puts `text` on the clipboard. The clipboard is kept open while the app
/// runs: on Linux the copied text is served by its owner.
#[cfg(not(target_os = "android"))]
pub fn copy_text(text: &str) -> Result<()> {
    use std::sync::Mutex;
    static CLIPBOARD: Mutex<Option<arboard::Clipboard>> = Mutex::new(None);
    let mut kept = CLIPBOARD.lock().unwrap_or_else(|e| e.into_inner());
    let clipboard = match kept.as_mut() {
        Some(c) => c,
        None => kept.insert(arboard::Clipboard::new()?),
    };
    clipboard.set_text(text)?;
    Ok(())
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

    pub fn string<'a>(env: &mut JNIEnv<'a>, s: &str) -> jni::errors::Result<JObject<'a>> {
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

/// Puts `text` on the clipboard through the ClipboardManager.
#[cfg(target_os = "android")]
pub fn copy_text(text: &str) -> Result<()> {
    use jni::objects::JValue;
    android::with_context(|env, context| {
        let service = android::string(env, "clipboard")?;
        let manager = env
            .call_method(
                context,
                "getSystemService",
                "(Ljava/lang/String;)Ljava/lang/Object;",
                &[JValue::Object(&service)],
            )?
            .l()?;
        let label = android::string(env, "vw-danlon")?;
        let text = android::string(env, text)?;
        let clip = env
            .call_static_method(
                "android/content/ClipData",
                "newPlainText",
                "(Ljava/lang/CharSequence;Ljava/lang/CharSequence;)Landroid/content/ClipData;",
                &[JValue::Object(&label), JValue::Object(&text)],
            )?
            .l()?;
        env.call_method(
            &manager,
            "setPrimaryClip",
            "(Landroid/content/ClipData;)V",
            &[JValue::Object(&clip)],
        )?;
        Ok(())
    })
    .map_err(|e| e.context("the clipboard can't be used"))
}

/// The largest shared file read; a trips export is far smaller.
#[cfg(target_os = "android")]
const SHARED_MAX: usize = 64 << 20;

/// Takes the file shared to the app, if the activity was started with one:
/// "Share to vw-danlon" (`ACTION_SEND`, the file in `EXTRA_STREAM`) or
/// "Open with" (`ACTION_VIEW`, the file as the data). The activity's intent
/// is then replaced by a plain one, so the file is taken once.
///
/// The activity is `singleTask` and its `onNewIntent` keeps the new intent
/// (both patched into the generated project by the release workflow), so a
/// share while the app is already running reaches the same activity and is
/// taken here when the app comes back to the foreground.
#[cfg(target_os = "android")]
pub fn take_shared_file() -> Result<Option<crate::import::Picked>> {
    use jni::objects::{JByteArray, JString, JValue};
    let picked = android::with_context(|env, activity| {
        let intent = env
            .call_method(activity, "getIntent", "()Landroid/content/Intent;", &[])?
            .l()?;
        if intent.is_null() {
            return Ok(None);
        }
        let action = env
            .call_method(&intent, "getAction", "()Ljava/lang/String;", &[])?
            .l()?;
        let action: String = if action.is_null() {
            String::new()
        } else {
            env.get_string(&JString::from(action))?.into()
        };
        let uri = match action.as_str() {
            "android.intent.action.SEND" => {
                let key = android::string(env, "android.intent.extra.STREAM")?;
                let uri = env
                    .call_method(
                        &intent,
                        "getParcelableExtra",
                        "(Ljava/lang/String;)Landroid/os/Parcelable;",
                        &[JValue::Object(&key)],
                    )?
                    .l()?;
                if uri.is_null() {
                    // Some apps only fill the clip data.
                    let clip = env
                        .call_method(&intent, "getClipData", "()Landroid/content/ClipData;", &[])?
                        .l()?;
                    if clip.is_null() {
                        uri
                    } else {
                        let item = env
                            .call_method(
                                &clip,
                                "getItemAt",
                                "(I)Landroid/content/ClipData$Item;",
                                &[JValue::Int(0)],
                            )?
                            .l()?;
                        env.call_method(&item, "getUri", "()Landroid/net/Uri;", &[])?
                            .l()?
                    }
                } else {
                    uri
                }
            }
            "android.intent.action.VIEW" => env
                .call_method(&intent, "getData", "()Landroid/net/Uri;", &[])?
                .l()?,
            _ => return Ok(None),
        };
        // Taken once, even when it can't be read.
        let plain = env.new_object("android/content/Intent", "()V", &[])?;
        env.call_method(
            activity,
            "setIntent",
            "(Landroid/content/Intent;)V",
            &[JValue::Object(&plain)],
        )?;
        if uri.is_null() {
            return Ok(None);
        }
        let resolver = env
            .call_method(
                activity,
                "getContentResolver",
                "()Landroid/content/ContentResolver;",
                &[],
            )?
            .l()?;
        let mime = env
            .call_method(
                &resolver,
                "getType",
                "(Landroid/net/Uri;)Ljava/lang/String;",
                &[JValue::Object(&uri)],
            )?
            .l()?;
        let mime: Option<String> = if mime.is_null() {
            None
        } else {
            Some(env.get_string(&JString::from(mime))?.into())
        };
        let display = display_name(env, &resolver, &uri)?;
        let name = crate::import::shared_name(display.as_deref(), mime.as_deref());
        let stream = env
            .call_method(
                &resolver,
                "openInputStream",
                "(Landroid/net/Uri;)Ljava/io/InputStream;",
                &[JValue::Object(&uri)],
            )?
            .l()?;
        if stream.is_null() {
            return Ok(None);
        }
        let chunk: JByteArray = env.new_byte_array(64 * 1024)?;
        let mut bytes = Vec::new();
        let mut buf = vec![0i8; 64 * 1024];
        loop {
            let n = env
                .call_method(&stream, "read", "([B)I", &[JValue::Object(&chunk)])?
                .i()?;
            if n < 0 || bytes.len() > SHARED_MAX {
                break;
            }
            let part = &mut buf[..n as usize];
            env.get_byte_array_region(&chunk, 0, part)?;
            bytes.extend(part.iter().map(|&b| b as u8));
        }
        env.call_method(&stream, "close", "()V", &[])?;
        Ok(Some(crate::import::Picked { name, bytes }))
    })
    .map_err(|e| e.context("the shared file can't be read"))?;
    if let Some(p) = &picked
        && p.bytes.len() > SHARED_MAX
    {
        anyhow::bail!("{} is too large to be a trips file", p.name);
    }
    Ok(picked)
}

/// The shared file's display name, from its content provider.
#[cfg(target_os = "android")]
fn display_name(
    env: &mut jni::JNIEnv,
    resolver: &jni::objects::JObject,
    uri: &jni::objects::JObject,
) -> jni::errors::Result<Option<String>> {
    use jni::objects::{JObject, JString, JValue};
    let column = android::string(env, "_display_name")?;
    let columns = env.new_object_array(1, "java/lang/String", &column)?;
    let null = JObject::null();
    let cursor = env
        .call_method(
            resolver,
            "query",
            "(Landroid/net/Uri;[Ljava/lang/String;Ljava/lang/String;[Ljava/lang/String;Ljava/lang/String;)Landroid/database/Cursor;",
            &[
                JValue::Object(uri),
                JValue::Object(&columns),
                JValue::Object(&null),
                JValue::Object(&null),
                JValue::Object(&null),
            ],
        )?
        .l()?;
    if cursor.is_null() {
        return Ok(None);
    }
    let mut name = None;
    if env.call_method(&cursor, "moveToFirst", "()Z", &[])?.z()? {
        let value = env
            .call_method(
                &cursor,
                "getString",
                "(I)Ljava/lang/String;",
                &[JValue::Int(0)],
            )?
            .l()?;
        if !value.is_null() {
            name = Some(env.get_string(&JString::from(value))?.into());
        }
    }
    env.call_method(&cursor, "close", "()V", &[])?;
    Ok(name)
}
