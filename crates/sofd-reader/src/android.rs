//! The Android entry point. An Android app has no `main`: the system starts a
//! thread of its own and calls `android_main` on it, handing over the activity
//! the event loop needs. Everything after that is the same frame the desktop
//! uses. A device also has no file dialog, so a document arrives through the
//! intent that started the activity, read before the first frame.

use crate::{App, DroppedFile, window};
use eframe::NativeOptions;
use eframe::egui;
use jni::objects::{JObject, JString};
use jni::{Env, JavaVM, jni_sig, jni_str};

/// The last thing a reader reports, and it must land somewhere: Android has no
/// stderr to read and no console to watch, so a log line is only written when
/// `android.util.Log` is behind it.
struct Logcat;

impl log::Log for Logcat {
    fn enabled(&self, _metadata: &log::Metadata<'_>) -> bool {
        true
    }

    fn log(&self, record: &log::Record<'_>) {
        write_to_logcat(record.level(), &record.args().to_string());
    }

    fn flush(&self) {}
}

/// One line to `android.util.Log`, on whatever thread writes it. Every failure
/// is dropped rather than reported: the report would go to the log being
/// written, and a logger that recursed would never return.
fn write_to_logcat(level: log::Level, message: &str) {
    let Some(vm) = JavaVM::singleton().ok().or_else(|| VM.get().cloned()) else {
        return;
    };
    let _ = vm.attach_current_thread(|env| -> Result<(), jni::errors::Error> {
        let tag = env.new_string("sofd_reader")?;
        let text = env.new_string(message)?;
        let method = match level {
            log::Level::Error => jni_str!("e"),
            log::Level::Warn => jni_str!("w"),
            log::Level::Info => jni_str!("i"),
            log::Level::Debug | log::Level::Trace => jni_str!("d"),
        };
        env.call_static_method(
            jni_str!("android.util.Log"),
            method,
            jni_sig!((tag: java.lang.String, message: java.lang.String) -> int),
            &[(&tag).into(), (&text).into()],
        )?;
        Ok(())
    });
}

/// The VM the library was loaded into, kept because `JavaVM::singleton` alone
/// is not enough: it is set by `JNI_OnLoad`, which runs before the entry point
/// but leaves nothing usable for a thread that has not attached yet.
static VM: std::sync::OnceLock<JavaVM> = std::sync::OnceLock::new();

/// Called by `android-activity` once the activity exists. `no_mangle` is what
/// makes the symbol reachable from the Java side.
#[unsafe(no_mangle)]
fn android_main(app: android_activity::AndroidApp) {
    // Installed first, so a reader that stops while it is still starting leaves
    // a reason behind rather than an empty window and a silent log.
    if log::set_logger(&Logcat).is_ok() {
        // `debug` because wgpu explains a refused device at that level, and a
        // device that cannot draw is the failure this build exists to report.
        log::set_max_level(log::LevelFilter::Debug);
    }
    log::info!("android_main started on {:?}", thread_name());
    // Read before the loop starts: the intent names the document the tap was
    // for, and the activity is the only handle on it.
    let handed_over = document_from_intent(&app);
    // `setprop debug.sofd.renderer glow` picks the renderer on a device
    // whose driver refuses one of them, and is how the emulator is tested:
    // its software Vulkan builds a wgpu device and then never finishes a frame.
    let first = match renderer_choice() {
        Some(renderer) => renderer,
        None => eframe::Renderer::Wgpu,
    };
    // wgpu first, as on the desktop, but a driver may refuse a pipeline rather
    // than report it, and an emulator's software Vulkan does exactly that: the
    // same loop the desktop runs then applies, and the refusal is not the end.
    let mut last = None;
    for renderer in [first, other(first)] {
        let options = NativeOptions {
            android_app: Some(app.clone()),
            viewport: egui::ViewportBuilder::default()
                .with_title("sOFD Reader")
                .with_min_inner_size([360.0, 480.0]),
            renderer,
            ..Default::default()
        };
        match open(options, handed_over.clone()) {
            Ok(()) => {
                log::info!("android_main on {:?} returned", thread_name());
                return;
            }
            Err(err) => {
                log::error!("the window did not open with {renderer:?}: {err}");
                last = Some(err);
            }
        }
    }
    // Logcat is the only place this is seen, so the report names the reason
    // rather than leaving an activity that quits without one.
    if let Some(err) = last {
        log::error!("neither renderer could open a window; the last said {err}");
    }
}

