use super::types::{path_color, DiagramLayout, PathLaneLayout};
use crate::time_compression::{TimeContext, TimeGap, COMPRESSED_GAP_HEIGHT};
use crate::utils::FrameType;
use egui::{Color32, Painter, Pos2, Rect, Stroke, StrokeKind, Vec2};

pub const LOSS_COLOR: Color32 = Color32::RED;

#[allow(clippy::too_many_arguments)]
pub fn draw_header(
    painter: &Painter,
    layout: &DiagramLayout,
    viewport: Rect,
    width: f32,
    fill: Color32,
    left_label: &str,
    right_label: &str,
    left_color: Color32,
    right_color: Color32,
) {
    let header_y = layout.rect.top() + viewport.top() + 18.0;
    painter.rect_filled(
        Rect::from_min_size(
            Pos2::new(layout.rect.left(), layout.rect.top() + viewport.top()),
            Vec2::new(width, 40.0),
        ),
        0.0,
        fill,
    );
    painter.text(
        Pos2::new(layout.left_x, header_y),
        egui::Align2::CENTER_CENTER,
        left_label,
        egui::FontId::proportional(14.0),
        left_color,
    );
    painter.text(
        Pos2::new(layout.right_x, header_y),
        egui::Align2::CENTER_CENTER,
        right_label,
        egui::FontId::proportional(14.0),
        right_color,
    );
}

pub fn draw_timelines(painter: &Painter, layout: &DiagramLayout, top: f32, bottom: f32) {
    for x in [layout.left_x, layout.right_x] {
        painter.line_segment(
            [Pos2::new(x, top), Pos2::new(x, bottom)],
            Stroke::new(2.0, Color32::GRAY),
        );
    }
}

pub fn draw_lane_timelines(
    painter: &Painter,
    _layout: &DiagramLayout,
    top: f32,
    bottom: f32,
    lanes: &PathLaneLayout,
) {
    for (&path_id, &x) in lanes.left_lanes.iter() {
        painter.line_segment(
            [Pos2::new(x, top), Pos2::new(x, bottom)],
            Stroke::new(1.5, Color32::from_rgb(100, 100, 100)),
        );
        painter.text(
            Pos2::new(x, top - 15.0),
            egui::Align2::CENTER_BOTTOM,
            format!("Path {}", path_id),
            egui::FontId::proportional(10.0),
            Color32::LIGHT_GRAY,
        );
    }
    for (&path_id, &x) in lanes.right_lanes.iter() {
        painter.line_segment(
            [Pos2::new(x, top), Pos2::new(x, bottom)],
            Stroke::new(1.5, Color32::from_rgb(100, 100, 100)),
        );
        painter.text(
            Pos2::new(x, top - 15.0),
            egui::Align2::CENTER_BOTTOM,
            format!("Path {}", path_id),
            egui::FontId::proportional(10.0),
            Color32::LIGHT_GRAY,
        );
    }
}

pub fn draw_gap_indicators<F>(
    painter: &Painter,
    layout: &DiagramLayout,
    gaps: &[TimeGap],
    time_to_y: &F,
) where
    F: Fn(f64) -> f32,
{
    let gap_color = Color32::from_rgb(180, 160, 80);
    let text_color = Color32::from_rgb(150, 150, 150);

    for gap in gaps {
        let y_pos = time_to_y(gap.start_time);
        let end_y = y_pos + COMPRESSED_GAP_HEIGHT;
        let center_y = y_pos + COMPRESSED_GAP_HEIGHT / 2.0;

        let bg_rect = Rect::from_min_max(
            Pos2::new(layout.left_x, y_pos),
            Pos2::new(layout.right_x, end_y),
        );
        painter.rect_filled(
            bg_rect,
            0.0,
            Color32::from_rgba_unmultiplied(180, 160, 80, 20),
        );

        for x in [layout.left_x, layout.right_x] {
            painter.line_segment(
                [Pos2::new(x, y_pos), Pos2::new(x, end_y)],
                Stroke::new(6.0, gap_color),
            );
        }

        let center_x = (layout.left_x + layout.right_x) / 2.0;
        let label = format!("compressed {:.1}ms", gap.compressed_amount);
        painter.text(
            Pos2::new(center_x, center_y),
            egui::Align2::CENTER_CENTER,
            &label,
            egui::FontId::proportional(12.0),
            text_color,
        );

        let right_text_x = layout.right_x - 10.0;
        let start_label = format!("{:.1}ms", gap.start_time);
        let end_label = format!("{:.1}ms", gap.end_time);
        painter.text(
            Pos2::new(right_text_x, y_pos + 5.0),
            egui::Align2::RIGHT_TOP,
            &start_label,
            egui::FontId::proportional(9.0),
            Color32::from_rgb(120, 120, 120),
        );
        painter.text(
            Pos2::new(right_text_x, end_y - 5.0),
            egui::Align2::RIGHT_BOTTOM,
            &end_label,
            egui::FontId::proportional(9.0),
            Color32::from_rgb(120, 120, 120),
        );
    }
}

