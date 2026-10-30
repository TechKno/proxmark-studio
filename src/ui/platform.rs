//! macOS menu integration. The Windows UI keeps equivalent visible controls.
#[cfg(target_os = "macos")]
pub struct NativeMenu {
    _menu: muda::Menu,
}
#[cfg(not(target_os = "macos"))]
pub struct NativeMenu;
impl NativeMenu {
    pub fn install() -> Result<Self, String> {
        #[cfg(target_os = "macos")]
        {
            use muda::{
                accelerator::{Accelerator as A, Code as C, Modifiers as M},
                Menu, MenuItem, PredefinedMenuItem as P, Submenu,
            };
            let root = Menu::new();
            let app = Submenu::new("Proxmark Studio", true);
            let file = Submenu::new("File", true);
            let edit = Submenu::new("Edit", true);
            let view = Submenu::new("View", true);
            let tools = Submenu::new("Card", true);
            let window = Submenu::new("Window", true);
            let help = Submenu::new("Help", true);
            let item = |id: &str, name: &str, key: Option<C>, shift: bool| {
                MenuItem::with_id(
                    id,
                    name,
                    true,
                    key.map(|code| {
                        A::new(Some(if shift { M::META | M::SHIFT } else { M::META }), code)
                    }),
                )
            };
            app.append_items(&[
                &item("about", "About Proxmark Studio", None, false),
                &P::separator(),
                &item("settings", "Settings…", Some(C::Comma), false),
                &P::separator(),
                &P::services(None),
                &P::separator(),
                &P::hide(None),
                &P::hide_others(None),
                &P::show_all(None),
                &P::separator(),
                &P::quit(None),
            ])
            .map_err(|e| e.to_string())?;
            file.append_items(&[
                &item("open", "Open Card Image…", Some(C::KeyO), false),
                &item("save", "Export Card Image…", Some(C::KeyS), false),
                &P::separator(),
                &item("close", "Close Window", Some(C::KeyW), false),
            ])
            .map_err(|e| e.to_string())?;
            edit.append_items(&[
                &item("undo", "Undo", Some(C::KeyZ), false),
                &item("redo", "Redo", Some(C::KeyZ), true),
                &P::separator(),
                &item("cut", "Cut", Some(C::KeyX), false),
                &item("copy", "Copy", Some(C::KeyC), false),
                &item("paste", "Paste", Some(C::KeyV), false),
                &item("select-all", "Select All", Some(C::KeyA), false),
            ])
            .map_err(|e| e.to_string())?;
            view.append_items(&[
                &item("sidebar", "Toggle Sidebar", Some(C::KeyS), true),
                &item("logs", "Show/Hide Log", Some(C::KeyL), true),
                &P::separator(),
                &item("basic", "Basic Mode", Some(C::Digit1), false),
                &item("advanced", "Advanced Mode", Some(C::Digit2), false),
                &P::fullscreen(None),
            ])
            .map_err(|e| e.to_string())?;
            tools
                .append_items(&[
                    &item("recovery", "Classic Key Recovery", None, false),
                    &item("magic", "Magic-Card Clone", None, false),
                    &item("signal", "Signal Analysis", None, false),
                    &P::separator(),
                    &item("stop", "Stop Current Operation", Some(C::Period), false),
                ])
                .map_err(|e| e.to_string())?;
            window
                .append_items(&[&P::minimize(None)])
                .map_err(|e| e.to_string())?;
            help.append_items(&[&item("about", "Proxmark Studio Help", None, false)])
                .map_err(|e| e.to_string())?;
            root.append_items(&[&app, &file, &edit, &view, &tools, &window, &help])
                .map_err(|e| e.to_string())?;
            root.init_for_nsapp();
            window.set_as_windows_menu_for_nsapp();
            help.set_as_help_menu_for_nsapp();
            Ok(Self { _menu: root })
        }
        #[cfg(not(target_os = "macos"))]
        {
            Ok(Self)
        }
    }
    pub fn actions() -> Vec<String> {
        #[cfg(target_os = "macos")]
        {
            let mut v = vec![];
            while let Ok(e) = muda::MenuEvent::receiver().try_recv() {
                v.push(e.id.0);
            }
            v
        }
        #[cfg(not(target_os = "macos"))]
        {
            vec![]
        }
    }
}

/// Called on the UI thread. Never change the user's macOS preferences.
pub fn display_preferences() -> (Option<eframe::egui::Color32>, bool, bool) {
    #[cfg(target_os = "macos")]
    {
        use objc2_app_kit::{NSColor, NSColorSpace, NSWorkspace};
        let accent = NSColor::controlAccentColor()
            .colorUsingColorSpace(&NSColorSpace::sRGBColorSpace())
            .map(|c| {
                eframe::egui::Color32::from_rgb(
                    (c.redComponent().clamp(0.0, 1.0) * 255.0) as u8,
                    (c.greenComponent().clamp(0.0, 1.0) * 255.0) as u8,
                    (c.blueComponent().clamp(0.0, 1.0) * 255.0) as u8,
                )
            });
        let workspace = NSWorkspace::sharedWorkspace();
        (
            accent,
            workspace.accessibilityDisplayShouldIncreaseContrast(),
            workspace.accessibilityDisplayShouldReduceMotion(),
        )
    }
    #[cfg(not(target_os = "macos"))]
    {
        (None, false, false)
    }
}
