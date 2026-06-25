use crate::utils::{self, FrameType};
use egui::text::LayoutJob;
use egui::{CollapsingHeader, FontFamily, FontId, RichText, TextFormat};
use qlog::events::quic::{PacketHeader, PacketType, QuicFrame};
use qlog::events::{EventData, RawInfo};

pub fn render_frame_with_prefix(
    ui: &mut egui::Ui,
    prefix: &str,
    event_id: usize,
    frame_id: usize,
    frame: &QuicFrame,
) {
    use QuicFrame::*;

    let ty = FrameType::from_quic_frame(frame);
    let mut heading = LayoutJob::default();
    heading.append(
        &format!("[{}]", ty.short_name()),
        0.,
        TextFormat {
            color: ty.color(),
            font_id: FontId::new(16.0, FontFamily::Proportional),
            ..Default::default()
        },
    );
    heading.append(
        &ty.display_name(),
        1.,
        TextFormat {
            font_id: FontId::new(16.0, FontFamily::Proportional),
            ..Default::default()
        },
    );

    CollapsingHeader::new(heading)
        .id_salt(format!(
            "{}-frame-{event_id}-{frame_id}-{}",
            prefix,
            ty.short_name()
        ))
        .show(ui, |ui| {
            egui::Grid::new(format!(
                "{}-frame-{event_id}-{frame_id}-{}",
                prefix,
                ty.short_name()
            ))
            .num_columns(2)
            .spacing([40.0, 4.0])
            .striped(true)
            .show(ui, |ui| match frame {
                Padding { raw } => {
                    if let Some(raw) = raw {
                        render_raw_info(ui, raw);
                    }
                }
                Ping { raw } => {
                    if let Some(raw) = raw {
                        render_raw_info(ui, raw);
                    }
                }
                Ack {
                    ack_delay,
                    acked_ranges,
                    ect1,
                    ect0,
                    ce,
                    raw,
                } => {
                    if let Some(ack_delay) = ack_delay {
                        ui.label("Ack Delay");
                        ui.label(format!("{ack_delay:?}"));
                        ui.end_row();
                    }
                    if let Some(acked_ranges) = acked_ranges {
                        ui.label("Acked Ranges");
                        ui.label(utils::format_acked_ranges(acked_ranges));
                        ui.end_row();
                    }
                    if let Some(ect0) = ect0 {
                        ui.label("ECT 0");
                        ui.label(format!("{ect0:?}"));
                        ui.end_row();
                    }
                    if let Some(ect1) = ect1 {
                        ui.label("ECT 1");
                        ui.label(format!("{ect1:?}"));
                        ui.end_row();
                    }
                    if let Some(ce) = ce {
                        ui.label("CE");
                        ui.label(format!("{ce:?}"));
                        ui.end_row();
                    }
                    if let Some(raw) = raw {
                        render_raw_info(ui, raw);
                    }
                }
                ResetStream {
                    stream_id,
                    error,
                    error_code,
                    final_size,
                    raw,
                } => {
                    ui.label("Stream Id");
                    ui.label(format!("{stream_id}"));
                    ui.end_row();
                    ui.label("Error");
                    ui.label(format!("{error:?}"));
                    ui.end_row();
                    if let Some(code) = error_code {
                        ui.label("Error Code");
                        ui.label(format!("{code}"));
                        ui.end_row();
                    }
                    ui.label("Final Size");
                    ui.label(format!("{final_size}"));
                    ui.end_row();

                    if let Some(raw) = raw {
                        render_raw_info(ui, raw);
                    }
                }
                StopSending {
                    stream_id,
                    error,
                    error_code,
                    raw,
                } => {
                    ui.label("Stream Id");
                    ui.label(format!("{stream_id}"));
                    ui.end_row();
                    ui.label("Error");
                    ui.label(format!("{error:?}"));
                    ui.end_row();
                    if let Some(code) = error_code {
                        ui.label("Error Code");
                        ui.label(format!("{code}"));
                        ui.end_row();
                    }

                    if let Some(raw) = raw {
                        render_raw_info(ui, raw);
                    }
                }
                Crypto { offset, raw } => {
                    ui.label("Offset");
                    ui.label(format!("{offset}"));
                    ui.end_row();
                    if let Some(raw) = raw {
                        render_raw_info(ui, raw);
                    }
                }
                NewToken { token, raw } => {
                    if let Some(ref ty) = token.ty {
                        ui.label("Token Type");
                        ui.label(format!("{ty:?}"));
                        ui.end_row();
                    }
                    if let Some(ref details) = token.details {
                        ui.label("Token Details");
                        ui.label(details);
                        ui.end_row();
                    }

                    if let Some(raw) = raw {
                        render_raw_info(ui, raw);
                    }
                }
                Stream {
                    stream_id,
                    offset,
                    fin,
                    raw,
                } => {
                    ui.label("Stream Id");
                    ui.label(format!("{stream_id}"));
                    ui.end_row();
                    if let Some(offset) = offset {
                        ui.label("Offset");
                        ui.label(format!("{offset}"));
                        ui.end_row();
                    }
                    if let Some(fin) = fin {
                        ui.label("Fin");
                        ui.label(format!("{fin}"));
                        ui.end_row();
                    }
                    if let Some(raw) = raw {
                        render_raw_info(ui, raw);
                    }
                }
                MaxData { maximum, raw } => {
                    ui.label("Maximum");
                    ui.label(format!("{maximum}"));
                    ui.end_row();
                    if let Some(raw) = raw {
                        render_raw_info(ui, raw);
                    }
                }
                MaxStreamData {
                    stream_id,
                    maximum,
                    raw,
                } => {
                    ui.label("Stream Id");
                    ui.label(format!("{stream_id}"));
                    ui.end_row();
                    ui.label("Maximum");
                    ui.label(format!("{maximum}"));
                    ui.end_row();
                    ui.label("Max Stream Data");
                    if let Some(raw) = raw {
                        render_raw_info(ui, raw);
                    }
                }
                MaxStreams {
                    stream_type,
                    maximum,
                    raw,
                } => {
                    ui.label("Stream Type");
                    ui.label(format!("{stream_type:?}"));
                    ui.end_row();
                    ui.label("Maximum");
                    ui.label(format!("{maximum}"));
                    ui.end_row();
                    if let Some(raw) = raw {
                        render_raw_info(ui, raw);
                    }
                }
                DataBlocked { limit, raw } => {
                    ui.label("Limit");
                    ui.label(format!("{limit}"));
                    ui.end_row();

                    if let Some(raw) = raw {
                        render_raw_info(ui, raw);
                    }
                }
                StreamDataBlocked {
                    stream_id,
                    limit,
                    raw,
                } => {
                    ui.label("Stream Id");
                    ui.label(format!("{stream_id}"));
                    ui.end_row();

                    ui.label("Limit");
                    ui.label(format!("{limit}"));
                    ui.end_row();

                    if let Some(raw) = raw {
                        render_raw_info(ui, raw);
                    }
                }
                StreamsBlocked {
                    stream_type,
                    limit,
                    raw,
                } => {
                    ui.label("Stream Type");
                    ui.label(format!("{stream_type:?}"));
                    ui.end_row();

                    ui.label("Limit");
                    ui.label(format!("{limit}"));
                    ui.end_row();

                    if let Some(raw) = raw {
                        render_raw_info(ui, raw);
                    }
                }
                NewConnectionId {
                    sequence_number,
                    retire_prior_to,
                    connection_id_length,
                    connection_id,
                    stateless_reset_token,
                    raw,
                } => {
                    ui.label("Connection Id");
                    ui.label(connection_id);
                    ui.end_row();

                    ui.label("Seq Number");
                    ui.label(format!("{sequence_number}"));
                    ui.end_row();
                    ui.label("Retire Prior To");
                    ui.label(format!("{retire_prior_to}"));
                    ui.end_row();
                    if let Some(len) = connection_id_length {
                        ui.label("Connection Id Length");
                        ui.label(format!("{len}"));
                        ui.end_row();
                    }
                    if let Some(token) = stateless_reset_token {
                        ui.label("Stateless Reset Token");
                        ui.label(token);
                        ui.end_row();
                    }

                    if let Some(raw) = raw {
                        render_raw_info(ui, raw);
                    }
                }
                RetireConnectionId {
                    sequence_number,
                    raw,
                } => {
                    ui.label("Seq Number");
                    ui.label(format!("{sequence_number}"));
                    ui.end_row();
                    if let Some(raw) = raw {
                        render_raw_info(ui, raw);
                    }
                }
                PathChallenge { data, raw } => {
                    if let Some(data) = data {
                        ui.label("Data");
                        ui.label(data);
                        ui.end_row();
                    }
                    if let Some(raw) = raw {
                        render_raw_info(ui, raw);
                    }
                }
                PathResponse { data, raw } => {
                    if let Some(data) = data {
                        ui.label("Data");
                        ui.label(data);
                        ui.end_row();
                    }

                    if let Some(raw) = raw {
                        render_raw_info(ui, raw);
                    }
                }
                ConnectionClose {
                    error_space,
                    error,
                    error_code,
                    reason,
                    reason_bytes,

                    trigger_frame_type,
                } => {
                    if let Some(error) = error {
                        ui.label("Error");
                        ui.label(format!("{error:?}"));
                        ui.end_row();
                    }
                    if let Some(error) = error_code {
                        ui.label("Error Code");
                        ui.label(format!("{error:?}"));
                        ui.end_row();
                    }
                    if let Some(error) = error_space {
                        ui.label("Error Space");
                        ui.label(format!("{error:?}"));
                        ui.end_row();
                    }
                    if let Some(reason) = reason {
                        ui.label("Reason");
                        ui.label(reason);
                        ui.end_row();
                    }
                    if let Some(reason) = reason_bytes {
                        ui.label("Reason Bytes");
                        ui.label(reason);
                        ui.end_row();
                    }
                    if let Some(ty) = trigger_frame_type {
                        ui.label("Trigger Frame Type");
                        ui.label(format!("{ty}"));
                        ui.end_row();
                    }
                }

                HandshakeDone { raw } => {
                    if let Some(raw) = raw {
                        render_raw_info(ui, raw);
                    }
                }

                Datagram { raw } => {
                    if let Some(raw) = raw {
                        render_raw_info(ui, raw);
                    }
                }

                PathAck {
                    path_id,
                    ack_delay,
                    acked_ranges,

                    ect1,
                    ect0,
                    ce,

                    raw,
                } => {
                    ui.label("Path Id");
                    ui.label(format!("{path_id}"));
                    ui.end_row();

                    if let Some(delay) = ack_delay {
                        ui.label("Ack Delay");
                        ui.label(format!("{delay}"));
                        ui.end_row();
                    }

                    if let Some(ranges) = acked_ranges {
                        ui.label("Acked Ranges");
                        ui.label(format!("{ranges:?}"));
                        ui.end_row();
                    }

                    if let Some(ect0) = ect0 {
                        ui.label("ECT 0");
                        ui.label(format!("{ect0:?}"));
                        ui.end_row();
                    }
                    if let Some(ect1) = ect1 {
                        ui.label("ECT 1");
                        ui.label(format!("{ect1:?}"));
                        ui.end_row();
                    }
                    if let Some(ce) = ce {
                        ui.label("CE");
                        ui.label(format!("{ce:?}"));
                        ui.end_row();
                    }

                    if let Some(raw) = raw {
                        render_raw_info(ui, raw);
                    }
                }
                PathAbandon {
                    path_id,
                    error_code,
                    raw,
                } => {
                    ui.label("Path Id");
                    ui.label(format!("{path_id}"));
                    ui.end_row();

                    ui.label("Error Code");
                    ui.label(format!("{error_code}"));
                    ui.end_row();

                    if let Some(raw) = raw {
                        render_raw_info(ui, raw);
                    }
                }

                PathStatusAvailable {
                    path_id,
                    path_status_sequence_number,
                    raw,
                } => {
                    ui.label("Path Id");
                    ui.label(format!("{path_id}"));
                    ui.end_row();

                    ui.label("Path Status Seq Number");
                    ui.label(format!("{path_status_sequence_number}"));
                    ui.end_row();

                    if let Some(raw) = raw {
                        render_raw_info(ui, raw);
                    }
                }

                PathStatusBackup {
                    path_id,
                    path_status_sequence_number,
                    raw,
                } => {
                    ui.label("Path Id");
                    ui.label(format!("{path_id}"));
                    ui.end_row();

                    ui.label("Path Status Seq Number");
                    ui.label(format!("{path_status_sequence_number}"));
                    ui.end_row();

                    if let Some(raw) = raw {
                        render_raw_info(ui, raw);
                    }
                }

                PathNewConnectionId {
                    path_id,
                    sequence_number,
                    retire_prior_to,
                    connection_id_length,
                    connection_id,
                    stateless_reset_token,
                    raw,
                } => {
                    ui.label("Connection Id");
                    ui.label(connection_id.to_string());
                    ui.end_row();

                    ui.label("Path Id");
                    ui.label(format!("{path_id}"));
                    ui.end_row();

                    ui.label("Seq Number");
                    ui.label(format!("{sequence_number}"));
                    ui.end_row();

                    ui.label("Retire Prior To");
                    ui.label(format!("{retire_prior_to}"));
                    ui.end_row();

                    if let Some(len) = connection_id_length {
                        ui.label("Connection Id Length");
                        ui.label(format!("{len}"));
                        ui.end_row();
                    }
                    if let Some(token) = stateless_reset_token {
                        ui.label("Stateless Reset Token");
                        ui.label(token.to_string());
                        ui.end_row();
                    }

                    if let Some(raw) = raw {
                        render_raw_info(ui, raw);
                    }
                }

                PathRetireConnectionId {
                    path_id,
                    sequence_number,
                    raw,
                } => {
                    ui.label("Path Id");
                    ui.label(format!("{path_id}"));
                    ui.end_row();

                    ui.label("Seq Number");
                    ui.label(format!("{sequence_number}"));
                    ui.end_row();

                    if let Some(raw) = raw {
                        render_raw_info(ui, raw);
                    }
                }
                MaxPathId {
                    maximum_path_id,
                    raw,
                } => {
                    ui.label("Maxium Path Id");
                    ui.label(format!("{maximum_path_id}"));
                    ui.end_row();

                    if let Some(raw) = raw {
                        render_raw_info(ui, raw);
                    }
                }
                AckFrequency {
                    sequence_number,
                    ack_eliciting_threshold,
                    requested_max_ack_delay,
                    reordering_threshold,
                    raw,
                } => {
                    ui.label("Seq Number");
                    ui.label(format!("{sequence_number}"));
                    ui.end_row();
                    ui.label("Ack Eliciting Threshold");
                    ui.label(format!("{ack_eliciting_threshold}"));
                    ui.end_row();
                    ui.label("Requested Max Ack Delay");
                    ui.label(format!("{requested_max_ack_delay}"));
                    ui.end_row();
                    ui.label("Reordering Threshold");
                    ui.label(format!("{reordering_threshold}"));
                    ui.end_row();

                    if let Some(raw) = raw {
                        render_raw_info(ui, raw);
                    }
                }
                ImmediateAck { raw } => {
                    if let Some(raw) = raw {
                        render_raw_info(ui, raw);
                    }
                }
                ObservedAddress {
                    sequence_number,
                    ip_v4,
                    ip_v6,
                    port,
                    raw,
                } => {
                    ui.label("Seq Number");
                    ui.label(format!("{sequence_number}"));
                    ui.end_row();
                    if let Some(ip) = ip_v4 {
                        ui.label("IP V4");
                        ui.label(ip);
                        ui.end_row();
                    }
                    if let Some(ip) = ip_v6 {
                        ui.label("IP V6");
                        ui.label(ip);
                        ui.end_row();
                    }
                    ui.label("Port");
                    ui.label(format!("{port}"));
                    ui.end_row();
                    if let Some(raw) = raw {
                        render_raw_info(ui, raw);
                    }
                }
                AddAddress {
                    sequence_number,
                    ip_v4,
                    ip_v6,
                    port,
                } => {
                    ui.label("Seq Number");
                    ui.label(format!("{sequence_number}"));
                    ui.end_row();
                    if let Some(ip) = ip_v4 {
                        ui.label("IP V4");
                        ui.label(ip);
                        ui.end_row();
                    }
                    if let Some(ip) = ip_v6 {
                        ui.label("IP V6");
                        ui.label(ip);
                        ui.end_row();
                    }
                    ui.label("Port");
                    ui.label(format!("{port}"));
                    ui.end_row();
                }
                ReachOut {
                    round,
                    ip_v4,
                    ip_v6,
                    port,
                } => {
                    ui.label("round");
                    ui.label(format!("{round}"));
                    ui.end_row();
                    if let Some(ip) = ip_v4 {
                        ui.label("IP V4");
                        ui.label(ip);
                        ui.end_row();
                    }
                    if let Some(ip) = ip_v6 {
                        ui.label("IP V6");
                        ui.label(ip);
                        ui.end_row();
                    }
                    ui.label("Port");
                    ui.label(format!("{port}"));
                    ui.end_row();
                }
                RemoveAddress { sequence_number } => {
                    ui.label("Seq Number");
                    ui.label(format!("{sequence_number}"));
                    ui.end_row();
                }
                PathsBlocked {
                    maximum_path_id,
                    raw,
                } => {
                    ui.label("Maxium Path Id");
                    ui.label(format!("{maximum_path_id}"));
                    ui.end_row();

                    if let Some(raw) = raw {
                        render_raw_info(ui, raw);
                    }
                }
                PathCidsBlocked {
                    path_id,
                    next_sequence_number,
                    raw,
                } => {
                    ui.label("Path Id");
                    ui.label(format!("{path_id}"));
                    ui.end_row();

                    ui.label("Next Seq Number");
                    ui.label(format!("{next_sequence_number}"));
                    ui.end_row();

                    if let Some(raw) = raw {
                        render_raw_info(ui, raw);
                    }
                }
                Unknown {
                    frame_type_bytes,
                    raw,
                } => {
                    if let Some(ty) = frame_type_bytes {
                        ui.label("Frame type");
                        ui.label(format!("{ty}"));
                        ui.end_row();
                    }
                    if let Some(raw) = raw {
                        render_raw_info(ui, raw);
                    }
                }
            });
        });
    ui.add_space(5.);
}