/// The renderer the device asks for, if it asks. A property is read instead of
/// an environment variable because nothing sets one for an app the launcher
/// starts, while `setprop` works on any device.
fn renderer_choice() -> Option<eframe::Renderer> {
    let asked = system_property("debug.sofd.renderer")?;
    match asked.trim() {
        "glow" => Some(eframe::Renderer::Glow),
        "wgpu" => Some(eframe::Renderer::Wgpu),
        other => {
            log::warn!("debug.sofd.renderer names {other:?}, which is neither glow nor wgpu");
            None
        }
    }
}

/// An Android system property. `std::env::var` does not see these: they live in
/// a shared-memory region the runtime reads, not in the environment, so the
/// `setprop` a tester types would otherwise be invisible to the app.
fn system_property(name: &str) -> Option<String> {
    const MAX: usize = 92;
    // SAFETY: `__system_property_get` writes at most `MAX` bytes plus the
    // terminator into the buffer, and the buffer is that size; the name is
    // NUL-terminated below and outlives the call.
    let mut value = [0u8; MAX];
    let mut key = Vec::with_capacity(name.len() + 1);
    key.extend_from_slice(name.as_bytes());
    key.push(0);
    let read = unsafe {
        __system_property_get(
            key.as_ptr().cast::<std::ffi::c_char>(),
            value.as_mut_ptr().cast::<std::ffi::c_char>(),
        )
    };
    if read <= 0 {
        return None;
    }
    let taken = usize::try_from(read).ok()?.min(MAX - 1);
    String::from_utf8(value[..taken].to_vec())
        .ok()
        .filter(|text| !text.is_empty())
}

unsafe extern "C" {
    /// The platform's property reader, which every Android libc exports.
    fn __system_property_get(name: *const std::ffi::c_char, value: *mut std::ffi::c_char) -> i32;
}

/// The renderer to try when the one asked for could not open a window.
fn other(renderer: eframe::Renderer) -> eframe::Renderer {
    match renderer {
        eframe::Renderer::Glow => eframe::Renderer::Wgpu,
        eframe::Renderer::Wgpu => eframe::Renderer::Glow,
    }
}

/// One attempt at the window, with the renderer `options` names.
fn open(options: NativeOptions, handed_over: Option<DroppedFile>) -> eframe::Result {
    eframe::run_native(
        "sOFD Reader",
        options,
        Box::new(move |cc| {
            // The system face carries the Chinese labels, as on the desktop.
            crate::fonts::install_cjk_fallback(&cc.egui_ctx);
            let mut app = App::new();
            if let Some(dropped) = handed_over {
                // Stored rather than opened: the first frame is the first place
                // a failure has a status bar to be written into.
                app.hand_over(dropped);
            }
            Ok(Box::new(Window(app)))
        }),
    )
}

/// The frame around [`window::draw`], the same one the desktop runs.
///
/// The binary's copy serves the platform dialogs as well; a device has none, so
/// this one draws and closes and asks nothing of the platform.
struct Window(App);

impl eframe::App for Window {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        // Asked before anything is drawn, because the window's whole layout is
        // stacked from its top-left corner: a header measured before the bars
        // are known would be drawn under the clock for one frame each rotation.
        if let Some(window) = frame.winit_window() {
            let scale = ui.ctx().pixels_per_point();
            window::remember_safe_area(ui.ctx(), window, scale);
        }
        let requests = window::draw(ui, &mut self.0);
        let title = self.0.title();
        ui.ctx()
            .send_viewport_cmd(egui::ViewportCommand::Title(title));
        if requests.quit {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

/// The document the activity was started for, if it was started for one.
///
/// A tap arrives as `ACTION_VIEW` with a content URI, a share as `ACTION_SEND`
/// with the same, and the bytes are read now rather than kept as a path: the
/// sandbox behind the URI is gone once the activity leaves the front. Every
/// failure is logged and dropped: refusing to start would be worse than empty.
fn document_from_intent(app: &android_activity::AndroidApp) -> Option<DroppedFile> {
    match read_shared_document(app) {
        Ok(Some(document)) => Some(document),
        Ok(None) => {
            log::info!("the activity was started without a document");
            None
        }
        Err(err) => {
            log::error!("the document the activity was started for was not read: {err}");
            None
        }
    }
}

/// The read itself, separated so its failures can be reported in one place.
fn read_shared_document(
    app: &android_activity::AndroidApp,
) -> Result<Option<DroppedFile>, Box<dyn std::error::Error>> {
    // SAFETY: `android-activity` owns both pointers for as long as the
    // `AndroidApp` is alive, and it is borrowed for the whole of this call. The
    // VM outlives every local reference taken through it.
    let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) };
    // The callback form rather than a borrowed `Env`: the guards it makes live
    // inside the closure's stack frames, so everything read here is released
    // by the time this returns.
    vm.attach_current_thread(|env| read_intent(env, app.activity_as_ptr()))
}