pub fn draw_time_markers(
    painter: &Painter,
    layout: &DiagramLayout,
    viewport: &Rect,
    time_ctx: &TimeContext,
    server_time_offset_ms: f64,
    files_swapped: bool,
) {
    let target_markers = 6;
    let step_pixels = viewport.height() / target_markers as f32;
    let marker_color = Color32::from_rgb(140, 140, 140);

    let visible_top = layout.rect.top() + viewport.top();
    let visible_bottom = layout.rect.top() + viewport.bottom();

    // Pre-compute gap visual positions for time adjustment
    let gap_visual_starts: Vec<f32> = {
        let mut result = Vec::with_capacity(time_ctx.gaps.len());
        let mut cum_compression = 0.0f64;
        let mut cum_gap_heights = 0.0f32;
        for gap in time_ctx.gaps {
            let start = (gap.start_time - time_ctx.min_time - cum_compression) as f32
                * time_ctx.pixels_per_ms
                + cum_gap_heights;
            result.push(start);
            cum_compression += gap.compressed_amount;
            cum_gap_heights += COMPRESSED_GAP_HEIGHT;
        }
        result
    };

    // Only iterate markers in visible range
    let first_marker_offset = (visible_top - layout.top_y - step_pixels).max(0.0);
    let first_idx = (first_marker_offset / step_pixels).floor() as usize;
    let last_offset = visible_bottom - layout.top_y + step_pixels;
    let last_idx = (last_offset / step_pixels).ceil() as usize;

    for i in first_idx..=last_idx {
        let visual_offset = i as f32 * step_pixels;
        let y = layout.top_y + visual_offset;

        if y < visible_top - 20.0 || y > visible_bottom + 20.0 {
            continue;
        }

        let mut time = time_ctx.min_time + (visual_offset / time_ctx.pixels_per_ms) as f64;
        for (idx, gap) in time_ctx.gaps.iter().enumerate() {
            if visual_offset > gap_visual_starts[idx] {
                time += gap.compressed_amount;
            }
        }
        time = time.clamp(time_ctx.min_time, time_ctx.max_time);

        // Format time label helper
        let format_time = |t: f64| -> String {
            let decimals = if t.abs() < 1.0 {
                2
            } else if t.abs() < 100.0 {
                1
            } else {
                0
            };
            format!("{:.prec$}", t, prec = decimals)
        };

        // Client time (no offset)
        let client_label = format_time(time);
        // Server time (offset added - server timeline is pulled UP, so at y position
        // showing client time T, server time is T + offset)
        let server_time = time + server_time_offset_ms;
        let server_label = format_time(server_time);

        // Left side label (client if not swapped, server if swapped)
        let left_label = if files_swapped {
            &server_label
        } else {
            &client_label
        };
        // Right side label (server if not swapped, client if swapped)
        let right_label = if files_swapped {
            &client_label
        } else {
            &server_label
        };

        painter.text(
            Pos2::new(layout.left_x - 15.0, y),
            egui::Align2::RIGHT_CENTER,
            left_label,
            egui::FontId::proportional(10.0),
            marker_color,
        );
        painter.text(
            Pos2::new(layout.right_x + 15.0, y),
            egui::Align2::LEFT_CENTER,
            right_label,
            egui::FontId::proportional(10.0),
            marker_color,
        );
    }
}

pub fn draw_arrowhead(painter: &Painter, from: Pos2, to: Pos2, color: Color32, size: f32) {
    let dx = to.x - from.x;
    let dy = to.y - from.y;
    let len = (dx * dx + dy * dy).sqrt();
    if len < 1.0 {
        return;
    }

    let ux = dx / len;
    let uy = dy / len;
    let px = -uy;
    let py = ux;

    let tip = to;
    let left = Pos2::new(
        tip.x - ux * size + px * size * 0.6,
        tip.y - uy * size + py * size * 0.6,
    );
    let right = Pos2::new(
        tip.x - ux * size - px * size * 0.6,
        tip.y - uy * size - py * size * 0.6,
    );

    painter.add(egui::Shape::convex_polygon(
        vec![tip, left, right],
        color,
        Stroke::NONE,
    ));
}