pub fn render_section_header(ui: &mut egui::Ui, title: &str) {
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 24.0), egui::Sense::hover());
    ui.painter()
        .rect_filled(rect, 2.0, egui::Color32::from_gray(45));
    ui.painter().text(
        rect.left_center() + egui::vec2(8.0, 0.0),
        egui::Align2::LEFT_CENTER,
        title,
        egui::FontId::proportional(13.0),
        egui::Color32::WHITE,
    );
}

pub fn render_badge(ui: &mut egui::Ui, text: &str, bg_color: egui::Color32) {
    egui::Frame::new()
        .fill(bg_color)
        .corner_radius(4.0)
        .inner_margin(egui::Margin::symmetric(6, 2))
        .show(ui, |ui| {
            ui.label(RichText::new(text).color(egui::Color32::WHITE).size(11.0));
        });
}

pub fn render_raw_info(ui: &mut egui::Ui, raw: &RawInfo) {
    if let Some(length) = raw.length {
        ui.label("Raw Info: Length");
        ui.label(format!("{length}"));
        ui.end_row();
    }
    if let Some(length) = raw.payload_length {
        ui.label("Raw Info: Payload Length");
        ui.label(format!("{length}"));
        ui.end_row();
    }
    if let Some(ref data) = raw.data {
        ui.label("Raw Info: Data");
        ui.label(data.as_str());
        ui.end_row();
    }
}