/// The read, with an `Env` in hand.
fn read_intent(
    env: &mut Env<'_>,
    activity_ptr: *mut std::ffi::c_void,
) -> Result<Option<DroppedFile>, Box<dyn std::error::Error>> {
    // SAFETY: the activity pointer is a local reference owned by the app
    // object, used before `android_main` returns.
    let activity = unsafe { JObject::from_raw(env, activity_ptr.cast()) };

    let intent = env
        .call_method(
            &activity,
            jni_str!("getIntent"),
            jni_sig!(() -> android.content.Intent),
            &[],
        )?
        .l()?;
    if intent.is_null() {
        return Ok(None);
    }
    let Some(uri) = intent_data(env, &intent)? else {
        return Ok(None);
    };

    let bytes = read_uri(env, &activity, &uri)?;
    // `open_bytes` reads the document rather than trusting the name, so an
    // unhelpful provider name costs nothing but a label.
    let name = display_name(env, &activity, &uri).unwrap_or_else(|| "document.ofd".to_owned());
    log::info!("read {name} from the intent, {} bytes", bytes.len());
    Ok(Some(DroppedFile::Bytes { name, bytes }))
}

/// The URI an intent carries, whether it is a `VIEW` data URI or a `SEND`
/// extra. `None` when the intent was the launcher's, which has neither.
///
/// The locals are attached to the envelope's lifetime rather than to the
/// `Env` borrow, so the URI outlives the call that made it.
fn intent_data<'local>(
    env: &mut Env<'local>,
    intent: &JObject<'local>,
) -> Result<Option<JObject<'local>>, Box<dyn std::error::Error>> {
    let data = env
        .call_method(
            intent,
            jni_str!("getData"),
            jni_sig!(() -> android.net.Uri),
            &[],
        )?
        .l()?;
    if !data.is_null() {
        return Ok(Some(data));
    }
    // `ACTION_SEND` puts the item in `EXTRA_STREAM` instead of the data field,
    // so a share from a file manager would otherwise open an empty window.
    let extra = env.new_string("android.intent.extra.STREAM")?;
    let sent = env
        .call_method(
            intent,
            jni_str!("getParcelableExtra"),
            jni_sig!((name: java.lang.String) -> android.os.Parcelable),
            &[(&extra).into()],
        )?
        .l()?;
    if sent.is_null() {
        return Ok(None);
    }
    Ok(Some(sent))
}

