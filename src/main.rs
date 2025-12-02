mod app;
mod congestion_graph;
mod multiplexing_diagram;
mod packet_correlation;
mod packetization_diagram;
mod qlog_data;
mod sequence_diagram;
mod stats_view;
mod utils;

use app::QlogViewerApp;
use clap::Parser;
use std::path::PathBuf;
use tracing::{error, info};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

#[derive(Parser, Debug)]
#[command(name = "n0qlog")]
#[command(about = "A GUI viewer for qlog files to visualize and debug QUIC connections", long_about = None)]
struct Args {
    #[arg(help = "One or more qlog files or directories to load on startup")]
    paths: Vec<PathBuf>,
}

fn collect_qlog_files(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut qlog_files = Vec::new();

    for path in paths {
        if path.is_file() {
            qlog_files.push(path);
        } else if path.is_dir() {
            info!("Scanning directory: {:?}", path);
            match std::fs::read_dir(&path) {
                Ok(entries) => {
                    for entry in entries.flatten() {
                        let entry_path = entry.path();
                        if entry_path.is_file() {
                            if let Some(ext) = entry_path.extension() {
                                let ext_str = ext.to_string_lossy().to_lowercase();
                                if ext_str == "qlog" || ext_str == "json" || ext_str == "sqlog" {
                                    info!("Found qlog file: {:?}", entry_path);
                                    qlog_files.push(entry_path);
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    error!("Failed to read directory {:?}: {}", path, e);
                }
            }
        } else {
            error!("Path does not exist or is not accessible: {:?}", path);
        }
    }

    qlog_files
}

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

    let args = Args::parse();
    let qlog_files = collect_qlog_files(args.paths);

    if !qlog_files.is_empty() {
        info!("Loading {} qlog file(s)", qlog_files.len());
    }

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
        Box::new(move |cc| Ok(Box::new(QlogViewerApp::new(cc, qlog_files)))),
    )
}