pub fn render_header(ui: &mut egui::Ui, event: &EventData, id_prefix: &str, idx: usize) {
    match event {
        EventData::QuicPacketSent(ref data) => {
            render_inner_header(ui, &data.header, id_prefix, idx);
        }
        EventData::QuicPacketReceived(ref data) => {
            render_inner_header(ui, &data.header, id_prefix, idx);
        }
        EventData::QuicPacketDropped(ref data) => {
            if let Some(ref header) = data.header {
                render_inner_header(ui, header, id_prefix, idx);
            }
        }
        EventData::QuicPacketBuffered(ref data) => {
            if let Some(ref header) = data.header {
                render_inner_header(ui, header, id_prefix, idx);
            }
        }
        EventData::QuicPacketLost(ref data) => {
            if let Some(ref header) = data.header {
                render_inner_header(ui, header, id_prefix, idx);
            }
        }
        _ => {}
    }
}

pub fn render_inner_header(ui: &mut egui::Ui, header: &PacketHeader, id_prefix: &str, idx: usize) {
    egui::Grid::new(format!("{}-packet_header-{}", id_prefix, idx))
        .num_columns(2)
        .spacing([20.0, 4.0])
        .show(ui, |ui| {
            ui.label("Packet Space");
            let space_color = match header.packet_type {
                PacketType::Initial => egui::Color32::from_rgb(52, 152, 219),
                PacketType::Handshake => egui::Color32::from_rgb(155, 89, 182),
                PacketType::ZeroRtt => egui::Color32::from_rgb(230, 126, 34),
                PacketType::OneRtt => egui::Color32::from_rgb(46, 204, 113),
                _ => egui::Color32::GRAY,
            };
            render_badge(ui, &format!("{:?}", header.packet_type), space_color);
            ui.end_row();

            if let Some(pn) = header.packet_number {
                ui.label("Packet Number");
                ui.label(format!("{}", pn));
                ui.end_row();
            }
            if let Some(pid) = header.path_id {
                ui.label("Path");
                let path_colors = [
                    egui::Color32::from_rgb(46, 204, 113),
                    egui::Color32::from_rgb(241, 196, 15),
                    egui::Color32::from_rgb(231, 76, 60),
                    egui::Color32::from_rgb(52, 152, 219),
                ];
                let color = path_colors[pid as usize % path_colors.len()];
                render_badge(ui, &format!("Path {}", pid), color);
                ui.end_row();
            }
        });
}