/// The bytes behind a content URI, read through the activity's resolver.
fn read_uri(
    env: &mut Env<'_>,
    activity: &JObject<'_>,
    uri: &JObject<'_>,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let resolver = env
        .call_method(
            activity,
            jni_str!("getContentResolver"),
            jni_sig!(() -> android.content.ContentResolver),
            &[],
        )?
        .l()?;
    let stream = env
        .call_method(
            &resolver,
            jni_str!("openInputStream"),
            jni_sig!((uri: android.net.Uri) -> java.io.InputStream),
            &[uri.into()],
        )?
        .l()?;
    if stream.is_null() {
        return Err("the provider would not open the document".into());
    }

    // Read in chunks into a buffer rather than asking the stream for its size:
    // a provider is free to report none, and the length of a shared document is
    // not a number a reader should trust in advance.
    const CHUNK: usize = 64 * 1024;
    let mut bytes = Vec::new();
    loop {
        // A fresh array per read, because the provider writes into this one and
        // the bytes have to be taken back out of it: appending from the Rust
        // side alone would append the zeroes it was filled with.
        let buffer = env.new_byte_array(CHUNK)?;
        let read = env
            .call_method(
                &stream,
                jni_str!("read"),
                jni_sig!((buffer: byte[]) -> int),
                &[(&buffer).into()],
            )?
            .i()?;
        if read <= 0 {
            break;
        }
        let taken = usize::try_from(read)?.min(CHUNK);
        // `jbyte` is signed, and a document is a stream of bytes: the values are
        // reinterpreted rather than converted, so a byte above 0x7f keeps its bits.
        let mut filled = vec![0i8; taken];
        buffer.get_region(env, 0, &mut filled)?;
        bytes.extend(filled.into_iter().map(|signed| signed as u8));
    }
    // The stream owns a file descriptor, and a reader that closed the document
    // without closing this would hold the provider open until the app exits.
    let _ = env.call_method(&stream, jni_str!("close"), jni_sig!(() -> void), &[]);
    Ok(bytes)
}

/// What the provider calls the document, for the status bar. A provider is
/// free to answer `null` or throw, and either way the document is still read.
fn display_name(env: &mut Env<'_>, activity: &JObject<'_>, uri: &JObject<'_>) -> Option<String> {
    let resolver = env
        .call_method(
            activity,
            jni_str!("getContentResolver"),
            jni_sig!(() -> android.content.ContentResolver),
            &[],
        )
        .ok()?
        .l()
        .ok()?;
    let cursor = env
        .call_method(
            &resolver,
            jni_str!("query"),
            jni_sig!(
                (
                    uri: android.net.Uri,
                    projection: byte[],
                    selection: java.lang.String,
                    selection_args: byte[],
                    sort_order: java.lang.String
                ) -> android.database.Cursor
            ),
            &[
                uri.into(),
                (&JObject::null()).into(),
                (&JObject::null()).into(),
                (&JObject::null()).into(),
                (&JObject::null()).into(),
            ],
        )
        .ok()?
        .l()
        .ok()?;
    if cursor.is_null() {
        return None;
    }
    let column = env.new_string("_display_name").ok()?;
    let index = env
        .call_method(
            &cursor,
            jni_str!("getColumnIndex"),
            jni_sig!((name: java.lang.String) -> int),
            &[(&column).into()],
        )
        .ok()?
        .i()
        .ok()?;
    let moved = index >= 0
        && env
            .call_method(
                &cursor,
                jni_str!("moveToFirst"),
                jni_sig!(() -> boolean),
                &[],
            )
            .ok()?
            .z()
            .ok()?;
    let name = if moved {
        let value = env
            .call_method(
                &cursor,
                jni_str!("getString"),
                jni_sig!((column: int) -> java.lang.String),
                &[index.into()],
            )
            .ok()?
            .l()
            .ok()?;
        if value.is_null() {
            None
        } else {
            let value = JString::cast_local(env, value).ok()?;
            value.try_to_string(env).ok()
        }
    } else {
        None
    };
    let _ = env.call_method(&cursor, jni_str!("close"), jni_sig!(() -> void), &[]);
    name.filter(|name| !name.is_empty())
}

/// The library version the Java VM is told about when the library is loaded.
///
/// # Safety
///
/// The Java runtime calls this with the VM it runs in. The pointer is stored
/// and cloned, never read through.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn JNI_OnLoad(
    vm: *mut jni::sys::JavaVM,
    _reserved: *mut std::ffi::c_void,
) -> jni::sys::jint {
    if !vm.is_null() {
        // SAFETY: the VM outlives every thread in this process, and the
        // pointer is the one the runtime passed in.
        let _ = VM.set(unsafe { JavaVM::from_raw(vm) });
    }
    // Written straight to the log rather than through `log`: this runs before
    // the entry point installs the sink, so a record here would go nowhere.
    write_to_logcat(log::Level::Info, "the library was loaded");
    jni::sys::JNI_VERSION_1_6
}

/// Which thread a line was written on. A panic names a thread only when its
/// hook prints, and the activity's and the reader's threads are different.
fn thread_name() -> String {
    std::thread::current()
        .name()
        .map_or_else(|| "unnamed".to_owned(), str::to_owned)
}
