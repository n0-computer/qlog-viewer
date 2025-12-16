use egui::Color32;

#[derive(Debug, Clone)]
pub struct LastMetricsState {
    pub smoothed_rtt: Option<f32>,
    pub bytes_in_flight: Option<u64>,
    pub congestion_window: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PathKey {
    pub file_idx: usize,
    pub path_id: u64,
}

#[derive(Debug, Clone, Default)]
pub struct PathMetricData {
    pub bytes_in_flight: Vec<(f64, u64)>,
    pub rtt: Vec<(f64, f32)>,
}

/// Cached global min/max values for metric graphs
#[derive(Debug, Clone, Default)]
pub struct MetricsBounds {
    pub bif_min: u64,
    pub bif_max: u64,
    pub rtt_min: f32,
    pub rtt_max: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetricsDisplayStyle {
    Box,
    Marker,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetricsVisualizationMode {
    Events,
    Graphs,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MetricsEventType {
    ConnectionStarted,
    ConnectionStateUpdated,
    MetricsUpdated,
    CongestionStateUpdated,
    PacketLost,
    TimerUpdated,
    Other(String),
}

impl MetricsEventType {
    pub fn color(&self) -> Color32 {
        match self {
            Self::ConnectionStarted => Color32::from_rgb(0, 150, 136),
            Self::ConnectionStateUpdated => Color32::from_rgb(0, 150, 136),
            Self::MetricsUpdated => Color32::from_rgb(142, 68, 173),
            Self::CongestionStateUpdated => Color32::from_rgb(255, 152, 0),
            Self::PacketLost => Color32::from_rgb(244, 67, 54),
            Self::TimerUpdated => Color32::from_rgb(158, 158, 158),
            Self::Other(_) => Color32::from_rgb(158, 158, 158),
        }
    }

    pub fn name(&self) -> String {
        match self {
            Self::ConnectionStarted => "Connection Started".to_string(),
            Self::ConnectionStateUpdated => "Connection State".to_string(),
            Self::MetricsUpdated => "Metrics Updated".to_string(),
            Self::CongestionStateUpdated => "Congestion State".to_string(),
            Self::PacketLost => "Packet Lost".to_string(),
            Self::TimerUpdated => "Timer Updated".to_string(),
            Self::Other(name) => name.clone(),
        }
    }

    pub fn display_style(&self) -> MetricsDisplayStyle {
        MetricsDisplayStyle::Box
    }
}

#[derive(Debug, Clone)]
pub struct MetricsEvent {
    pub time: f64,
    pub event_type: MetricsEventType,
    pub display_text: String,
    pub detail_text: String,
    pub color: Color32,
    pub event_idx: usize,
    pub display_style: MetricsDisplayStyle,
    pub file_idx: usize,
    pub path_id: Option<u64>,
    pub smoothed_rtt: Option<f32>,
    pub bytes_in_flight: Option<u64>,
}