pub fn render_transport_parameters(
    ui: &mut egui::Ui,
    params: &qlog::events::quic::ParametersSet,
    id_prefix: &str,
    idx: usize,
) {
    ui.heading("Transport Parameters");

    if let Some(ref initiator) = params.initiator {
        ui.label(format!("Initiator: {:?}", initiator));
    }

    CollapsingHeader::new("Connection IDs")
        .id_salt(format!("{}-params-cids-{}", id_prefix, idx))
        .default_open(true)
        .show(ui, |ui| {
            if let Some(ref cid) = params.original_destination_connection_id {
                ui.label(format!("Original DCID: {}", cid));
            }
            if let Some(ref cid) = params.initial_source_connection_id {
                ui.label(format!("Initial SCID: {}", cid));
            }
            if let Some(ref cid) = params.retry_source_connection_id {
                ui.label(format!("Retry SCID: {}", cid));
            }
            if let Some(ref token) = params.stateless_reset_token {
                ui.label(format!("Stateless Reset Token: {}", token));
            }
            if let Some(limit) = params.active_connection_id_limit {
                ui.label(format!("Active CID Limit: {}", limit));
            }
        });

    CollapsingHeader::new("Flow Control")
        .id_salt(format!("{}-params-flow-{}", id_prefix, idx))
        .default_open(true)
        .show(ui, |ui| {
            if let Some(v) = params.initial_max_data {
                ui.label(format!("Initial Max Data: {}", utils::format_bytes(v)));
            }
            if let Some(v) = params.initial_max_stream_data_bidi_local {
                ui.label(format!(
                    "Max Stream Data (bidi local): {}",
                    utils::format_bytes(v)
                ));
            }
            if let Some(v) = params.initial_max_stream_data_bidi_remote {
                ui.label(format!(
                    "Max Stream Data (bidi remote): {}",
                    utils::format_bytes(v)
                ));
            }
            if let Some(v) = params.initial_max_stream_data_uni {
                ui.label(format!("Max Stream Data (uni): {}", utils::format_bytes(v)));
            }
            if let Some(v) = params.initial_max_streams_bidi {
                ui.label(format!("Max Streams (bidi): {}", v));
            }
            if let Some(v) = params.initial_max_streams_uni {
                ui.label(format!("Max Streams (uni): {}", v));
            }
        });

    CollapsingHeader::new("Timing")
        .id_salt(format!("{}-params-timing-{}", id_prefix, idx))
        .default_open(true)
        .show(ui, |ui| {
            if let Some(v) = params.max_idle_timeout {
                ui.label(format!("Max Idle Timeout: {}ms", v));
            }
            if let Some(v) = params.max_ack_delay {
                ui.label(format!("Max ACK Delay: {}ms", v));
            }
            if let Some(v) = params.ack_delay_exponent {
                ui.label(format!("ACK Delay Exponent: {}", v));
            }
            if let Some(v) = params.min_ack_delay {
                ui.label(format!("Min ACK Delay: {}µs", v));
            }
        });

    CollapsingHeader::new("Network")
        .id_salt(format!("{}-params-network-{}", id_prefix, idx))
        .default_open(true)
        .show(ui, |ui| {
            if let Some(v) = params.max_udp_payload_size {
                ui.label(format!("Max UDP Payload: {} bytes", v));
            }
            if let Some(v) = params.max_datagram_frame_size {
                ui.label(format!("Max Datagram Frame: {} bytes", v));
            }
            if let Some(v) = params.disable_active_migration {
                ui.label(format!("Disable Active Migration: {}", v));
            }
            if let Some(ref addr) = params.preferred_address {
                ui.label(format!("Preferred Address: {:?}", addr));
            }
        });

    let has_extensions = params.initial_max_path_id.is_some()
        || params.max_remote_nat_traversal_addresses.is_some()
        || params.grease_quic_bit.is_some()
        || params.address_discovery.is_some();

    if has_extensions {
        CollapsingHeader::new("Extensions")
            .id_salt(format!("{}-params-ext-{}", id_prefix, idx))
            .default_open(true)
            .show(ui, |ui| {
                if let Some(v) = params.initial_max_path_id {
                    ui.label(format!("Initial Max Path ID: {} (Multipath)", v));
                }
                if let Some(v) = params.max_remote_nat_traversal_addresses {
                    ui.label(format!("Max NAT Traversal Addrs: {}", v));
                }
                if let Some(v) = params.grease_quic_bit {
                    ui.label(format!("GREASE QUIC Bit: {}", v));
                }
                if let Some(ref role) = params.address_discovery {
                    ui.label(format!("Address Discovery: {:?}", role));
                }
            });
    }

    let has_tls = params.resumption_allowed.is_some()
        || params.early_data_enabled.is_some()
        || params.tls_cipher.is_some();

    if has_tls {
        CollapsingHeader::new("TLS")
            .id_salt(format!("{}-params-tls-{}", id_prefix, idx))
            .default_open(false)
            .show(ui, |ui| {
                if let Some(v) = params.resumption_allowed {
                    ui.label(format!("Resumption Allowed: {}", v));
                }
                if let Some(v) = params.early_data_enabled {
                    ui.label(format!("0-RTT Enabled: {}", v));
                }
                if let Some(ref cipher) = params.tls_cipher {
                    ui.label(format!("TLS Cipher: {}", cipher));
                }
            });
    }

    if !params.unknown_parameters.is_empty() {
        CollapsingHeader::new("Unknown Parameters")
            .id_salt(format!("{}-params-unknown-{}", id_prefix, idx))
            .default_open(false)
            .show(ui, |ui| {
                for param in &params.unknown_parameters {
                    ui.label(format!("ID 0x{:x}: {}", param.id, param.value));
                }
            });
    }
}

