mod app;
mod congestion_graph;
mod multiplexing_diagram;
mod packetization_diagram;
mod qlog_data;
mod sequence_diagram;
mod utils;

use app::QlogViewerApp;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

fn load_icon() -> egui::IconData {
    let icon_data = include_bytes!("../assets/icon.png");
    let image = image::load_from_memory(icon_data)
        .expect("Failed to load icon")
        .into_rgba8();
    let (width, height) = image.dimensions();
    egui::IconData {
        rgba: image.into_raw(),
        width,
        height,
    }
}

fn main() -> eframe::Result<()> {
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::from_default_env())
        .init();

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1400.0, 900.0])
            .with_min_inner_size([800.0, 600.0])
            .with_icon(load_icon()),
        ..Default::default()
    };

    eframe::run_native(
        "n0qlog",
        native_options,
        Box::new(|cc| Ok(Box::new(QlogViewerApp::new(cc)))),
    )
}