pub fn draw_loss_marker(painter: &Painter, center: Pos2, size: f32) {
    let box_rect = Rect::from_center_size(center, Vec2::new(size * 1.5, size));

    painter.rect_filled(
        box_rect,
        2.0,
        Color32::from_rgba_unmultiplied(255, 0, 0, 80),
    );

    let hatch_stroke = Stroke::new(1.0, Color32::from_rgb(180, 50, 50));
    let step = 3.0;
    let (left, right, top, bottom) = (
        box_rect.left(),
        box_rect.right(),
        box_rect.top(),
        box_rect.bottom(),
    );

    let mut x = left;
    while x < right + (bottom - top) {
        let x1 = x.max(left);
        let y1 = (top + (x1 - x)).clamp(top, bottom);
        let x2 = (x + (bottom - top)).min(right);
        let y2 = (bottom - (right - x2).max(0.0)).clamp(top, bottom);

        if x1 < right && x2 > left {
            painter.line_segment([Pos2::new(x1, y1), Pos2::new(x2, y2)], hatch_stroke);
        }
        x += step;
    }

    painter.rect_stroke(
        box_rect,
        2.0,
        Stroke::new(1.5, LOSS_COLOR),
        StrokeKind::Inside,
    );

    let x_size = size * 0.4;
    for (dx, dy) in [(1.0, 1.0), (1.0, -1.0)] {
        painter.line_segment(
            [
                Pos2::new(center.x - x_size, center.y - x_size * dy),
                Pos2::new(center.x + x_size, center.y + x_size * dy * dx),
            ],
            Stroke::new(2.0, LOSS_COLOR),
        );
    }
}

pub fn draw_path_legend(painter: &Painter, rect: Rect, viewport: Rect, path_ids: &[u64]) {
    if path_ids.is_empty() || path_ids.len() == 1 {
        return;
    }

    let legend_width = 100.0;
    let legend_height = (path_ids.len() as f32 * 22.0) + 12.0;
    let margin = 10.0;

    let legend_x = rect.right() + viewport.right() - legend_width - margin;
    let legend_y = rect.top() + viewport.top() + margin;

    let legend_rect = Rect::from_min_size(
        Pos2::new(legend_x, legend_y),
        Vec2::new(legend_width, legend_height),
    );

    painter.rect_filled(
        legend_rect,
        4.0,
        Color32::from_rgba_unmultiplied(40, 40, 45, 230),
    );
    painter.rect_stroke(
        legend_rect,
        4.0,
        Stroke::new(1.0, Color32::from_rgb(80, 80, 85)),
        StrokeKind::Inside,
    );

    painter.text(
        Pos2::new(legend_x + 8.0, legend_y + 8.0),
        egui::Align2::LEFT_TOP,
        "Paths",
        egui::FontId::proportional(10.0),
        Color32::LIGHT_GRAY,
    );

    for (i, &path_id) in path_ids.iter().enumerate() {
        let y = legend_y + 24.0 + (i as f32 * 22.0);
        let color = path_color(path_id);

        let box_rect = Rect::from_min_size(Pos2::new(legend_x + 8.0, y), Vec2::new(14.0, 14.0));
        painter.rect_filled(box_rect, 2.0, color);
        painter.rect_stroke(
            box_rect,
            2.0,
            Stroke::new(1.0, Color32::from_rgb(60, 60, 65)),
            StrokeKind::Inside,
        );

        painter.text(
            Pos2::new(legend_x + 28.0, y + 7.0),
            egui::Align2::LEFT_CENTER,
            format!("Path {}", path_id),
            egui::FontId::proportional(10.0),
            Color32::LIGHT_GRAY,
        );
    }
}

#[allow(dead_code)]
pub fn draw_rotated_text(
    painter: &Painter,
    center: Pos2,
    _angle: f32,
    text: &str,
    font: egui::FontId,
    color: Color32,
) {
    painter.text(center, egui::Align2::CENTER_CENTER, text, font, color);
}