pub fn render_parameters_restored(
    ui: &mut egui::Ui,
    params: &qlog::events::quic::ParametersRestored,
) {
    ui.heading("Restored Parameters (0-RTT)");

    if let Some(v) = params.max_idle_timeout {
        ui.label(format!("Max Idle Timeout: {}ms", v));
    }
    if let Some(v) = params.max_udp_payload_size {
        ui.label(format!("Max UDP Payload: {} bytes", v));
    }
    if let Some(v) = params.active_connection_id_limit {
        ui.label(format!("Active CID Limit: {}", v));
    }
    if let Some(v) = params.initial_max_data {
        ui.label(format!("Initial Max Data: {}", utils::format_bytes(v)));
    }
    if let Some(v) = params.initial_max_stream_data_bidi_local {
        ui.label(format!(
            "Max Stream Data (bidi local): {}",
            utils::format_bytes(v)
        ));
    }
    if let Some(v) = params.initial_max_stream_data_bidi_remote {
        ui.label(format!(
            "Max Stream Data (bidi remote): {}",
            utils::format_bytes(v)
        ));
    }
    if let Some(v) = params.initial_max_stream_data_uni {
        ui.label(format!("Max Stream Data (uni): {}", utils::format_bytes(v)));
    }
    if let Some(v) = params.initial_max_streams_bidi {
        ui.label(format!("Max Streams (bidi): {}", v));
    }
    if let Some(v) = params.initial_max_streams_uni {
        ui.label(format!("Max Streams (uni): {}", v));
    }
    if let Some(v) = params.disable_active_migration {
        ui.label(format!("Disable Active Migration: {}", v));
    }
    if let Some(v) = params.max_datagram_frame_size {
        ui.label(format!("Max Datagram Frame: {} bytes", v));
    }
    if let Some(v) = params.grease_quic_bit {
        ui.label(format!("GREASE QUIC Bit: {}", v));
    }
}

