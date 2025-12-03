use egui::Color32;
use qlog::events::quic::QuicFrame;

/// QUIC frame types for display and coloring.
#[derive(Debug, Clone, PartialEq)]
pub enum FrameType {
    Padding,
    Ping,
    Ack,
    ResetStream,
    StopSending,
    Crypto,
    NewToken,
    Stream(u64),
    MaxData,
    MaxStreamData,
    MaxStreams,
    DataBlocked,
    StreamDataBlocked,
    StreamsBlocked,
    NewConnectionId,
    RetireConnectionId,
    PathChallenge,
    PathResponse,
    PathAck,
    PathAbandon,
    PathStatusAvailable,
    PathStatusBackup,
    PathNewConnectionId,
    PathRetireConnectionId,
    PathsBlocked,
    PathCidsBlocked,
    MaxPathId,
    ConnectionClose,
    HandshakeDone,
    Datagram,
    Custom(String),
}

impl FrameType {
    /// Convert from qlog QuicFrame to our FrameType.
    pub fn from_quic_frame(frame: &QuicFrame) -> Self {
        match frame {
            QuicFrame::Padding { .. } => Self::Padding,
            QuicFrame::Ping { .. } => Self::Ping,
            QuicFrame::Ack { .. } => Self::Ack,
            QuicFrame::ResetStream { .. } => Self::ResetStream,
            QuicFrame::StopSending { .. } => Self::StopSending,
            QuicFrame::Crypto { .. } => Self::Crypto,
            QuicFrame::NewToken { .. } => Self::NewToken,
            QuicFrame::Stream { stream_id, .. } => Self::Stream(*stream_id),
            QuicFrame::MaxData { .. } => Self::MaxData,
            QuicFrame::MaxStreamData { .. } => Self::MaxStreamData,
            QuicFrame::MaxStreams { .. } => Self::MaxStreams,
            QuicFrame::DataBlocked { .. } => Self::DataBlocked,
            QuicFrame::StreamDataBlocked { .. } => Self::StreamDataBlocked,
            QuicFrame::StreamsBlocked { .. } => Self::StreamsBlocked,
            QuicFrame::NewConnectionId { .. } => Self::NewConnectionId,
            QuicFrame::RetireConnectionId { .. } => Self::RetireConnectionId,
            QuicFrame::PathChallenge { .. } => Self::PathChallenge,
            QuicFrame::PathResponse { .. } => Self::PathResponse,
            QuicFrame::PathAck { .. } => Self::PathAck,
            QuicFrame::PathAbandon { .. } => Self::PathAbandon,
            QuicFrame::PathStatusAvailable { .. } => Self::PathStatusAvailable,
            QuicFrame::PathStatusBackup { .. } => Self::PathStatusBackup,
            QuicFrame::PathNewConnectionId { .. } => Self::PathNewConnectionId,
            QuicFrame::PathRetireConnectionId { .. } => Self::PathRetireConnectionId,
            QuicFrame::PathsBlocked { .. } => Self::PathsBlocked,
            QuicFrame::PathCidsBlocked { .. } => Self::PathCidsBlocked,
            QuicFrame::MaxPathId { .. } => Self::MaxPathId,
            QuicFrame::ConnectionClose { .. } => Self::ConnectionClose,
            QuicFrame::HandshakeDone { .. } => Self::HandshakeDone,
            QuicFrame::Datagram { .. } => Self::Datagram,
            QuicFrame::Unknown {
                frame_type_bytes,
                raw,
                ..
            } => {
                let name = raw
                    .as_ref()
                    .and_then(|r| r.data.clone())
                    .unwrap_or_else(|| {
                        frame_type_bytes
                            .map(|ftb| format!("0x{:X}", ftb))
                            .unwrap_or_else(|| "Unknown".to_string())
                    });
                Self::Custom(name)
            }
        }
    }