pub fn draw_frame_tags(
    painter: &Painter,
    frames: &[FrameType],
    line_start: Pos2,
    line_end: Pos2,
    _arrow_idx: usize,
    is_selected: bool,
    has_selection: bool,
) {
    if frames.is_empty() {
        return;
    }

    let mut grouped: Vec<(&FrameType, usize)> = Vec::new();
    for frame in frames {
        if let Some(last) = grouped.last_mut() {
            if last.0.display_name() == frame.display_name() {
                last.1 += 1;
                continue;
            }
        }
        grouped.push((frame, 1));
    }

    let dx = line_end.x - line_start.x;
    let dy = line_end.y - line_start.y;
    let line_len = (dx * dx + dy * dy).sqrt();
    if line_len < 1.0 {
        return;
    }

    let ux = dx / line_len;
    let uy = dy / line_len;
    let px = -uy;
    let py = ux;

    let tag_height = 16.0;
    let tag_gap = 4.0;

    let tag_data: Vec<(String, Color32, f32)> = grouped
        .iter()
        .map(|(frame, count)| {
            let text = if *count > 1 {
                format!("{} x{}", frame.display_name(), count)
            } else {
                frame.display_name()
            };
            let width = (text.len() as f32 * 6.0 + 12.0).clamp(50.0, 120.0);
            (text, frame.color(), width)
        })
        .collect();

    let num_tags = tag_data.len();
    let total_width: f32 = tag_data.iter().map(|(_, _, w)| w + tag_gap).sum::<f32>() - tag_gap;

    let mid_x = (line_start.x + line_end.x) / 2.0;
    let mid_y = (line_start.y + line_end.y) / 2.0;
    let base_perp_offset = -18.0;
    let available_len = line_len * 0.8;
    let fits_in_one_row = total_width <= available_len;
    let row_spacing = tag_height + 2.0;

    if fits_in_one_row || num_tags <= 2 {
        let start_offset = -total_width / 2.0;
        let mut current_offset = start_offset;

        for (text, color, width) in &tag_data {
            let along_offset = current_offset + width / 2.0;
            let tag_center_x = mid_x + along_offset * ux + base_perp_offset * px;
            let tag_center_y = mid_y + along_offset * uy + base_perp_offset * py;

            let tag_rect = Rect::from_center_size(
                Pos2::new(tag_center_x, tag_center_y),
                Vec2::new(*width, tag_height),
            );

            let (bg_color, text_color) = if is_selected {
                (*color, Color32::WHITE)
            } else if has_selection {
                (
                    Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 40),
                    Color32::from_rgba_unmultiplied(255, 255, 255, 40),
                )
            } else {
                (*color, Color32::WHITE)
            };

            if is_selected {
                let shadow_offset = 2.0;
                let shadow_rect = Rect::from_center_size(
                    Pos2::new(tag_center_x + shadow_offset, tag_center_y + shadow_offset),
                    Vec2::new(*width, tag_height),
                );
                painter.rect_filled(
                    shadow_rect,
                    2.0,
                    Color32::from_rgba_unmultiplied(0, 0, 0, 80),
                );
            }

            painter.rect_filled(tag_rect, 2.0, bg_color);
            painter.text(
                tag_rect.center(),
                egui::Align2::CENTER_CENTER,
                text,
                egui::FontId::proportional(10.0),
                text_color,
            );

            current_offset += width + tag_gap;
        }
    } else {
        let half = num_tags.div_ceil(2);
        let rows: Vec<&[(String, Color32, f32)]> = vec![&tag_data[..half], &tag_data[half..]];

        for (row_idx, row_tags) in rows.iter().enumerate() {
            let row_width: f32 =
                row_tags.iter().map(|(_, _, w)| w + tag_gap).sum::<f32>() - tag_gap;
            let start_offset = -row_width / 2.0;
            let mut current_offset = start_offset;
            let row_perp_offset = base_perp_offset - (row_idx as f32) * row_spacing;

            for (text, color, width) in *row_tags {
                let along_offset = current_offset + width / 2.0;
                let tag_center_x = mid_x + along_offset * ux + row_perp_offset * px;
                let tag_center_y = mid_y + along_offset * uy + row_perp_offset * py;

                let tag_rect = Rect::from_center_size(
                    Pos2::new(tag_center_x, tag_center_y),
                    Vec2::new(*width, tag_height),
                );

                let (bg_color, text_color) = if is_selected {
                    (*color, Color32::WHITE)
                } else if has_selection {
                    (
                        Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 40),
                        Color32::from_rgba_unmultiplied(255, 255, 255, 40),
                    )
                } else {
                    (*color, Color32::WHITE)
                };

                if is_selected {
                    let shadow_offset = 2.0;
                    let shadow_rect = Rect::from_center_size(
                        Pos2::new(tag_center_x + shadow_offset, tag_center_y + shadow_offset),
                        Vec2::new(*width, tag_height),
                    );
                    painter.rect_filled(
                        shadow_rect,
                        2.0,
                        Color32::from_rgba_unmultiplied(0, 0, 0, 80),
                    );
                }

                painter.rect_filled(tag_rect, 2.0, bg_color);
                painter.text(
                    tag_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    text,
                    egui::FontId::proportional(10.0),
                    text_color,
                );

                current_offset += width + tag_gap;
            }
        }
    }
}