pub fn render_recovery_parameters(
    ui: &mut egui::Ui,
    params: &qlog::events::quic::RecoveryParametersSet,
) {
    ui.heading("Recovery Parameters");

    if let Some(v) = params.reordering_threshold {
        ui.label(format!("Reordering Threshold: {}", v));
    }
    if let Some(v) = params.time_threshold {
        ui.label(format!("Time Threshold: {:.2}", v));
    }
    if let Some(v) = params.timer_granularity {
        ui.label(format!("Timer Granularity: {}ms", v));
    }
    if let Some(v) = params.initial_rtt {
        ui.label(format!("Initial RTT: {:.2}ms", v));
    }
    if let Some(v) = params.max_datagram_size {
        ui.label(format!("Max Datagram Size: {} bytes", v));
    }
    if let Some(v) = params.initial_congestion_window {
        ui.label(format!("Initial CWND: {}", utils::format_bytes(v)));
    }
    if let Some(v) = params.minimum_congestion_window {
        ui.label(format!("Min CWND: {} bytes", v));
    }
    if let Some(v) = params.loss_reduction_factor {
        ui.label(format!("Loss Reduction Factor: {:.2}", v));
    }
    if let Some(v) = params.persistent_congestion_threshold {
        ui.label(format!("Persistent Congestion Threshold: {}", v));
    }
}