    /// Full display name for the frame type.
    pub fn display_name(&self) -> String {
        match self {
            Self::Padding => "PADDING".to_string(),
            Self::Ping => "PING".to_string(),
            Self::Ack => "ACK".to_string(),
            Self::ResetStream => "RESET_STREAM".to_string(),
            Self::StopSending => "STOP_SENDING".to_string(),
            Self::Crypto => "CRYPTO".to_string(),
            Self::NewToken => "NEW_TOKEN".to_string(),
            Self::Stream(id) => format!("STREAM({})", id),
            Self::MaxData => "MAX_DATA".to_string(),
            Self::MaxStreamData => "MAX_STREAM_DATA".to_string(),
            Self::MaxStreams => "MAX_STREAMS".to_string(),
            Self::DataBlocked => "DATA_BLOCKED".to_string(),
            Self::StreamDataBlocked => "STREAM_DATA_BLOCKED".to_string(),
            Self::StreamsBlocked => "STREAMS_BLOCKED".to_string(),
            Self::NewConnectionId => "NEW_CONNECTION_ID".to_string(),
            Self::RetireConnectionId => "RETIRE_CONNECTION_ID".to_string(),
            Self::PathChallenge => "PATH_CHALLENGE".to_string(),
            Self::PathResponse => "PATH_RESPONSE".to_string(),
            Self::PathAck => "PATH_ACK".to_string(),
            Self::PathAbandon => "PATH_ABANDON".to_string(),
            Self::PathStatusAvailable => "PATH_STATUS_AVAILABLE".to_string(),
            Self::PathStatusBackup => "PATH_STATUS_BACKUP".to_string(),
            Self::PathNewConnectionId => "PATH_NEW_CONNECTION_ID".to_string(),
            Self::PathRetireConnectionId => "PATH_RETIRE_CONNECTION_ID".to_string(),
            Self::PathsBlocked => "PATHS_BLOCKED".to_string(),
            Self::PathCidsBlocked => "PATH_CIDS_BLOCKED".to_string(),
            Self::MaxPathId => "MAX_PATH_ID".to_string(),
            Self::ConnectionClose => "CONNECTION_CLOSE".to_string(),
            Self::HandshakeDone => "HANDSHAKE_DONE".to_string(),
            Self::Datagram => "DATAGRAM".to_string(),
            Self::Custom(name) => name.to_uppercase(),
        }
    }

    /// Short 3-5 character name for compact display.
    pub fn short_name(&self) -> String {
        match self {
            Self::Padding => "PAD".to_string(),
            Self::Ping => "PNG".to_string(),
            Self::Ack => "ACK".to_string(),
            Self::ResetStream => "RST".to_string(),
            Self::StopSending => "STP".to_string(),
            Self::Crypto => "CRY".to_string(),
            Self::NewToken => "TOK".to_string(),
            Self::Stream(id) => format!("S{}", id),
            Self::MaxData => "MXD".to_string(),
            Self::MaxStreamData | Self::MaxStreams => "MXS".to_string(),
            Self::DataBlocked => "BLK".to_string(),
            Self::StreamDataBlocked | Self::StreamsBlocked => "SBK".to_string(),
            Self::NewConnectionId => "NCI".to_string(),
            Self::RetireConnectionId => "RCI".to_string(),
            Self::PathChallenge => "PCH".to_string(),
            Self::PathResponse => "PRS".to_string(),
            Self::PathAck => "PAK".to_string(),
            Self::PathAbandon => "PAB".to_string(),
            Self::PathStatusAvailable => "PSA".to_string(),
            Self::PathStatusBackup => "PSB".to_string(),
            Self::PathNewConnectionId => "PNC".to_string(),
            Self::PathRetireConnectionId => "PRC".to_string(),
            Self::PathsBlocked => "PBK".to_string(),
            Self::PathCidsBlocked => "PCB".to_string(),
            Self::MaxPathId => "MPI".to_string(),
            Self::ConnectionClose => "CLS".to_string(),
            Self::HandshakeDone => "HSD".to_string(),
            Self::Datagram => "DGM".to_string(),
            Self::Custom(name) => derive_short_name(name),
        }
    }

    /// Color for rendering this frame type.
    pub fn color(&self) -> Color32 {
        match self {
            Self::Stream(_) => Color32::from_rgb(255, 80, 80),
            Self::Crypto => Color32::from_rgb(128, 0, 128),
            Self::Ack => Color32::from_rgb(0, 128, 0),
            Self::Padding => Color32::from_rgb(255, 165, 0),
            Self::MaxData | Self::MaxStreamData | Self::MaxStreams => {
                Color32::from_rgb(100, 150, 200)
            }
            Self::HandshakeDone => Color32::from_rgb(50, 150, 50),
            Self::NewConnectionId | Self::RetireConnectionId => Color32::from_rgb(200, 100, 50),
            Self::ConnectionClose => Color32::from_rgb(255, 100, 100),
            _ => Color32::from_rgb(100, 100, 100),
        }
    }

