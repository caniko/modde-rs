#![allow(clippy::wildcard_imports)]
use super::*;

#[cfg(unix)]
pub(super) fn external_refresh_stream() -> impl iced::futures::Stream<Item = Message> {
    use iced::futures::SinkExt as _;
    use tokio::io::AsyncReadExt as _;

    iced::stream::channel(8, async move |mut output| {
        // Per-process socket path: `modde-${euid}-${pid}.sock`. Each
        // GUI gets its own; the CLI fan-outs to every matching file
        // in `$XDG_RUNTIME_DIR`, so multi-window installs all see
        // every change.
        let path = modde_core::ipc::gui_socket_path();
        // Pid collisions are essentially impossible inside a single
        // boot, but be defensive: if a previous run of *this exact
        // pid* (rare; happens with pid wraparound on long-uptime
        // systems) left a node behind, unlink it.
        let _ = std::fs::remove_file(&path);

        let listener = match tokio::net::UnixListener::bind(&path) {
            Ok(l) => l,
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    socket = %path.display(),
                    "could not bind refresh socket; CLI → GUI live updates disabled \
                     for this window"
                );
                return;
            }
        };

        // Best-effort cleanup at process exit so the CLI's GC pass
        // doesn't have to do it. Held in a guard captured by the
        // listening task: dropped when the subscription stream is
        // torn down (window close / app shutdown), which unlinks
        // the socket. If the process panics the file leaks, but the
        // CLI side GCs unreachable sockets on every notify pass.
        let _guard = SocketGuard::new(path.clone());

        tracing::info!(socket = %path.display(), "listening for CLI refresh signals");

        loop {
            let (mut stream, _addr) = match listener.accept().await {
                Ok(p) => p,
                Err(e) => {
                    tracing::warn!(error = %e, "accept failed; restarting listen loop");
                    continue;
                }
            };
            // Drain whatever the peer sent so the kernel buffer is
            // freed; we don't actually parse the payload — the
            // existence of the connection is the signal.
            let mut buf = [0u8; 64];
            let _ = stream.read(&mut buf).await;
            if output.send(Message::ExternalRefresh).await.is_err() {
                // Application is shutting down.
                break;
            }
        }
    })
}

#[cfg(all(windows, feature = "windows-integrations"))]
pub(super) fn external_refresh_stream() -> impl iced::futures::Stream<Item = Message> {
    use iced::futures::SinkExt as _;
    use tokio::io::AsyncReadExt as _;
    use tokio::net::windows::named_pipe::ServerOptions;

    iced::stream::channel(8, async move |mut output| {
        let marker_path = modde_core::ipc::gui_socket_path();
        if let Some(parent) = marker_path.parent()
            && let Err(e) = std::fs::create_dir_all(parent)
        {
            tracing::warn!(
                error = %e,
                path = %parent.display(),
                "could not create Windows refresh marker directory; CLI → GUI live updates disabled"
            );
            return;
        }

        let pipe_name = modde_core::ipc::gui_pipe_name();
        if let Err(e) = std::fs::write(&marker_path, &pipe_name) {
            tracing::warn!(
                error = %e,
                marker = %marker_path.display(),
                "could not write Windows refresh marker; CLI → GUI live updates disabled"
            );
            return;
        }

        let _guard = SocketGuard::new(marker_path.clone());
        tracing::info!(pipe = %pipe_name, marker = %marker_path.display(), "listening for CLI refresh signals");

        loop {
            let mut server = match ServerOptions::new().create(&pipe_name) {
                Ok(server) => server,
                Err(e) => {
                    tracing::warn!(
                        error = %e,
                        pipe = %pipe_name,
                        "could not create Windows refresh pipe; CLI → GUI live updates disabled"
                    );
                    return;
                }
            };

            if let Err(e) = server.connect().await {
                tracing::warn!(error = %e, "Windows refresh pipe accept failed; restarting listen loop");
                continue;
            }

            let mut buf = [0u8; 64];
            let _ = server.read(&mut buf).await;
            if output.send(Message::ExternalRefresh).await.is_err() {
                break;
            }
        }
    })
}

#[cfg(not(any(unix, all(windows, feature = "windows-integrations"))))]
pub(super) fn external_refresh_stream() -> impl iced::futures::Stream<Item = Message> {
    iced::stream::channel(1, |_output| async move {
        std::future::pending::<()>().await;
    })
}

/// Drop guard that unlinks a listener socket/marker when the listening task ends.
#[cfg(any(unix, all(windows, feature = "windows-integrations")))]
struct SocketGuard {
    path: std::path::PathBuf,
}

#[cfg(any(unix, all(windows, feature = "windows-integrations")))]
impl SocketGuard {
    fn new(path: std::path::PathBuf) -> Self {
        Self { path }
    }
}

#[cfg(any(unix, all(windows, feature = "windows-integrations")))]
impl Drop for SocketGuard {
    fn drop(&mut self) {
        modde_core::ipc::cleanup_socket(&self.path);
    }
}

/// Max thumbnail dimensions — matches the sidebar image slot
/// (~166 px wide, 96 px tall).  We decode the fetched bytes, scale
/// down with a Lanczos3 filter, and hand the smaller RGBA buffer to
/// iced so it doesn't keep a multi-megapixel texture around.
const THUMB_MAX_W: u32 = 340;
const THUMB_MAX_H: u32 = 192;

pub(super) fn resize_thumbnail_bytes(raw: &[u8]) -> iced::widget::image::Handle {
    let Ok(img) = image::load_from_memory(raw) else {
        // If decoding fails, fall back to letting iced try the raw bytes.
        return iced::widget::image::Handle::from_bytes(raw.to_vec());
    };

    let resized = img.resize(
        THUMB_MAX_W,
        THUMB_MAX_H,
        image::imageops::FilterType::Lanczos3,
    );
    let rgba = resized.to_rgba8();
    let (w, h) = rgba.dimensions();
    iced::widget::image::Handle::from_rgba(w, h, rgba.into_raw())
}

/// Run the iced application.
pub fn run() -> iced::Result {
    iced::application(Modde::new, Modde::update, Modde::view)
        .title(Modde::title)
        .theme(Modde::theme)
        .subscription(Modde::subscription)
        .window(window_settings())
        .run()
}

fn window_settings() -> window::Settings {
    window::Settings {
        decorations: false,
        icon: Some(app_icon()),
        ..window::Settings::default()
    }
}

fn app_icon() -> window::Icon {
    window::icon::from_file_data(
        include_bytes!("../../../../dist/com.tartanoglu.modde.png"),
        Some(image::ImageFormat::Png),
    )
    .expect("embedded modde app icon must be a valid PNG")
}