pub fn render_recovery_metrics_updated(
    ui: &mut egui::Ui,
    params: &qlog::events::quic::RecoveryMetricsUpdated,
    id_prefix: &str,
    idx: usize,
) {
    render_section_header(ui, "Recovery Metrics");
    ui.add_space(4.0);

    egui::Grid::new(format!("{}-recovery_metrics-{}", id_prefix, idx))
        .num_columns(2)
        .spacing([20.0, 4.0])
        .striped(true)
        .show(ui, |ui| {
            if let Some(v) = params.path_id {
                ui.label("Path");
                ui.label(format!("{}", v));
                ui.end_row();
            }
            if let Some(v) = params.min_rtt {
                ui.label("Min RTT");
                ui.label(utils::format_rtt(v));
                ui.end_row();
            }
            if let Some(v) = params.smoothed_rtt {
                ui.label("Smoothed RTT");
                ui.label(utils::format_rtt(v));
                ui.end_row();
            }
            if let Some(v) = params.latest_rtt {
                ui.label("Latest RTT");
                ui.label(utils::format_rtt(v));
                ui.end_row();
            }
            if let Some(v) = params.rtt_variance {
                ui.label("RTT Variance");
                ui.label(utils::format_rtt(v));
                ui.end_row();
            }
            if let Some(v) = params.congestion_window {
                ui.label("Cwnd");
                ui.label(utils::format_bytes(v));
                ui.end_row();
            }
            if let Some(v) = params.bytes_in_flight {
                ui.label("Bytes in Flight");
                ui.label(utils::format_bytes(v));
                ui.end_row();
            }
            if let Some(v) = params.ssthresh {
                ui.label("Ssthresh");
                ui.label(utils::format_bytes(v));
                ui.end_row();
            }
            if let Some(v) = params.packets_in_flight {
                ui.label("Packets in Flight");
                ui.label(format!("{}", v));
                ui.end_row();
            }
            if let Some(v) = params.pacing_rate {
                ui.label("Pacing Rate");
                ui.label(format!("{}", v));
                ui.end_row();
            }
            if let Some(v) = params.pto_count {
                ui.label("PTO Count");
                ui.label(format!("{}", v));
                ui.end_row();
            }
        });
}

