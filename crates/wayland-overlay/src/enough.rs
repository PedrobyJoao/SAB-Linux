use std::{cell::RefCell, collections::HashMap, ffi::OsStr, rc::Rc, time::Duration};

use gtk::{gdk, gio, prelude::*};
use gtk4 as gtk;
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use hyprbreathe_core::{AppConfig, EnoughOverlaySpec, PointerMode};
use hyprbreathe_platform_linux::LinuxClock;

use super::{
    configure_input_region, configure_shortcut_inhibition, format_countdown, install_css,
    monitor_key,
};

pub fn run_enough(spec: EnoughOverlaySpec, config: AppConfig) -> Result<(), String> {
    if std::env::var("XDG_SESSION_TYPE").as_deref() != Ok("wayland") {
        return Err("Enough requires a Wayland session".into());
    }
    let instance = hyprbreathe_config::RuntimeInstance::current();
    let application = gtk::Application::builder()
        .application_id(instance.overlay_application_id())
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    application.connect_activate(move |application| {
        if !gtk4_layer_shell::is_supported() {
            tracing::error!("wlr-layer-shell is unavailable");
            application.quit();
            return;
        }
        install_css(&config);
        let hold = application.hold();
        let manager = Rc::new(RefCell::new(EnoughSurfaces {
            application: application.clone(),
            spec: spec.clone(),
            namespace: instance.overlay_namespace(),
            surfaces: HashMap::new(),
        }));
        manager.borrow_mut().reconcile();
        glib::timeout_add_local(Duration::from_millis(200), move || {
            let _hold = &hold;
            let mut manager = manager.borrow_mut();
            manager.reconcile();
            manager.update();
            glib::ControlFlow::Continue
        });
    });
    application.run_with_args(&[instance.name()]);
    Ok(())
}

struct EnoughSurface {
    monitor: gdk::Monitor,
    window: gtk::ApplicationWindow,
    label: gtk::Label,
}

struct EnoughSurfaces {
    application: gtk::Application,
    spec: EnoughOverlaySpec,
    namespace: &'static str,
    surfaces: HashMap<String, EnoughSurface>,
}

impl EnoughSurfaces {
    fn reconcile(&mut self) {
        let Some(display) = gdk::Display::default() else {
            return;
        };
        let model = display.monitors();
        let mut monitors: Vec<_> = (0..model.n_items())
            .filter_map(|index| model.item(index))
            .filter_map(|item| item.downcast::<gdk::Monitor>().ok())
            .enumerate()
            .map(|(index, monitor)| (monitor_key(&monitor, index), monitor))
            .collect();
        let popup = self.spec.deadline_boot_ms.is_some();
        if popup {
            monitors.truncate(1);
        }
        self.surfaces.retain(|key, surface| {
            let keep = monitors
                .iter()
                .any(|(candidate, monitor)| candidate == key && *monitor == surface.monitor)
                && surface.window.is_visible();
            if !keep {
                surface.window.destroy();
            }
            keep
        });
        for (key, monitor) in monitors {
            if !self.surfaces.contains_key(&key) {
                let surface = self.create(&monitor, popup);
                self.surfaces.insert(key, surface);
            }
        }
    }

    fn create(&self, monitor: &gdk::Monitor, popup: bool) -> EnoughSurface {
        let window = gtk::ApplicationWindow::builder()
            .application(&self.application)
            .decorated(false)
            .deletable(false)
            .build();
        window.add_css_class("hyprbreathe-overlay");
        window.init_layer_shell();
        window.set_namespace(Some(self.namespace));
        window.set_monitor(Some(monitor));
        window.set_layer(Layer::Overlay);
        window.set_exclusive_zone(-1);
        window.set_anchor(Edge::Top, true);
        window.set_anchor(Edge::Right, true);
        window.set_keyboard_mode(if popup {
            KeyboardMode::None
        } else {
            KeyboardMode::Exclusive
        });
        let panel = gtk::Box::new(gtk::Orientation::Vertical, 18);
        panel.add_css_class("hyprbreathe-panel");
        let title = gtk::Label::new(Some(if popup {
            "Time to finish"
        } else {
            "Enough for today"
        }));
        title.add_css_class("hyprbreathe-title");
        panel.append(&title);
        let label = gtk::Label::new(None);
        label.set_wrap(true);
        label.set_max_width_chars(40);
        label.set_justify(gtk::Justification::Center);
        panel.append(&label);
        if popup {
            window.set_margin(Edge::Top, 20);
            window.set_margin(Edge::Right, 20);
            panel.add_css_class("hyprbreathe-panel-compact");
            configure_input_region(&window, None, PointerMode::None);
        } else {
            window.set_anchor(Edge::Bottom, true);
            window.set_anchor(Edge::Left, true);
            panel.set_halign(gtk::Align::Center);
            panel.set_valign(gtk::Align::Center);
            label.set_text(&format!(
                "You'll have {} to save your work and close apps.\n\
                 Your computer will then shut down.\n\
                 The timer starts when you acknowledge.",
                self.spec.poweroff_countdown,
            ));
            let button = gtk::Button::with_label("Let me close my apps");
            button.add_css_class("hyprbreathe-action");
            let id = self.spec.session_id.to_string();
            let label = label.clone();
            button.connect_clicked(move |button| {
                acknowledge(button, &label, &id);
            });
            panel.append(&button);
        }
        window.set_child(Some(&panel));
        window.present();
        configure_shortcut_inhibition(&window, !popup);
        EnoughSurface {
            monitor: monitor.clone(),
            window,
            label,
        }
    }

    fn update(&self) {
        let Some(deadline) = self.spec.deadline_boot_ms else {
            return;
        };
        let text = if let Some(error) = &self.spec.error {
            format!(
                "Shutdown failed: {error}\n\
                Please shut down your computer manually."
            )
        } else if let Ok(now) = LinuxClock.sample() {
            let remaining = deadline.saturating_sub(now.boottime_ms);
            if remaining == 0 {
                "Requesting shutdown…".into()
            } else {
                format!(
                    "Shutdown in {}\nSave your work.",
                    format_countdown(Duration::from_millis(remaining))
                )
            }
        } else {
            "Unable to read the shutdown timer".into()
        };
        for surface in self.surfaces.values() {
            surface.label.set_text(&text);
        }
    }
}

fn acknowledge(button: &gtk::Button, label: &gtk::Label, id: &str) {
    let result = std::env::current_exe()
        .map_err(|e| e.to_string())
        .and_then(|executable| {
            gio::Subprocess::newv(
                &[
                    executable.as_os_str(),
                    OsStr::new("acknowledge-enough"),
                    OsStr::new(id),
                ],
                gio::SubprocessFlags::STDOUT_SILENCE | gio::SubprocessFlags::STDERR_SILENCE,
            )
            .map_err(|e| e.to_string())
        });
    let process = match result {
        Ok(process) => process,
        Err(error) => {
            label.set_text(&format!("Acknowledgment failed: {error}"));
            return;
        }
    };
    button.set_sensitive(false);
    let button = button.clone();
    let label = label.clone();
    glib::spawn_future_local(async move {
        if let Err(error) = process.wait_check_future().await {
            label.set_text(&format!("Acknowledgment failed: {error}"));
            button.set_sensitive(true);
        }
    });
}
