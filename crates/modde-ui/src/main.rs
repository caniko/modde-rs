const DEFAULT_LOG_FILTER: &str = concat!(
    "modde_ui=debug,",
    "modde_core=debug,",
    "modde_games=debug,",
    "modde_sources=debug,",
    "modde=debug,",
    "iced=warn,",
    "iced_wgpu=warn,",
    "wgpu=warn,",
    "naga=warn"
);

fn main() -> iced::Result {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(DEFAULT_LOG_FILTER));
    match modde_core::library::diagnostics::gui_log() {
        Ok(log) => tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_ansi(false)
            .with_writer(std::sync::Mutex::new(log))
            .init(),
        Err(error) => {
            eprintln!("Could not open persistent GUI log: {error}");
            tracing_subscriber::fmt()
                .with_env_filter(filter)
                .with_writer(std::io::stderr)
                .init();
        }
    }

    tracing::info!("starting modde-ui");

    modde_ui::app::run()
}