pub fn render_timer_updated(
    ui: &mut egui::Ui,
    timer: &qlog::events::quic::TimerUpdated,
    id_prefix: &str,
    idx: usize,
) {
    render_section_header(ui, "Timer Updated");
    ui.add_space(4.0);

    egui::Grid::new(format!("{}-timer_updated-{}", id_prefix, idx))
        .num_columns(2)
        .spacing([20.0, 4.0])
        .striped(true)
        .show(ui, |ui| {
            ui.label("Event Type");
            ui.label(format!("{:?}", timer.event_type));
            ui.end_row();

            if let Some(ref timer_type) = timer.timer_type {
                ui.label("Timer Type");
                ui.label(format!("{:?}", timer_type));
                ui.end_row();
            }
            if let Some(path_id) = timer.path_id {
                ui.label("Path ID");
                ui.label(format!("{}", path_id));
                ui.end_row();
            }
            if let Some(timer_id) = timer.timer_id {
                ui.label("Timer ID");
                ui.label(format!("{}", timer_id));
                ui.end_row();
            }
            if let Some(ref pns) = timer.packet_number_space {
                ui.label("Packet Number Space");
                ui.label(format!("{:?}", pns));
                ui.end_row();
            }
            if let Some(delta) = timer.delta {
                ui.label("Delta");
                ui.label(utils::format_rtt(delta));
                ui.end_row();
            }
        });
}

pub fn render_ecn_state_updated(
    ui: &mut egui::Ui,
    ecn: &qlog::events::quic::EcnStateUpdated,
    id_prefix: &str,
    idx: usize,
) {
    render_section_header(ui, "ECN State Updated");
    ui.add_space(4.0);

    egui::Grid::new(format!("{}-ecn_state-{}", id_prefix, idx))
        .num_columns(2)
        .spacing([20.0, 4.0])
        .striped(true)
        .show(ui, |ui| {
            if let Some(ref old) = ecn.old {
                ui.label("Old State");
                ui.label(format!("{:?}", old));
                ui.end_row();
            }
            ui.label("New State");
            ui.label(format!("{:?}", ecn.new));
            ui.end_row();
        });
}

pub fn render_connection_started(
    ui: &mut egui::Ui,
    started: &qlog::events::quic::ConnectionStarted,
    id_prefix: &str,
    idx: usize,
) {
    render_section_header(ui, "Connection Started");
    ui.add_space(4.0);

    ui.label(RichText::new("Local").strong());
    render_tuple_endpoint_info(ui, &started.local, &format!("local-{id_prefix}"), idx);
    ui.add_space(5.);
    ui.label(RichText::new("Remote").strong());
    render_tuple_endpoint_info(ui, &started.remote, &format!("remote-{id_prefix}"), idx);
}

pub fn render_tuple_assigned(
    ui: &mut egui::Ui,
    tuple: &qlog::events::quic::TupleAssigned,
    id_prefix: &str,
    idx: usize,
) {
    render_section_header(ui, "Tuple Assigned");
    ui.add_space(4.0);

    egui::Grid::new(format!("{}-tuple_assigned-{}", id_prefix, idx))
        .num_columns(2)
        .spacing([20.0, 4.0])
        .show(ui, |ui| {
            ui.label("Tuple ID");
            ui.label(tuple.tuple_id.to_string());
            ui.end_row();
        });

    if let Some(ref local) = tuple.tuple_local {
        ui.add_space(4.0);
        ui.label(RichText::new("Local").strong());
        render_tuple_endpoint_info(ui, local, &format!("local-{id_prefix}"), idx);
    }
    if let Some(ref remote) = tuple.tuple_remote {
        ui.add_space(4.0);
        ui.label(RichText::new("Remote").strong());
        render_tuple_endpoint_info(ui, remote, &format!("remote-{id_prefix}"), idx);
    }
}

pub fn render_tuple_endpoint_info(
    ui: &mut egui::Ui,
    info: &qlog::events::TupleEndpointInfo,
    id_prefix: &str,
    idx: usize,
) {
    egui::Grid::new(format!("tuple_endpoint_grid_{}_{}", id_prefix, idx))
        .num_columns(2)
        .spacing([20.0, 4.0])
        .striped(true)
        .show(ui, |ui| {
            if let Some(ref v) = info.ip_v4 {
                ui.label("IP v4");
                ui.label(v);
                ui.end_row();
            }
            if let Some(ref v) = info.port_v4 {
                ui.label("Port v4");
                ui.label(format!("{}", v));
                ui.end_row();
            }
            if let Some(ref v) = info.ip_v6 {
                ui.label("IP v6");
                ui.label(v);
                ui.end_row();
            }
            if let Some(v) = info.port_v6 {
                ui.label("Port v6");
                ui.label(format!("{}", v));
                ui.end_row();
            }
        });

    if let Some(ref ids) = info.connection_ids {
        CollapsingHeader::new("Connection IDs")
            .id_salt(format!("{}-connection-ids-{}", id_prefix, idx))
            .default_open(true)
            .show(ui, |ui| {
                for connection in ids {
                    ui.label(connection);
                }
            });
    }
}