    /// Alternative color scheme for packetization diagram.
    pub fn packetization_color(&self) -> Color32 {
        match self {
            Self::Stream(_) => Color32::from_rgb(255, 80, 80),
            Self::Crypto => Color32::from_rgb(255, 150, 150),
            Self::Ack => Color32::from_rgb(180, 180, 180),
            Self::Padding => Color32::from_rgb(100, 100, 100),
            Self::MaxData | Self::MaxStreamData => Color32::from_rgb(150, 200, 255),
            Self::HandshakeDone => Color32::from_rgb(150, 255, 150),
            Self::ConnectionClose => Color32::from_rgb(255, 100, 100),
            _ => Color32::from_rgb(200, 200, 200),
        }
    }

    /// Check if this frame type is a stream frame.
    pub fn is_stream(&self) -> bool {
        matches!(self, Self::Stream(_))
    }
}

fn derive_short_name(name: &str) -> String {
    if name.starts_with("0x") {
        return name.chars().take(5).collect();
    }

    let parts: Vec<&str> = name
        .split(['_', '-', ' '])
        .filter(|s| !s.is_empty())
        .collect();

    let result: String = match parts.len() {
        0 => name.chars().take(5).collect(),
        1 => name
            .chars()
            .filter(|c| c.is_alphanumeric())
            .take(5)
            .collect(),
        _ => parts
            .iter()
            .filter_map(|p| p.chars().next())
            .take(5)
            .collect(),
    };

    result.to_uppercase()
}

/// Convert HSV color values to RGB.
pub fn hsv_to_rgb(h: f32, s: f32, v: f32) -> (u8, u8, u8) {
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = v - c;

    let (r, g, b) = match (h / 60.0) as i32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };

    (
        ((r + m) * 255.0) as u8,
        ((g + m) * 255.0) as u8,
        ((b + m) * 255.0) as u8,
    )
}

/// Format bytes as human-readable string using binary units (B, KiB, MiB, GiB).
pub fn format_bytes(bytes: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = 1024 * 1024;
    const GIB: u64 = 1024 * 1024 * 1024;

    if bytes >= GIB {
        format!("{:.2} GiB", bytes as f64 / GIB as f64)
    } else if bytes >= MIB {
        format!("{:.2} MiB", bytes as f64 / MIB as f64)
    } else if bytes >= KIB {
        format!("{:.2} KiB", bytes as f64 / KIB as f64)
    } else {
        format!("{} B", bytes)
    }
}

/// Generate a distinct color for a stream ID using golden ratio distribution.
pub fn stream_color(stream_id: u64) -> Color32 {
    let hue = (stream_id as f32 * 137.5) % 360.0;
    let (r, g, b) = hsv_to_rgb(hue, 0.7, 0.9);
    Color32::from_rgb(r, g, b)
}

/// Extract stream ID from a QUIC frame if applicable.
pub fn get_frame_stream_id(frame: &QuicFrame) -> Option<u64> {
    match frame {
        QuicFrame::Stream { stream_id, .. }
        | QuicFrame::MaxStreamData { stream_id, .. }
        | QuicFrame::StreamDataBlocked { stream_id, .. }
        | QuicFrame::ResetStream { stream_id, .. }
        | QuicFrame::StopSending { stream_id, .. } => Some(*stream_id),
        _ => None,
    }
}

/// Get frame size from a QUIC frame if applicable.
pub fn get_frame_size(frame: &QuicFrame) -> Option<u64> {
    match frame {
        QuicFrame::Stream { raw, .. }
        | QuicFrame::Crypto { raw, .. }
        | QuicFrame::Datagram { raw, .. } => raw.as_ref().and_then(|r| r.length),
        _ => None,
    }
}

/// Convert raw packet type enum to full human-readable name.
pub fn full_packet_type(pkt_type: &str) -> String {
    match pkt_type {
        "Initial" => "Initial".to_string(),
        "Handshake" => "Handshake".to_string(),
        "ZeroRtt" => "0-RTT".to_string(),
        "OneRtt" => "1-RTT".to_string(),
        s => s.to_string(),
    }
}

/// Convert packet type to short display name.
pub fn short_packet_type(pkt_type: &str) -> String {
    match pkt_type {
        "Initial" => "I".to_string(),
        "Handshake" => "H".to_string(),
        "ZeroRtt" => "0R".to_string(),
        "OneRtt" => "1R".to_string(),
        s => s.chars().take(2).collect(),
    }
}
