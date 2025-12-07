use crate::utils::FrameType;
use egui::epaint::Hsva;
use egui::{Color32, Rect};
use qlog::events::TupleEndpointInfo;
use std::collections::HashMap;
use std::sync::Arc;

pub const PATH_COLORS: &[(u8, u8, u8)] = &[
    (41, 128, 185), // Blue (path 0)
    (39, 174, 96),  // Green (path 1)
    (230, 126, 34), // Orange (path 2)
    (142, 68, 173), // Purple (path 3)
    (241, 196, 15), // Yellow (path 4)
    (231, 76, 60),  // Red (path 5)
];

pub fn path_color(path_id: u64) -> Color32 {
    if let Some(&(r, g, b)) = PATH_COLORS.get(path_id as usize) {
        Color32::from_rgb(r, g, b)
    } else {
        let hue = ((path_id as f32 * 137.508) % 360.0) / 360.0;
        hsv_to_rgb(hue, 0.7, 0.8)
    }
}

pub fn hsv_to_rgb(h: f32, s: f32, v: f32) -> Color32 {
    Hsva::new(h, s, v, 1.0).into()
}

#[derive(Clone)]
pub struct DiagramLayout {
    pub left_x: f32,
    pub right_x: f32,
    pub top_y: f32,
    pub rect: Rect,
}

pub struct PathLaneLayout {
    pub left_lanes: HashMap<u64, f32>,
    pub right_lanes: HashMap<u64, f32>,
    #[allow(dead_code)]
    pub center_x: f32,
    pub use_lanes: bool,
}

pub struct DualArrow {
    pub packet: Arc<PacketInfo>,
    pub send_time: f64,
    pub recv_time: f64,
    pub from_left: bool,
    pub is_lost: bool,
    pub sent_event_idx: usize,
    pub recv_event_idx: Option<usize>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PacketDirection {
    Sent,
    Received,
}

pub struct PacketInfo {
    pub time: f64,
    pub direction: PacketDirection,
    pub packet_type: String,
    pub packet_type_short: String,
    pub packet_number: u64,
    pub path_id: Option<u64>,
    pub tuple_id: Option<String>,
    pub frames: Vec<FrameType>,
    pub event_idx: usize,
}

#[derive(Default)]
pub struct ExtractedPackets {
    pub packets: Vec<Arc<PacketInfo>>,
    pub packet_types: std::collections::BTreeSet<String>,
}

#[derive(Clone, Debug)]
pub struct TupleInfo {
    pub tuple_id: String,
    pub remote_addr: Option<String>,
    pub local_addr: Option<String>,
}

impl TupleInfo {
    #[allow(dead_code)]
    pub fn remote_display(&self) -> String {
        self.remote_addr
            .clone()
            .unwrap_or_else(|| "unknown".to_string())
    }

    #[allow(dead_code)]
    pub fn is_relayed(&self) -> bool {
        if let Some(ref addr) = self.remote_addr {
            addr.ends_with(":12345")
        } else {
            false
        }
    }

    pub fn from_tuple_assigned(
        tuple_id: String,
        remote: Option<&TupleEndpointInfo>,
        local: Option<&TupleEndpointInfo>,
    ) -> Self {
        Self {
            tuple_id,
            remote_addr: remote.and_then(format_endpoint_info),
            local_addr: local.and_then(format_endpoint_info),
        }
    }
}

pub fn format_endpoint_info(info: &TupleEndpointInfo) -> Option<String> {
    if let (Some(ip), Some(port)) = (&info.ip_v4, info.port_v4) {
        Some(format!("{}:{}", ip, port))
    } else if let (Some(ip), Some(port)) = (&info.ip_v6, info.port_v6) {
        Some(format!("[{}]:{}", ip, port))
    } else if let Some(ip) = &info.ip_v4 {
        Some(ip.clone())
    } else {
        info.ip_v6.as_ref().map(|ip| format!("[{}]", ip))
    }
}

pub struct SelectedArrowRenderData {
    pub arrow_idx: usize,
    pub start_x: f32,
    pub y_send_adjusted: f32,
    pub actual_end_x: f32,
    pub actual_end_y: f32,
    pub is_truncated: bool,
    pub count: usize,
    pub idx: usize,
    pub y_send: f32,
}

pub struct ArrowInteractionData {
    pub arrow_idx: usize,
    pub sent_event_idx: usize,
    pub recv_event_idx: Option<usize>,
    pub from_left: bool,
    pub rect: Rect,
    pub line_start_x: f32,
    pub line_start_y: f32,
    pub line_end_x: f32,
    pub line_end_y: f32,
}
