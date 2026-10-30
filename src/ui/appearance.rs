use super::*;
use proxmark_studio::appearance::{Appearance, Preferences};
pub fn secondary(ui: &egui::Ui) -> Color32 {
    ui.visuals()
        .override_text_color
        .unwrap_or(if ui.visuals().dark_mode {
            Color32::from_gray(185)
        } else {
            Color32::from_gray(95)
        })
}
/// A left-aligned sidebar row with an exposed selectable-label role and focus ring.
pub fn sidebar_row(ui: &mut egui::Ui, selected: bool, label: &str) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 32.0), egui::Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::SelectableLabel,
            ui.is_enabled(),
            selected,
            label,
        )
    });
    let visual = ui.style().interact_selectable(&response, selected);
    if selected || response.hovered() || response.has_focus() {
        ui.painter().rect(
            rect,
            6,
            visual.weak_bg_fill,
            if response.has_focus() {
                egui::Stroke::new(1.0_f32, visual.fg_stroke.color)
            } else {
                egui::Stroke::NONE
            },
            egui::StrokeKind::Inside,
        );
    }
    design::icon(
        ui.painter(),
        egui::Rect::from_center_size(
            rect.left_center() + egui::vec2(18.0, 0.0),
            Vec2::splat(17.0),
        ),
        label,
        if selected {
            design::accent(ui)
        } else {
            secondary(ui)
        },
    );
    let mut job = egui::text::LayoutJob::simple(
        label.into(),
        egui::TextStyle::Body.resolve(ui.style()),
        visual.fg_stroke.color,
        (rect.width() - 46.0).max(32.0),
    );
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    job.wrap.overflow_character = Some('…');
    let galley = ui.fonts(|f| f.layout_job(job));
    let origin = rect.left_center() + egui::vec2(36.0, -galley.size().y / 2.0);
    ui.painter().galley(origin, galley, visual.fg_stroke.color);
    response.on_hover_text(label)
}
pub fn fonts(_ctx: &egui::Context) {
    #[cfg(target_os = "macos")]
    {
        let mut fonts = egui::FontDefinitions::default();
        for (name, path, family) in [
            (
                "mac-system",
                "/System/Library/Fonts/SFNS.ttf",
                egui::FontFamily::Proportional,
            ),
            (
                "mac-mono",
                "/System/Library/Fonts/SFNSMono.ttf",
                egui::FontFamily::Monospace,
            ),
        ] {
            if let Ok(bytes) = std::fs::read(path) {
                fonts
                    .font_data
                    .insert(name.into(), egui::FontData::from_owned(bytes).into());
                fonts
                    .families
                    .entry(family)
                    .or_default()
                    .insert(0, name.into());
            }
        }
        _ctx.set_fonts(fonts);
    }
}
pub fn apply(ctx: &egui::Context, p: &Preferences) {
    let (system_accent, system_contrast, system_reduce_motion) = platform::display_preferences();
    ctx.data_mut(|d| {
        d.insert_temp(
            egui::Id::new("platform-display"),
            (system_accent, system_contrast, system_reduce_motion),
        )
    });
    window_theme(ctx, p.appearance);
    ctx.set_theme(match p.appearance {
        Appearance::System => egui::ThemePreference::System,
        Appearance::Light => egui::ThemePreference::Light,
        Appearance::Dark => egui::ThemePreference::Dark,
    });
    for theme in [egui::Theme::Light, egui::Theme::Dark] {
        let dark = theme == egui::Theme::Dark;
        let mut style = (*ctx.style_of(theme)).clone();
        let mut v = if dark {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        };
        v.panel_fill = if dark {
            Color32::from_gray(25)
        } else {
            Color32::from_gray(250)
        };
        v.window_fill = v.panel_fill;
        v.widgets.noninteractive.fg_stroke.color = if dark {
            Color32::from_gray(235)
        } else {
            Color32::from_gray(30)
        };
        v.widgets.inactive.fg_stroke.color = v.widgets.noninteractive.fg_stroke.color;
        v.widgets.inactive.bg_fill = if dark {
            Color32::from_gray(65)
        } else {
            Color32::WHITE
        };
        v.widgets.inactive.weak_bg_fill = v.widgets.inactive.bg_fill;
        v.widgets.inactive.bg_stroke = egui::Stroke::new(
            0.7_f32,
            if dark {
                Color32::from_gray(90)
            } else {
                Color32::from_gray(205)
            },
        );
        v.faint_bg_color = if dark {
            Color32::from_gray(31)
        } else {
            Color32::from_gray(242)
        };
        let accent = system_accent.unwrap_or(if dark {
            Color32::from_rgb(100, 180, 255)
        } else {
            Color32::from_rgb(0, 91, 190)
        });
        let base = v.panel_fill;
        let mix = |a: u8, b: u8| ((u16::from(a) * 3 + u16::from(b) * 7) / 10) as u8;
        v.selection.bg_fill = Color32::from_rgb(
            mix(accent.r(), base.r()),
            mix(accent.g(), base.g()),
            mix(accent.b(), base.b()),
        );
        // Selection text remains legible for every system accent, including yellow.
        v.selection.stroke =
            egui::Stroke::new(1.0_f32, if dark { Color32::WHITE } else { Color32::BLACK });
        v.warn_fg_color = if dark {
            Color32::from_rgb(255, 201, 95)
        } else {
            Color32::from_rgb(120, 65, 0)
        };
        v.error_fg_color = if dark {
            Color32::from_rgb(255, 145, 145)
        } else {
            Color32::from_rgb(172, 25, 25)
        };
        if p.increase_contrast || system_contrast {
            v.override_text_color = Some(if dark { Color32::WHITE } else { Color32::BLACK });
            v.widgets.noninteractive.bg_stroke = egui::Stroke::new(
                1.0_f32,
                if dark {
                    Color32::from_gray(160)
                } else {
                    Color32::from_gray(90)
                },
            );
        }
        for w in [
            &mut v.widgets.inactive,
            &mut v.widgets.hovered,
            &mut v.widgets.active,
        ] {
            w.corner_radius = egui::CornerRadius::same(6);
        }
        style.visuals = v;
        style.spacing.item_spacing = Vec2::new(10.0, 10.0);
        style.spacing.button_padding = Vec2::new(10.0, 5.0);
        style.spacing.interact_size = Vec2::new(28.0, 28.0);
        style.animation_time = if p.reduce_motion || system_reduce_motion {
            0.0
        } else {
            0.1
        };
        let size = p.text_size.clamp(13.0, 20.0);
        for (t, s) in [
            (egui::TextStyle::Body, size),
            (egui::TextStyle::Button, size),
            (egui::TextStyle::Small, (size - 1.0).max(12.0)),
            (egui::TextStyle::Heading, size + 6.0),
            (egui::TextStyle::Monospace, size - 1.0),
        ] {
            style.text_styles.insert(
                t.clone(),
                egui::FontId::new(
                    s,
                    if t == egui::TextStyle::Monospace {
                        egui::FontFamily::Monospace
                    } else {
                        egui::FontFamily::Proportional
                    },
                ),
            );
        }
        ctx.set_style_of(theme, style);
    }
}
fn edit_event(ctx: &egui::Context, event: egui::Event) {
    let target = ctx.input(|i| {
        i.raw
            .viewports
            .iter()
            .find(|(_, v)| v.focused == Some(true))
            .map(|(id, _)| *id)
            .unwrap_or(egui::ViewportId::ROOT)
    });
    if target == ctx.viewport_id() {
        ctx.input_mut(|i| i.events.push(event));
    } else {
        ctx.data_mut(|d| {
            let id = egui::Id::new(("edit-events", target));
            let mut events = d.get_temp::<Vec<egui::Event>>(id).unwrap_or_default();
            events.push(event);
            d.insert_temp(id, events);
        });
    }
}
fn receive_edit_events(ctx: &egui::Context) {
    let id = egui::Id::new(("edit-events", ctx.viewport_id()));
    let events = ctx
        .data_mut(|d| d.remove_temp::<Vec<egui::Event>>(id))
        .unwrap_or_default();
    ctx.input_mut(|i| i.events.extend(events));
}
pub fn window_theme(ctx: &egui::Context, appearance: Appearance) {
    let id = egui::Id::new(("window-theme", ctx.viewport_id()));
    if ctx.data(|d| d.get_temp::<Appearance>(id)) == Some(appearance) {
        return;
    }
    ctx.data_mut(|d| d.insert_temp(id, appearance));
    let theme = match appearance {
        Appearance::System => egui::SystemTheme::SystemDefault,
        Appearance::Light => egui::SystemTheme::Light,
        Appearance::Dark => egui::SystemTheme::Dark,
    };
    ctx.send_viewport_cmd(egui::ViewportCommand::SetTheme(theme));
    if ctx.viewport_id() != egui::ViewportId::ROOT {
        ctx.send_viewport_cmd_to(
            egui::ViewportId::ROOT,
            egui::ViewportCommand::SetTheme(theme),
        );
    }
}
pub fn refresh_system(ctx: &egui::Context, p: &Preferences) {
    let current = platform::display_preferences();
    let previous = ctx
        .data(|d| d.get_temp::<(Option<Color32>, bool, bool)>(egui::Id::new("platform-display")));
    if previous != Some(current) {
        apply(ctx, p);
        ctx.request_repaint();
    }
}
impl Studio {
    pub(super) fn open_settings(&mut self, ctx: &egui::Context) {
        self.show_settings = true;
        ctx.data_mut(|d| d.insert_temp(egui::Id::new("focus-settings"), true));
    }
    pub(super) fn platform_actions(&mut self, ctx: &egui::Context) {
        #[allow(unused_mut)]
        let mut actions = platform::NativeMenu::actions();
        #[cfg(not(target_os = "macos"))]
        {
            ctx.input_mut(|i| {
                for (key, id) in [
                    (egui::Key::Comma, "settings"),
                    (egui::Key::Num1, "basic"),
                    (egui::Key::Num2, "advanced"),
                    (egui::Key::O, "open"),
                    (egui::Key::S, "save"),
                    (egui::Key::Period, "stop"),
                ] {
                    if i.consume_key(egui::Modifiers::COMMAND, key) {
                        actions.push(id.into());
                    }
                }
            });
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
            && self.review.is_some()
        {
            self.review = None;
            self.review_ack = false;
        }
        let persist = actions.iter().any(|a| {
            matches!(
                a.as_str(),
                "sidebar" | "basic" | "advanced" | "recovery" | "magic" | "signal"
            )
        });
        for action in actions {
            match action.as_str() {
                "copy" => edit_event(ctx, egui::Event::Copy),
                "cut" => edit_event(ctx, egui::Event::Cut),
                #[cfg(target_os = "macos")]
                "paste" => {
                    if let Ok(mut clipboard) = arboard::Clipboard::new() {
                        if let Ok(text) = clipboard.get_text() {
                            edit_event(ctx, egui::Event::Paste(text));
                        }
                    }
                }
                "select-all" | "undo" | "redo" => {
                    let mut modifiers = egui::Modifiers::COMMAND;
                    modifiers.shift = action == "redo";
                    edit_event(
                        ctx,
                        egui::Event::Key {
                            key: if action == "select-all" {
                                egui::Key::A
                            } else {
                                egui::Key::Z
                            },
                            physical_key: None,
                            pressed: true,
                            repeat: false,
                            modifiers,
                        },
                    );
                }
                "close" => {
                    if self.show_settings {
                        self.show_settings = false;
                        ctx.send_viewport_cmd_to(
                            egui::ViewportId::ROOT,
                            egui::ViewportCommand::Focus,
                        );
                    } else {
                        ctx.send_viewport_cmd_to(
                            egui::ViewportId::ROOT,
                            egui::ViewportCommand::Close,
                        );
                    }
                }
                "about" => self.show_help = true,
                "settings" => self.open_settings(ctx),
                "sidebar" => {
                    self.settings.ui_preferences.show_sidebar =
                        !self.settings.ui_preferences.show_sidebar
                }
                "logs" => self.show_logs = !self.show_logs,
                "basic" => self.settings.ui_mode = Mode::Basic,
                "advanced" => self.settings.ui_mode = Mode::Advanced,
                "recovery" => {
                    self.settings.ui_mode = Mode::Advanced;
                    self.page = Page::Classic;
                    self.classic.tab = 0;
                }
                "magic" => {
                    self.settings.ui_mode = Mode::Advanced;
                    self.page = Page::Classic;
                    self.classic.tab = 1;
                }
                "signal" => {
                    self.settings.ui_mode = Mode::Advanced;
                    self.page = Page::Tools;
                    self.tools.tab = 3;
                }
                "stop" => {
                    self.tools.live = false;
                    if let Some(w) = &self.worker {
                        w.cancel();
                    }
                }
                "open" if self.worker.is_none() => {
                    self.open_image();
                }
                "save" if self.worker.is_none() => {
                    if let Some(d) = &self.bench.dump {
                        if let Some(p) = rfd::FileDialog::new()
                            .set_file_name("card-image.json")
                            .save_file()
                        {
                            match serde_json::to_vec_pretty(d)
                                .map_err(|e| e.to_string())
                                .and_then(|v| storage::write_private(&p, &v))
                            {
                                Ok(()) => self.status = "Card image exported".into(),
                                Err(e) => self.fail(e),
                            }
                        }
                    } else {
                        self.fail("Open or capture a card image before exporting");
                    }
                }
                _ => {}
            }
        }
        if persist {
            if let Err(e) = self.persist_ui() {
                self.fail(e);
            }
        }
    }
    fn open_image(&mut self) {
        if let Some(p) = rfd::FileDialog::new()
            .add_filter("Card image", &["json", "bin", "dump", "eml"])
            .pick_file()
        {
            match storage::read_bounded(&p, 65536).and_then(|v| {
                proxmark_studio::backup::import_memory(
                    &v,
                    p.extension().and_then(|v| v.to_str()).unwrap_or(""),
                    self.bench.geometry,
                    &self.bench.uid,
                )
            }) {
                Ok(d) => {
                    self.bench.geometry = d.geometry;
                    self.bench.uid = d.uid.clone();
                    self.bench.editor = command::encode(&d.bytes);
                    self.bench.dump = Some(d);
                    self.bench.draft_history.reset();
                    self.settings.ui_mode = Mode::Advanced;
                    self.page = Page::Workbench;
                }
                Err(e) => self.fail(e),
            }
        }
    }
    pub(super) fn settings_window(&mut self, ctx: &egui::Context) {
        if !self.show_settings {
            return;
        }
        let mut changed = false;
        let mut close = false;
        ctx.show_viewport_immediate(egui::ViewportId::from_hash_of("studio-settings"),egui::ViewportBuilder::default().with_title("Settings").with_inner_size([620.0,680.0]).with_min_inner_size([540.0,500.0]),|ctx,class|{
  window_theme(ctx,self.settings.ui_preferences.appearance);
  receive_edit_events(ctx);
  if ctx.data_mut(|d|d.remove_temp::<bool>(egui::Id::new("focus-settings"))).unwrap_or(false) {ctx.send_viewport_cmd(egui::ViewportCommand::Focus);}
  let content=|ui:&mut egui::Ui|{egui::ScrollArea::vertical().show(ui,|ui|{ui.heading("Appearance");ui.add_space(12.0);ui.horizontal(|ui|{ui.label("Appearance");for(v,label)in[(Appearance::System,"System"),(Appearance::Light,"Light"),(Appearance::Dark,"Dark")]{changed|=ui.selectable_value(&mut self.settings.ui_preferences.appearance,v,label).changed();}});changed|=ui.add(egui::Slider::new(&mut self.settings.ui_preferences.text_size,13.0..=20.0).text("Text size")).changed();changed|=ui.checkbox(&mut self.settings.ui_preferences.increase_contrast,"Increase contrast").changed();changed|=ui.checkbox(&mut self.settings.ui_preferences.reduce_motion,"Reduce motion").changed();changed|=ui.checkbox(&mut self.settings.ui_preferences.show_sidebar,"Show sidebar").changed();ui.separator();ui.label("The Mac uses its installed system fonts. System appearance follows the operating system. Mac accent, Increase Contrast and Reduce Motion preferences update while the app is open. Controls support keyboard focus and expose accessibility labels.");self.security_settings(ui);ui.add_space(10.0);if ui.button("Done").clicked(){close=true;}});};
  if class==egui::ViewportClass::Embedded{egui::Window::new("Settings").show(ctx,content);}else{egui::CentralPanel::default().frame(egui::Frame::new().fill(ctx.style().visuals.panel_fill).inner_margin(20)).show(ctx,content);}
  if ctx.input(|i|i.viewport().close_requested()){close=true;}
 });
        if changed {
            apply(ctx, &self.settings.ui_preferences);
            let _ = self.persist_ui();
        }
        if close {
            self.show_settings = false;
            ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Focus);
        }
    }
}

#[cfg(test)]
mod edit_tests {
    use super::*;
    #[test]
    fn deferred_edit_events_are_consumed_once_without_nested_context_locks() {
        let ctx = egui::Context::default();
        let id = egui::Id::new(("edit-events", egui::ViewportId::ROOT));
        ctx.data_mut(|d| d.insert_temp(id, vec![egui::Event::Paste("synthetic".into())]));
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            receive_edit_events(ctx);
            if ctx
                .data_mut(|d| d.remove_temp::<bool>(egui::Id::new("focus-settings")))
                .unwrap_or(false)
            {
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            }
            receive_edit_events(ctx);
            if ctx
                .data_mut(|d| d.remove_temp::<bool>(egui::Id::new("focus-settings")))
                .unwrap_or(false)
            {
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            }
            assert_eq!(
                ctx.input(|i| i
                    .events
                    .iter()
                    .filter(|e| matches!(e, egui::Event::Paste(_)))
                    .count()),
                1
            );
        });
    }
}
