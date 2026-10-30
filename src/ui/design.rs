//! Shared visual components. Painted controls retain keyboard and accessibility roles.
use super::*;
use egui::{Align2, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, WidgetInfo, WidgetType};

pub fn surface(ui: &egui::Ui) -> Color32 {
    if ui.visuals().dark_mode {
        Color32::from_gray(35)
    } else {
        Color32::WHITE
    }
}
pub fn line(ui: &egui::Ui) -> Color32 {
    if ui.visuals().override_text_color.is_some() {
        return ui.visuals().widgets.noninteractive.bg_stroke.color;
    }
    if ui.visuals().dark_mode {
        Color32::from_gray(57)
    } else {
        Color32::from_gray(226)
    }
}
pub fn accent(ui: &egui::Ui) -> Color32 {
    if ui.visuals().dark_mode {
        Color32::from_rgb(116, 180, 255)
    } else {
        Color32::from_rgb(24, 94, 192)
    }
}
pub fn icon(p: &egui::Painter, rect: Rect, name: &str, color: Color32) {
    let pos = |x: f32, y: f32| {
        Pos2::new(
            rect.left() + x * rect.width() / 24.0,
            rect.top() + y * rect.height() / 24.0,
        )
    };
    let stroke = Stroke::new((rect.width() / 18.0).clamp(1.25, 2.4), color);
    let path = |points: &[(f32, f32)]| {
        p.add(egui::Shape::line(
            points.iter().map(|&(x, y)| pos(x, y)).collect(),
            stroke,
        ));
    };
    let box_at = |x: f32, y: f32, w: f32, h: f32| {
        p.rect_stroke(
            Rect::from_min_max(pos(x, y), pos(x + w, y + h)),
            2,
            stroke,
            StrokeKind::Inside,
        );
    };
    match name {
        "Home" | "Overview" => {
            path(&[(3.0, 10.0), (12.0, 3.0), (21.0, 10.0)]);
            path(&[
                (5.0, 9.0),
                (5.0, 21.0),
                (10.0, 21.0),
                (10.0, 15.0),
                (14.0, 15.0),
                (14.0, 21.0),
                (19.0, 21.0),
                (19.0, 9.0),
            ]);
        }
        "Read" | "Tag tools" => {
            box_at(6.0, 5.0, 12.0, 15.0);
            path(&[(2.0, 8.0), (2.0, 16.0)]);
            path(&[(22.0, 8.0), (22.0, 16.0)]);
            path(&[(10.0, 10.0), (14.0, 10.0)]);
            path(&[(10.0, 14.0), (13.0, 14.0)]);
        }
        "Clone" => {
            box_at(3.0, 3.0, 12.0, 14.0);
            box_at(9.0, 8.0, 12.0, 14.0);
        }
        "Write" => {
            path(&[
                (4.0, 17.0),
                (3.0, 22.0),
                (8.0, 21.0),
                (21.0, 8.0),
                (16.0, 3.0),
                (4.0, 17.0),
            ]);
            path(&[(13.0, 6.0), (18.0, 11.0)]);
        }
        "Library" | "Saved profiles" => {
            box_at(3.0, 4.0, 5.0, 17.0);
            box_at(10.0, 4.0, 5.0, 17.0);
            path(&[
                (17.0, 5.0),
                (21.0, 4.0),
                (23.0, 20.0),
                (19.0, 21.0),
                (17.0, 5.0),
            ]);
        }
        "History" => {
            p.circle_stroke(pos(12.0, 12.0), rect.width() * 0.38, stroke);
            path(&[(12.0, 6.0), (12.0, 12.0), (16.0, 14.0)]);
        }
        "Sidebar" => {
            box_at(2.0, 4.0, 20.0, 16.0);
            path(&[(9.0, 4.0), (9.0, 20.0)]);
        }
        "Console" | "Logs" => {
            box_at(2.0, 4.0, 20.0, 16.0);
            path(&[(6.0, 9.0), (9.0, 12.0), (6.0, 15.0)]);
            path(&[(12.0, 15.0), (17.0, 15.0)]);
        }
        "Settings" | "Client & firmware" => {
            for (x, y) in [(6.0, 9.0), (12.0, 16.0), (18.0, 7.0)] {
                path(&[(x, 3.0), (x, 21.0)]);
                p.circle_filled(pos(x, y), rect.width() * 0.10, color);
            }
        }
        "Advanced tools" | "Signal" => {
            path(&[
                (2.0, 12.0),
                (5.0, 12.0),
                (8.0, 4.0),
                (12.0, 21.0),
                (16.0, 7.0),
                (19.0, 12.0),
                (22.0, 12.0),
            ]);
        }
        "DESFire" | "Classic cards" => {
            box_at(5.0, 10.0, 14.0, 12.0);
            path(&[
                (8.0, 10.0),
                (8.0, 6.0),
                (10.0, 3.0),
                (14.0, 3.0),
                (16.0, 6.0),
                (16.0, 10.0),
            ]);
            p.circle_filled(pos(12.0, 15.0), rect.width() * 0.065, color);
            path(&[(12.0, 15.0), (12.0, 18.0)]);
        }
        "Card memory" | "Credentials & batch" => {
            for y in [4.0, 10.0, 16.0] {
                for x in [4.0, 12.0] {
                    box_at(x, y, 7.0, 5.0);
                }
            }
        }
        _ => {
            box_at(4.0, 3.0, 16.0, 18.0);
            path(&[(8.0, 7.0), (16.0, 7.0)]);
            p.circle_filled(pos(12.0, 16.0), rect.width() * 0.075, color);
        }
    }
}
pub fn icon_button(ui: &mut egui::Ui, name: &str) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(30.0), Sense::click());
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, ui.is_enabled(), name));
    if response.hovered() || response.has_focus() {
        ui.painter()
            .rect_filled(rect, 7, ui.visuals().widgets.hovered.weak_bg_fill);
    }
    if response.has_focus() {
        ui.painter().rect_stroke(
            rect,
            7,
            Stroke::new(1.0_f32, accent(ui)),
            StrokeKind::Inside,
        );
    }
    icon(
        ui.painter(),
        rect.shrink(6.0),
        name,
        appearance::secondary(ui),
    );
    response.on_hover_text(name)
}
pub fn mode_picker(ui: &mut egui::Ui, mode: &mut Mode) {
    egui::Frame::new()
        .fill(if ui.visuals().dark_mode {
            Color32::from_gray(42)
        } else {
            Color32::from_gray(232)
        })
        .corner_radius(8)
        .inner_margin(3)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            for (value, label) in [
                (Mode::Basic, "Basic"),
                (Mode::NetAccess, "Organisations"),
                (Mode::Advanced, "Advanced"),
            ] {
                let (rect, response) = ui.allocate_exact_size(
                    Vec2::new(
                        if value == Mode::NetAccess {
                            120.0
                        } else {
                            78.0
                        },
                        26.0,
                    ),
                    Sense::click(),
                );
                response.widget_info(|| {
                    WidgetInfo::selected(
                        WidgetType::SelectableLabel,
                        ui.is_enabled(),
                        *mode == value,
                        label,
                    )
                });
                if *mode == value {
                    ui.painter().rect(
                        rect,
                        6,
                        if ui.visuals().dark_mode {
                            Color32::from_gray(61)
                        } else {
                            surface(ui)
                        },
                        Stroke::new(0.5_f32, line(ui)),
                        StrokeKind::Inside,
                    );
                }
                if response.has_focus() {
                    ui.painter().rect_stroke(
                        rect,
                        6,
                        Stroke::new(1.0_f32, accent(ui)),
                        StrokeKind::Inside,
                    );
                }
                ui.painter().text(
                    rect.center(),
                    Align2::CENTER_CENTER,
                    label,
                    FontId::proportional(13.0),
                    if *mode == value {
                        ui.visuals().text_color()
                    } else {
                        appearance::secondary(ui)
                    },
                );
                if response.clicked() {
                    *mode = value;
                }
            }
        });
}
pub fn task_tile_height(ui: &egui::Ui, name: &str, description: &str) -> f32 {
    let size = ui.text_style_height(&egui::TextStyle::Body);
    let heading = ui.painter().layout_no_wrap(
        name.into(),
        FontId::proportional(size + 6.0),
        ui.visuals().text_color(),
    );
    let body = ui.painter().layout(
        description.into(),
        FontId::proportional(size),
        appearance::secondary(ui),
        (ui.available_width() - 44.0).max(40.0),
    );
    (86.0 + heading.size().y + 12.0 + body.size().y + 54.0).max(226.0)
}
pub fn task_tile(
    ui: &mut egui::Ui,
    name: &str,
    description: &str,
    cta: &str,
    index: usize,
    height: f32,
) -> bool {
    let size = ui.text_style_height(&egui::TextStyle::Body);
    let galley = ui.painter().layout(
        description.into(),
        FontId::proportional(size),
        appearance::secondary(ui),
        (ui.available_width() - 44.0).max(40.0),
    );
    let heading = ui.painter().layout_no_wrap(
        name.into(),
        FontId::proportional(size + 6.0),
        ui.visuals().text_color(),
    );
    let description_y = 86.0 + heading.size().y + 12.0;
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::click());
    response.widget_info(|| {
        WidgetInfo::labeled(
            WidgetType::Button,
            ui.is_enabled(),
            format!("{name} a card. {description}"),
        )
    });
    let border = if response.hovered() || response.has_focus() {
        accent(ui)
    } else {
        line(ui)
    };
    ui.painter().rect(
        rect,
        14,
        surface(ui),
        Stroke::new(1.0_f32, border),
        StrokeKind::Inside,
    );
    let color = match index {
        1 => {
            if ui.visuals().dark_mode {
                Color32::from_rgb(178, 158, 245)
            } else {
                Color32::from_rgb(114, 83, 183)
            }
        }
        2 => {
            if ui.visuals().dark_mode {
                Color32::from_rgb(106, 201, 173)
            } else {
                Color32::from_rgb(31, 127, 103)
            }
        }
        _ => accent(ui),
    };
    let badge = Rect::from_min_size(rect.min + egui::vec2(22.0, 22.0), Vec2::splat(48.0));
    ui.painter()
        .rect_filled(badge, 12, color.gamma_multiply(0.10));
    icon(ui.painter(), badge.shrink(11.0), name, color);
    ui.painter().galley(
        rect.min + egui::vec2(22.0, 86.0),
        heading,
        ui.visuals().text_color(),
    );
    ui.painter().galley(
        rect.min + egui::vec2(22.0, description_y),
        galley,
        appearance::secondary(ui),
    );
    ui.painter().text(
        rect.left_bottom() + egui::vec2(22.0, -25.0),
        Align2::LEFT_CENTER,
        cta,
        FontId::proportional(size),
        color,
    );
    ui.painter().text(
        rect.right_bottom() + egui::vec2(-24.0, -25.0),
        Align2::RIGHT_CENTER,
        "→",
        FontId::proportional(18.0),
        color,
    );
    response.clicked()
}
pub fn tool_row(ui: &mut egui::Ui, name: &str, description: &str, icon_name: &str) -> bool {
    let width = ui.available_width();
    let name_text = ui.painter().layout(
        name.into(),
        egui::TextStyle::Body.resolve(ui.style()),
        ui.visuals().text_color(),
        (width - 85.0).max(40.0),
    );
    let detail_text = ui.painter().layout(
        description.into(),
        egui::TextStyle::Small.resolve(ui.style()),
        appearance::secondary(ui),
        (width - 85.0).max(40.0),
    );
    let height = (name_text.size().y + detail_text.size().y + 32.0).max(70.0);
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, height), Sense::click());
    response.widget_info(|| {
        WidgetInfo::labeled(
            WidgetType::Button,
            ui.is_enabled(),
            format!("{name}. {description}"),
        )
    });
    if response.hovered() || response.has_focus() {
        ui.painter()
            .rect_filled(rect, 9, ui.visuals().faint_bg_color);
    }
    icon(
        ui.painter(),
        Rect::from_center_size(
            Pos2::new(rect.left() + 24.0, rect.center().y),
            Vec2::splat(22.0),
        ),
        icon_name,
        accent(ui),
    );
    let detail_y = name_text.size().y + 21.0;
    ui.painter().galley(
        rect.min + egui::vec2(52.0, 13.0),
        name_text,
        ui.visuals().text_color(),
    );
    ui.painter().galley(
        rect.min + egui::vec2(52.0, detail_y),
        detail_text,
        appearance::secondary(ui),
    );
    ui.painter().text(
        rect.right_center() - egui::vec2(16.0, 0.0),
        Align2::CENTER_CENTER,
        "›",
        FontId::proportional(20.0),
        appearance::secondary(ui),
    );
    response.clicked()
}
