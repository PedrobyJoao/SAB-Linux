use std::{
    cell::{Cell, RefCell},
    process::Command as ProcessCommand,
    rc::Rc,
    sync::{Mutex, OnceLock},
    time::Duration,
};

use gtk::{gio, prelude::*};
use gtk4 as gtk;
use hyprbreathe_core::{
    AppConfig, Command, CompletionSound, ContentSelector, DisplayMode, DurationMs, PointerMode,
    StrictMode,
};
use serde::Deserialize;

const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
const RELEASES_URL: &str = "https://github.com/PedrobyJoao/hyprbreathe/releases/latest";
static IPC_RUNTIME: OnceLock<Result<Mutex<tokio::runtime::Runtime>, String>> = OnceLock::new();

pub fn run() -> Result<(), String> {
    let instance = hyprbreathe_config::RuntimeInstance::current();
    let initial = hyprbreathe_config::load().map_err(|error| error.to_string())?;
    let application = gtk::Application::builder()
        .application_id(instance.settings_application_id())
        .build();
    let window_holder = Rc::new(RefCell::new(None::<gtk::ApplicationWindow>));
    let holder_for_activate = window_holder.clone();
    application.connect_activate(move |application| {
        if let Some(window) = holder_for_activate.borrow().as_ref() {
            window.present();
            return;
        }
        install_css();
        let window = build_window(application, initial.clone(), instance);
        let holder_for_close = holder_for_activate.clone();
        window.connect_close_request(move |_| {
            holder_for_close.borrow_mut().take();
            glib::Propagation::Proceed
        });
        window.present();
        holder_for_activate.replace(Some(window));
    });
    let holder_for_shutdown = window_holder.clone();
    application.connect_shutdown(move |_| {
        holder_for_shutdown.borrow_mut().take();
    });
    application.run_with_args(&[instance.name()]);
    Ok(())
}

#[derive(Clone)]
struct SettingsWidgets {
    enough: EnoughWidgets,
    mini_interval: gtk::Entry,
    mini_duration: gtk::Entry,
    long_interval: gtk::Entry,
    long_duration: gtk::Entry,
    rest_interval: gtk::Entry,
    rest_duration: gtk::Entry,
    notifications_enabled: gtk::Switch,
    mini_notification_lead: gtk::Entry,
    long_notification_lead: gtk::Entry,
    rest_notification_lead: gtk::Entry,
    mini_postpone: PostponeWidgets,
    long_postpone: PostponeWidgets,
    rest_postpone: PostponeWidgets,
    strict_mode: gtk::DropDown,
    allow_postpone_during_lockout: gtk::Switch,
    inhibit_shortcuts: gtk::Switch,
    manual_resume: gtk::Switch,
    completion_sound: gtk::DropDown,
    display_mode: gtk::DropDown,
    content_selector: gtk::DropDown,
    pointer_mode: gtk::DropDown,
    show_message: gtk::Switch,
    tray_enabled: gtk::Switch,
}

#[derive(Clone)]
struct EnoughWidgets {
    enabled: gtk::Switch,
    limit: gtk::Entry,
    poweroff_countdown: gtk::Entry,
}

#[derive(Clone)]
struct PostponeWidgets {
    enabled: gtk::Switch,
    duration: gtk::Entry,
}

#[derive(Clone)]
struct CollaborationControls {
    page: gtk::ScrolledWindow,
    relay_entry: gtk::Entry,
    join_entry: gtk::Entry,
    invite_output: gtk::Entry,
    role_value: gtk::Label,
    connection_value: gtk::Label,
    participants_value: gtk::Label,
    relay_value: gtk::Label,
    error_value: gtk::Label,
    host_button: gtk::Button,
    join_button: gtk::Button,
    leave_button: gtk::Button,
    copy_button: gtk::Button,
    busy: Rc<Cell<bool>>,
    refreshing: Rc<Cell<bool>>,
}

#[derive(Debug, Deserialize)]
struct CoopUiStatus {
    mode: String,
    relay_url: Option<String>,
    connected: bool,
    host_present: bool,
    guest_count: usize,
    following_host: bool,
    last_error: Option<String>,
    invite: Option<String>,
}

enum CoopUiAction {
    Host(String),
    Join(String),
    Leave,
}

impl SettingsWidgets {
    fn collect(&self, base: &AppConfig) -> Result<AppConfig, String> {
        let mut config = base.clone();
        config.enough.enabled = self.enough.enabled.is_active();
        config.enough.limit = parse_duration(&self.enough.limit, "Enough daily limit")?;
        config.enough.poweroff_countdown =
            parse_duration(&self.enough.poweroff_countdown, "Power-off countdown")?;
        config.schedule.mini.interval = parse_duration(&self.mini_interval, "Mini interval")?;
        config.schedule.mini.duration = parse_duration(&self.mini_duration, "Mini duration")?;
        config.schedule.long.interval = parse_duration(&self.long_interval, "Long interval")?;
        config.schedule.long.duration = parse_duration(&self.long_duration, "Long duration")?;
        config.schedule.rest.interval = parse_duration(&self.rest_interval, "Rest interval")?;
        config.schedule.rest.duration = parse_duration(&self.rest_duration, "Rest duration")?;

        config.notifications.enabled = self.notifications_enabled.is_active();
        config.notifications.mini_lead =
            parse_duration(&self.mini_notification_lead, "Mini notification lead")?;
        config.notifications.long_lead =
            parse_duration(&self.long_notification_lead, "Long notification lead")?;
        config.notifications.rest_lead =
            parse_duration(&self.rest_notification_lead, "Rest notification lead")?;

        update_postpone_rule(
            &mut config.postpone.mini,
            &self.mini_postpone,
            "Mini postpone duration",
        )?;
        update_postpone_rule(
            &mut config.postpone.long,
            &self.long_postpone,
            "Long postpone duration",
        )?;
        update_postpone_rule(
            &mut config.postpone.rest,
            &self.rest_postpone,
            "Rest postpone duration",
        )?;

        config.strict.mode = strict_mode_from_index(self.strict_mode.selected());
        config.strict.allow_postpone_during_lockout =
            self.allow_postpone_during_lockout.is_active();
        config.strict.inhibit_shortcuts = self.inhibit_shortcuts.is_active();
        config.completion.manual_resume = self.manual_resume.is_active();
        config.completion.sound = completion_sound_from_index(self.completion_sound.selected());

        config.display.mode = display_mode_from_index(self.display_mode.selected());
        config.display.content_selector =
            content_selector_from_index(self.content_selector.selected());
        config.display.pointer_mode = pointer_mode_from_index(self.pointer_mode.selected());
        config.content.show_message = self.show_message.is_active();

        config.tray.enabled = self.tray_enabled.is_active();

        hyprbreathe_config::validate(&config).map_err(|error| error.to_string())?;
        Ok(config)
    }
}

fn build_window(
    application: &gtk::Application,
    initial: AppConfig,
    instance: hyprbreathe_config::RuntimeInstance,
) -> gtk::ApplicationWindow {
    let state = Rc::new(RefCell::new(initial.clone()));
    let (schedule_page, schedule_widgets) = schedule_page(&initial);
    let (actions_page, action_widgets) = actions_page(&initial);
    let (desktop_page, desktop_widgets) = desktop_page(&initial);
    let collaboration = collaboration_page(&initial);
    let (enough_page, enough_widgets) = enough_page(&initial);
    let widgets = SettingsWidgets {
        enough: enough_widgets,
        mini_interval: schedule_widgets.0,
        mini_duration: schedule_widgets.1,
        long_interval: schedule_widgets.2,
        long_duration: schedule_widgets.3,
        notifications_enabled: schedule_widgets.4,
        mini_notification_lead: schedule_widgets.5,
        long_notification_lead: schedule_widgets.6,
        rest_interval: schedule_widgets.7,
        rest_duration: schedule_widgets.8,
        rest_notification_lead: schedule_widgets.9,
        mini_postpone: action_widgets.0,
        long_postpone: action_widgets.1,
        rest_postpone: action_widgets.7,
        strict_mode: action_widgets.2,
        allow_postpone_during_lockout: action_widgets.3,
        inhibit_shortcuts: action_widgets.4,
        manual_resume: action_widgets.5,
        completion_sound: action_widgets.6,
        display_mode: desktop_widgets.0,
        content_selector: desktop_widgets.1,
        pointer_mode: desktop_widgets.2,
        show_message: desktop_widgets.3,
        tray_enabled: desktop_widgets.4,
    };

    let stack = gtk::Stack::builder()
        .transition_type(gtk::StackTransitionType::Crossfade)
        .transition_duration(140)
        .vexpand(true)
        .build();
    stack.add_titled(&schedule_page, Some("schedule"), "Schedule");
    stack.add_titled(&actions_page, Some("actions"), "Actions");
    stack.add_titled(&desktop_page, Some("desktop"), "Desktop");
    stack.add_titled(&enough_page, Some("enough"), "Enough");
    stack.add_titled(&collaboration.page, Some("collaboration"), "Collaboration");
    stack.set_visible_child_name("schedule");

    let switcher = gtk::StackSwitcher::builder()
        .stack(&stack)
        .halign(gtk::Align::Center)
        .build();
    let introduction = gtk::Box::new(gtk::Orientation::Vertical, 4);
    introduction.set_margin_top(16);
    introduction.set_margin_start(24);
    introduction.set_margin_end(24);
    let title = gtk::Label::new(Some("Break preferences"));
    title.set_halign(gtk::Align::Start);
    title.add_css_class("settings-intro-title");
    let description = gtk::Label::new(Some(
        "Set your break cadence and what can happen while a break is active.",
    ));
    description.set_halign(gtk::Align::Start);
    description.set_wrap(true);
    description.add_css_class("settings-description");
    introduction.append(&title);
    introduction.append(&description);

    let status_text = format!(
        "Settings are stored in {}.",
        hyprbreathe_config::config_path().display()
    );
    let status = gtk::Label::new(Some(&status_text));
    status.set_halign(gtk::Align::Start);
    status.set_wrap(true);
    status.set_hexpand(true);
    status.add_css_class("settings-status");
    let status_bar = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    status_bar.add_css_class("settings-status-bar");
    status_bar.append(&status);
    status_bar.append(&version_corner());
    collaboration.connect(status.clone());

    let root = gtk::Box::new(gtk::Orientation::Vertical, 12);
    root.append(&introduction);
    root.append(&switcher);
    root.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    root.append(&stack);
    root.append(&status_bar);

    let save = gtk::Button::with_label("Save");
    save.add_css_class("suggested-action");
    save.set_tooltip_text(Some("Validate, save, and reload the configuration"));
    let header = gtk::HeaderBar::new();
    let window_title = format!("{} Settings", instance.name());
    header.set_title_widget(Some(&gtk::Label::new(Some(&window_title))));
    header.pack_end(&save);

    let window = gtk::ApplicationWindow::builder()
        .application(application)
        .title(&window_title)
        .default_width(760)
        .default_height(720)
        .child(&root)
        .build();
    window.set_titlebar(Some(&header));

    let state_for_save = state.clone();
    save.connect_clicked(move |button| {
        let current =
            hyprbreathe_config::load().unwrap_or_else(|_| state_for_save.borrow().clone());
        let config = match widgets.collect(&current) {
            Ok(config) => config,
            Err(error) => {
                set_status(&status, &error, true);
                return;
            }
        };
        button.set_sensitive(false);
        set_status(&status, "Saving configuration...", false);

        let button = button.clone();
        let status = status.clone();
        let state = state_for_save.clone();
        glib::spawn_future_local(async move {
            let saved_config = config.clone();
            let result = gio::spawn_blocking(move || save_and_reload(&config)).await;
            button.set_sensitive(true);
            match result {
                Ok(Ok(message)) => {
                    state.replace(saved_config);
                    set_status(&status, &message, false);
                }
                Ok(Err(error)) => set_status(&status, &error, true),
                Err(_) => set_status(&status, "Saving failed unexpectedly.", true),
            }
        });
    });

    window
}

fn enough_page(config: &AppConfig) -> (gtk::ScrolledWindow, EnoughWidgets) {
    let enabled = gtk::Switch::builder()
        .active(config.enough.enabled)
        .valign(gtk::Align::Center)
        .build();
    let limit = duration_entry(config.enough.limit);
    let poweroff_countdown = duration_entry(config.enough.poweroff_countdown);
    let group = settings_group(
        "Enough for today",
        "Powers off your computer after your daily allowance. \
         Unsaved work may be lost. Idle pauses usage; it never \
         restores time. Resets at local midnight.",
        &[
            settings_row(
                "Enable Enough",
                "Optional daily limit with no postponement.",
                &enabled,
            ),
            settings_row(
                "Daily screen-time limit",
                "Total usage, excluding idle and reminder breaks.",
                &limit,
            ),
            settings_row(
                "Power-off countdown",
                "Time to save work after acknowledging the overlay.",
                &poweroff_countdown,
            ),
        ],
    );
    let explanation = settings_group(
        "Once Enough triggers",
        "The screen stays blocked until you acknowledge. \
         A corner timer then runs until shutdown, even while idle. \
         Changing settings cannot cancel a triggered session. \
         Daemon restarts preserve it; reboots do not yet preserve \
         your daily allowance.",
        &[],
    );
    (
        settings_page(&[group, explanation]),
        EnoughWidgets {
            enabled,
            limit,
            poweroff_countdown,
        },
    )
}

type SchedulePageWidgets = (
    gtk::Entry,
    gtk::Entry,
    gtk::Entry,
    gtk::Entry,
    gtk::Switch,
    gtk::Entry,
    gtk::Entry,
    gtk::Entry,
    gtk::Entry,
    gtk::Entry,
);

fn schedule_page(config: &AppConfig) -> (gtk::ScrolledWindow, SchedulePageWidgets) {
    let mini_interval = duration_entry(config.schedule.mini.interval);
    let mini_duration = duration_entry(config.schedule.mini.duration);
    let long_interval = duration_entry(config.schedule.long.interval);
    let long_duration = duration_entry(config.schedule.long.duration);
    let rest_interval = duration_entry(config.schedule.rest.interval);
    let rest_duration = duration_entry(config.schedule.rest.duration);

    let mini_group = settings_group(
        "Mini breaks",
        "Short, frequent pauses from keyboard and screen use.",
        &[
            settings_row(
                "Screen-time interval",
                "Accumulated screen time before a mini break.",
                &mini_interval,
            ),
            settings_row(
                "Duration",
                "How long the overlay remains visible.",
                &mini_duration,
            ),
        ],
    );
    let long_group = settings_group(
        "Long breaks",
        "Completing a long break also resets mini screen time.",
        &[
            settings_row(
                "Screen-time interval",
                "Accumulated screen time before a long break.",
                &long_interval,
            ),
            settings_row(
                "Duration",
                "How long the overlay remains visible.",
                &long_duration,
            ),
        ],
    );
    let rest_group = settings_group(
        "Rest breaks",
        "Completing a rest break resets all screen-time counters.",
        &[
            settings_row(
                "Screen-time interval",
                "Accumulated screen time before a rest break.",
                &rest_interval,
            ),
            settings_row(
                "Duration",
                "How long the overlay remains visible.",
                &rest_duration,
            ),
        ],
    );

    let notifications_enabled = gtk::Switch::builder()
        .active(config.notifications.enabled)
        .valign(gtk::Align::Center)
        .build();
    let mini_notification_lead = duration_entry(config.notifications.mini_lead);
    let long_notification_lead = duration_entry(config.notifications.long_lead);
    let rest_notification_lead = duration_entry(config.notifications.rest_lead);
    mini_notification_lead.set_sensitive(config.notifications.enabled);
    long_notification_lead.set_sensitive(config.notifications.enabled);
    rest_notification_lead.set_sensitive(config.notifications.enabled);
    let mini_lead_for_toggle = mini_notification_lead.clone();
    let long_lead_for_toggle = long_notification_lead.clone();
    let rest_lead_for_toggle = rest_notification_lead.clone();
    notifications_enabled.connect_active_notify(move |toggle| {
        mini_lead_for_toggle.set_sensitive(toggle.is_active());
        long_lead_for_toggle.set_sensitive(toggle.is_active());
        rest_lead_for_toggle.set_sensitive(toggle.is_active());
    });
    let notification_group = settings_group(
        "Notifications",
        "Show a desktop notification shortly before an overlay appears.",
        &[
            settings_row(
                "Pre-break notifications",
                "Master switch for mini, long, and rest break notices.",
                &notifications_enabled,
            ),
            settings_row(
                "Mini-break notice",
                "How early to notify before a mini break.",
                &mini_notification_lead,
            ),
            settings_row(
                "Long-break notice",
                "How early to notify before a long break.",
                &long_notification_lead,
            ),
            settings_row(
                "Rest-break notice",
                "How early to notify before a rest break.",
                &rest_notification_lead,
            ),
        ],
    );

    (
        settings_page(&[mini_group, long_group, rest_group, notification_group]),
        (
            mini_interval,
            mini_duration,
            long_interval,
            long_duration,
            notifications_enabled,
            mini_notification_lead,
            long_notification_lead,
            rest_interval,
            rest_duration,
            rest_notification_lead,
        ),
    )
}

type ActionPageWidgets = (
    PostponeWidgets,
    PostponeWidgets,
    gtk::DropDown,
    gtk::Switch,
    gtk::Switch,
    gtk::Switch,
    gtk::DropDown,
    PostponeWidgets,
);

fn actions_page(config: &AppConfig) -> (gtk::ScrolledWindow, ActionPageWidgets) {
    let (mini_postpone, mini_postpone_rows) = postpone_controls(&config.postpone.mini);
    let (long_postpone, long_postpone_rows) = postpone_controls(&config.postpone.long);
    let (rest_postpone, rest_postpone_rows) = postpone_controls(&config.postpone.rest);

    let mini_group = settings_group(
        "Mini-break actions",
        "Controls available while a mini break is active.",
        &mini_postpone_rows,
    );
    let long_group = settings_group(
        "Long-break actions",
        "Controls available while a long break is active.",
        &long_postpone_rows,
    );
    let rest_group = settings_group(
        "Rest-break actions",
        "Controls available while a rest break is active.",
        &rest_postpone_rows,
    );

    let strict_mode = gtk::DropDown::from_strings(&["Off", "Shortcuts only", "Entire break"]);
    strict_mode.set_selected(strict_mode_index(config.strict.mode));
    let allow_postpone_during_lockout = gtk::Switch::builder()
        .active(config.strict.allow_postpone_during_lockout)
        .valign(gtk::Align::Center)
        .build();
    let inhibit_shortcuts = gtk::Switch::builder()
        .active(config.strict.inhibit_shortcuts)
        .valign(gtk::Align::Center)
        .build();
    let strict_group = settings_group(
        "Strict mode",
        "Control when a visible break may be dismissed.",
        &[
            settings_row(
                "Mode",
                "Allow actions immediately or lock them for the break.",
                &strict_mode,
            ),
            settings_row(
                "Postpone during lockout",
                "Allow postponement before strict mode unlocks other controls.",
                &allow_postpone_during_lockout,
            ),
            settings_row(
                "Block desktop shortcuts",
                "Temporarily inhibit compositor bindings during strict breaks.",
                &inhibit_shortcuts,
            ),
        ],
    );

    let manual_resume = gtk::Switch::builder()
        .active(config.completion.manual_resume)
        .valign(gtk::Align::Center)
        .build();
    let completion_sound =
        gtk::DropDown::from_strings(&["Warm rise", "Soft bloom", "Deep halo", "Clean chime"]);
    completion_sound.set_selected(completion_sound_index(config.completion.sound));
    let completion_group = settings_group(
        "Break completion",
        "Choose what happens when a break reaches zero.",
        &[
            settings_row(
                "Completion sound",
                "Sound played when the countdown reaches zero.",
                &completion_sound,
            ),
            settings_row(
                "Manual resume",
                "Wait at zero until you press a key or click the overlay.",
                &manual_resume,
            ),
        ],
    );

    (
        settings_page(&[
            mini_group,
            long_group,
            rest_group,
            completion_group,
            strict_group,
        ]),
        (
            mini_postpone,
            long_postpone,
            strict_mode,
            allow_postpone_during_lockout,
            inhibit_shortcuts,
            manual_resume,
            completion_sound,
            rest_postpone,
        ),
    )
}

type DesktopPageWidgets = (
    gtk::DropDown,
    gtk::DropDown,
    gtk::DropDown,
    gtk::Switch,
    gtk::Switch,
);

fn desktop_page(config: &AppConfig) -> (gtk::ScrolledWindow, DesktopPageWidgets) {
    let display_mode = gtk::DropDown::from_strings(&[
        "Every monitor",
        "Focused monitor",
        "Cursor monitor",
        "Primary monitor",
        "Configured monitor",
        "Dim all, controls on one",
    ]);
    display_mode.set_selected(display_mode_index(config.display.mode));
    let content_selector = gtk::DropDown::from_strings(&[
        "Focused monitor",
        "Cursor monitor",
        "Primary monitor",
        "Configured monitor",
    ]);
    content_selector.set_selected(content_selector_index(config.display.content_selector));
    content_selector.set_sensitive(config.display.mode == DisplayMode::DimAllContentOne);
    let content_selector_for_mode = content_selector.clone();
    display_mode.connect_selected_notify(move |dropdown| {
        content_selector_for_mode.set_sensitive(
            display_mode_from_index(dropdown.selected()) == DisplayMode::DimAllContentOne,
        );
    });

    let pointer_mode =
        gtk::DropDown::from_strings(&["Controls only", "Block all clicks", "Click through"]);
    pointer_mode.set_selected(pointer_mode_index(config.display.pointer_mode));
    let show_message = gtk::Switch::builder()
        .active(config.content.show_message)
        .valign(gtk::Align::Center)
        .build();
    let display_group = settings_group(
        "Overlay",
        "Choose where breaks appear and how the desktop behaves underneath.",
        &[
            settings_row("Display mode", "Monitor coverage policy.", &display_mode),
            settings_row(
                "Controls monitor",
                "Used when every monitor is dimmed.",
                &content_selector,
            ),
            settings_row(
                "Pointer input",
                "Whether clicks reach applications below.",
                &pointer_mode,
            ),
            settings_row(
                "Exercise suggestion",
                "Show one configured message during each break.",
                &show_message,
            ),
        ],
    );

    let tray_enabled = gtk::Switch::builder()
        .active(config.tray.enabled)
        .valign(gtk::Align::Center)
        .build();
    let integration_group = settings_group(
        "Desktop integration",
        "Optional behavior outside the break overlay.",
        &[settings_row(
            "Tray indicator",
            "Show schedule status and quick actions in your panel.",
            &tray_enabled,
        )],
    );

    let advanced = gtk::Label::new(Some(
        "Monitor identifiers, recovery policy, messages, and logging remain available in config.toml.",
    ));
    advanced.set_wrap(true);
    advanced.set_halign(gtk::Align::Start);
    advanced.add_css_class("settings-description");
    let advanced_group = gtk::Box::new(gtk::Orientation::Vertical, 0);
    advanced_group.append(&advanced);

    (
        settings_page(&[display_group, integration_group, advanced_group]),
        (
            display_mode,
            content_selector,
            pointer_mode,
            show_message,
            tray_enabled,
        ),
    )
}

fn collaboration_page(config: &AppConfig) -> CollaborationControls {
    let role_value = collaboration_value_label();
    let connection_value = collaboration_value_label();
    let participants_value = collaboration_value_label();
    let relay_value = collaboration_value_label();
    let error_value = gtk::Label::new(None);
    error_value.set_halign(gtk::Align::Start);
    error_value.set_wrap(true);
    error_value.set_visible(false);
    error_value.add_css_class("settings-error");
    let status_group = settings_group(
        "Room status",
        "Connection state is refreshed from the running hyprbreathe daemon.",
        &[
            settings_row("Role", "Your role in the current room.", &role_value),
            settings_row(
                "Connection",
                "Whether schedule snapshots are flowing.",
                &connection_value,
            ),
            settings_row(
                "Participants",
                "Guests connected to the host room.",
                &participants_value,
            ),
            settings_row("Relay", "The active WebSocket relay.", &relay_value),
        ],
    );
    status_group.append(&error_value);

    let relay_entry = gtk::Entry::builder()
        .text(config.coop.relay_url.as_deref().unwrap_or_default())
        .placeholder_text("lambda-1.example.ts.net:8787")
        .width_chars(30)
        .max_width_chars(48)
        .build();
    relay_entry.set_tooltip_text(Some(
        "A Tailscale DNS name or complete ws:// / wss:// relay URL",
    ));
    let host_button = gtk::Button::with_label("Host new room");
    host_button.add_css_class("suggested-action");
    let invite_output = gtk::Entry::builder()
        .editable(false)
        .placeholder_text("Your invite appears here")
        .width_chars(30)
        .max_width_chars(48)
        .build();
    invite_output.add_css_class("settings-secret");
    let copy_button = gtk::Button::with_label("Copy invite");
    copy_button.set_sensitive(false);
    let invite_controls = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    invite_controls.append(&invite_output);
    invite_controls.append(&copy_button);
    let host_group = settings_group(
        "Host a room",
        "Use your host's Tailscale MagicDNS name so every shared user reaches the correct address. The relay must already be listening on that port.",
        &[
            settings_row(
                "Relay address",
                "A DNS name with port, or a complete WebSocket URL.",
                &relay_entry,
            ),
            settings_row(
                "Create or rotate room",
                "Generates a new secret invite and invalidates this host's previous room.",
                &host_button,
            ),
            settings_row(
                "Invite",
                "Share this complete value privately with your collaborators.",
                &invite_controls,
            ),
        ],
    );

    let join_entry = gtk::Entry::builder()
        .placeholder_text("ws://host:8787/ws#hyprbreathe=...")
        .width_chars(30)
        .max_width_chars(48)
        .build();
    join_entry.add_css_class("settings-secret");
    let join_button = gtk::Button::with_label("Join room");
    join_button.add_css_class("suggested-action");
    let join_group = settings_group(
        "Join a room",
        "Paste the complete invite sent by the host. Do not open it in a browser.",
        &[
            settings_row(
                "Room invite",
                "Includes the relay URL and secret room token.",
                &join_entry,
            ),
            settings_row(
                "Follow host",
                "Adopt the host's schedule and coordination policy.",
                &join_button,
            ),
        ],
    );

    let leave_button = gtk::Button::with_label("Leave room");
    leave_button.add_css_class("destructive-action");
    leave_button.set_sensitive(config.coop.mode != hyprbreathe_core::CoopMode::Off);
    let leave_group = settings_group(
        "Leave collaboration",
        "Leaving clears the room secret and starts a fresh local schedule.",
        &[settings_row(
            "Disconnect",
            "Stop hosting or following the current room.",
            &leave_button,
        )],
    );

    CollaborationControls {
        page: settings_page(&[status_group, host_group, join_group, leave_group]),
        relay_entry,
        join_entry,
        invite_output,
        role_value,
        connection_value,
        participants_value,
        relay_value,
        error_value,
        host_button,
        join_button,
        leave_button,
        copy_button,
        busy: Rc::new(Cell::new(false)),
        refreshing: Rc::new(Cell::new(false)),
    }
}

fn collaboration_value_label() -> gtk::Label {
    let label = gtk::Label::new(Some("—"));
    label.set_halign(gtk::Align::End);
    label.set_wrap(true);
    label.set_width_chars(22);
    label.set_max_width_chars(32);
    label.set_xalign(1.0);
    label.set_selectable(true);
    label
}

impl CollaborationControls {
    fn connect(&self, settings_status: gtk::Label) {
        let controls = self.clone();
        let status = settings_status.clone();
        self.host_button.connect_clicked(move |_| {
            controls.run_action(
                CoopUiAction::Host(controls.relay_entry.text().to_string()),
                status.clone(),
            );
        });

        let controls = self.clone();
        let status = settings_status.clone();
        self.join_button.connect_clicked(move |_| {
            controls.run_action(
                CoopUiAction::Join(controls.join_entry.text().to_string()),
                status.clone(),
            );
        });

        let controls = self.clone();
        let status = settings_status.clone();
        self.join_entry.connect_activate(move |_| {
            controls.run_action(
                CoopUiAction::Join(controls.join_entry.text().to_string()),
                status.clone(),
            );
        });

        let controls = self.clone();
        let status = settings_status.clone();
        self.leave_button.connect_clicked(move |_| {
            controls.run_action(CoopUiAction::Leave, status.clone());
        });

        let invite = self.invite_output.clone();
        let status = settings_status.clone();
        self.copy_button.connect_clicked(move |_| {
            if invite.text().is_empty() {
                return;
            }
            if let Some(display) = gtk::gdk::Display::default() {
                display.clipboard().set_text(&invite.text());
                set_status(&status, "Co-op invite copied to the clipboard.", false);
            }
        });

        self.refresh();
        let controls = self.clone();
        glib::timeout_add_seconds_local(1, move || {
            controls.refresh();
            glib::ControlFlow::Continue
        });
    }

    fn run_action(&self, action: CoopUiAction, settings_status: gtk::Label) {
        if self.busy.replace(true) {
            return;
        }
        self.set_action_buttons(false);
        self.set_local_error(None);
        set_status(&settings_status, "Updating co-op room...", false);

        let controls = self.clone();
        glib::spawn_future_local(async move {
            let result = gio::spawn_blocking(move || perform_coop_action(action)).await;
            controls.busy.set(false);
            match result {
                Ok(Ok(status)) => {
                    controls.apply_status(&status);
                    set_status(
                        &settings_status,
                        match (
                            status.mode.as_str(),
                            status.connected,
                            status.following_host,
                        ) {
                            ("host", true, _) => "Co-op room is hosted and ready.",
                            ("host", false, _) => "Room created; connecting to the co-op relay.",
                            ("guest", _, true) => "Joined the co-op room and following its host.",
                            ("guest", _, false) => "Invite saved; connecting to the co-op host.",
                            _ => "Left the co-op room; local schedule reset.",
                        },
                        false,
                    );
                }
                Ok(Err(error)) => {
                    controls.set_action_buttons(true);
                    controls.set_local_error(Some(&error));
                    set_status(&settings_status, &error, true);
                }
                Err(_) => {
                    controls.set_action_buttons(true);
                    controls.set_local_error(Some("Co-op action failed unexpectedly."));
                    set_status(&settings_status, "Co-op action failed unexpectedly.", true);
                }
            }
        });
    }

    fn refresh(&self) {
        if self.busy.get() || self.refreshing.replace(true) {
            return;
        }
        let controls = self.clone();
        glib::spawn_future_local(async move {
            let result = gio::spawn_blocking(load_coop_status).await;
            controls.refreshing.set(false);
            match result {
                Ok(Ok(status)) => controls.apply_status(&status),
                Ok(Err(error)) => controls.set_local_error(Some(&error)),
                Err(_) => controls.set_local_error(Some("Could not read co-op status.")),
            }
        });
    }

    fn apply_status(&self, status: &CoopUiStatus) {
        self.role_value.set_text(match status.mode.as_str() {
            "host" => "Host",
            "guest" => "Guest",
            _ => "Local only",
        });
        self.connection_value.set_text(match status.mode.as_str() {
            "host" if status.connected => "Relay connected",
            "host" => "Connecting to relay…",
            "guest" if status.following_host => "Following host schedule",
            "guest" if status.connected && status.host_present => "Waiting for first snapshot…",
            "guest" if status.connected => "Waiting for host…",
            "guest" => "Connecting to relay…",
            _ => "Not in a room",
        });
        let participants = match status.mode.as_str() {
            "host" => format!(
                "{} connected guest{}",
                status.guest_count,
                if status.guest_count == 1 { "" } else { "s" }
            ),
            "guest" if status.host_present => "Host is present".into(),
            "guest" => "Host is unavailable".into(),
            _ => "—".into(),
        };
        self.participants_value.set_text(&participants);
        self.relay_value
            .set_text(status.relay_url.as_deref().unwrap_or("—"));
        self.invite_output
            .set_text(status.invite.as_deref().unwrap_or_default());
        self.copy_button.set_sensitive(
            status
                .invite
                .as_ref()
                .is_some_and(|invite| !invite.is_empty()),
        );
        self.leave_button.set_sensitive(status.mode != "off");
        self.set_action_buttons(true);
        self.set_local_error(status.last_error.as_deref());
    }

    fn set_action_buttons(&self, enabled: bool) {
        self.host_button.set_sensitive(enabled);
        self.join_button.set_sensitive(enabled);
        if !enabled {
            self.leave_button.set_sensitive(false);
            self.copy_button.set_sensitive(false);
        }
    }

    fn set_local_error(&self, error: Option<&str>) {
        self.error_value.set_text(error.unwrap_or_default());
        self.error_value
            .set_visible(error.is_some_and(|error| !error.is_empty()));
    }
}

fn perform_coop_action(action: CoopUiAction) -> Result<CoopUiStatus, String> {
    let command = match action {
        CoopUiAction::Host(input) => {
            let relay = normalize_relay_input(&input)?;
            Command::CoopHost { relay_url: relay }
        }
        CoopUiAction::Join(invite) => {
            let invite = invite.trim();
            if invite.is_empty() {
                return Err("Paste a complete co-op invite before joining.".into());
            }
            Command::CoopJoin {
                invite: invite.to_owned(),
            }
        }
        CoopUiAction::Leave => Command::CoopLeave,
    };
    request_daemon(command)?;
    load_coop_status()
}

fn load_coop_status() -> Result<CoopUiStatus, String> {
    serde_json::from_value(request_daemon(Command::CoopStatus)?)
        .map_err(|error| format!("Invalid co-op status: {error}"))
}

fn request_daemon(command: Command) -> Result<serde_json::Value, String> {
    let runtime = IPC_RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map(Mutex::new)
            .map_err(|error| format!("Could not initialize co-op IPC: {error}"))
    });
    let runtime = runtime.as_ref().map_err(Clone::clone)?;
    let runtime = runtime
        .lock()
        .map_err(|_| "Co-op IPC became unavailable.".to_owned())?;
    let response = runtime
        .block_on(async {
            tokio::time::timeout(
                Duration::from_secs(2),
                hyprbreathe_ipc::request(hyprbreathe_config::socket_path(), command),
            )
            .await
        })
        .map_err(|_| "The hyprbreathe daemon response timed out.".to_owned())?
        .map_err(|error| format!("Could not contact the hyprbreathe daemon: {error}"))?;
    if !response.ok {
        return Err(response.message);
    }
    Ok(response.data.unwrap_or(serde_json::Value::Null))
}

fn normalize_relay_input(input: &str) -> Result<String, String> {
    let input = input.trim();
    if input.is_empty() {
        return Err("Enter a Tailscale DNS name, IP address, or relay URL.".into());
    }
    if input.contains('#') {
        return Err("Enter the relay address without a room-invite fragment.".into());
    }
    let mut relay = if input.starts_with("ws://") || input.starts_with("wss://") {
        input.to_owned()
    } else {
        format!("ws://{input}")
    };
    let authority_start = relay.find("://").map_or(0, |index| index + 3);
    if !relay[authority_start..].contains('/') {
        relay.push_str("/ws");
    }
    Ok(relay)
}

fn version_corner() -> gtk::Box {
    let corner = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    corner.set_halign(gtk::Align::End);
    let update_link = gtk::LinkButton::with_label(RELEASES_URL, "");
    update_link.set_visible(false);
    update_link.set_valign(gtk::Align::Center);
    update_link.add_css_class("settings-update-link");
    let version = gtk::Label::new(Some(&format!("v{CURRENT_VERSION}")));
    version.add_css_class("settings-status");
    corner.append(&update_link);
    corner.append(&version);

    let update_link = update_link.clone();
    glib::spawn_future_local(async move {
        let Ok(Some(tag)) = gio::spawn_blocking(fetch_latest_release_tag).await else {
            return;
        };
        if is_newer_version(&tag, CURRENT_VERSION) {
            update_link.set_label(&format!("{tag} available"));
            update_link.set_visible(true);
        }
    });
    corner
}

fn fetch_latest_release_tag() -> Option<String> {
    let output = ProcessCommand::new("curl")
        .args([
            "-fsSL",
            "--max-time",
            "5",
            "-o",
            "/dev/null",
            "-w",
            "%{url_effective}",
            RELEASES_URL,
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let final_url = String::from_utf8(output.stdout).ok()?;
    let tag = final_url.trim().rsplit_once("/tag/")?.1;
    (!tag.is_empty()).then(|| tag.to_string())
}

fn is_newer_version(remote_tag: &str, current: &str) -> bool {
    let remote = parse_version(remote_tag.trim_start_matches('v'));
    let current = parse_version(current);
    matches!((remote, current), (Some(remote), Some(current)) if remote > current)
}

fn parse_version(value: &str) -> Option<(u64, u64, u64)> {
    let mut parts = value.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;
    parts.next().is_none().then_some((major, minor, patch))
}

fn postpone_controls(rule: &hyprbreathe_core::PostponeRule) -> (PostponeWidgets, Vec<gtk::Box>) {
    let enabled = gtk::Switch::builder()
        .active(rule.enabled)
        .valign(gtk::Align::Center)
        .build();
    let duration = duration_entry(rule.duration);

    duration.set_sensitive(rule.enabled);
    let duration_for_enabled = duration.clone();
    enabled.connect_active_notify(move |toggle| {
        duration_for_enabled.set_sensitive(toggle.is_active());
    });

    let widgets = PostponeWidgets {
        enabled: enabled.clone(),
        duration: duration.clone(),
    };
    let rows = vec![
        settings_row(
            "Allow postpone",
            "Allow this break to be delayed once per cycle.",
            &enabled,
        ),
        settings_row(
            "Postpone by",
            "Delay before showing this break again.",
            &duration,
        ),
    ];
    (widgets, rows)
}

fn update_postpone_rule(
    rule: &mut hyprbreathe_core::PostponeRule,
    widgets: &PostponeWidgets,
    field_name: &str,
) -> Result<(), String> {
    rule.enabled = widgets.enabled.is_active();
    rule.duration = parse_duration(&widgets.duration, field_name)?;
    Ok(())
}

fn settings_page(groups: &[gtk::Box]) -> gtk::ScrolledWindow {
    let content = gtk::Box::new(gtk::Orientation::Vertical, 24);
    content.set_margin_top(20);
    content.set_margin_bottom(24);
    content.set_margin_start(28);
    content.set_margin_end(28);
    content.set_width_request(680);
    content.set_halign(gtk::Align::Center);
    for group in groups {
        content.append(group);
    }
    gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .child(&content)
        .build()
}

fn settings_group(title: &str, description: &str, rows: &[gtk::Box]) -> gtk::Box {
    let group = gtk::Box::new(gtk::Orientation::Vertical, 8);
    let heading = gtk::Label::new(Some(title));
    heading.set_halign(gtk::Align::Start);
    heading.add_css_class("settings-section-title");
    let detail = gtk::Label::new(Some(description));
    detail.set_halign(gtk::Align::Start);
    detail.set_wrap(true);
    detail.add_css_class("settings-description");
    let list = gtk::Box::new(gtk::Orientation::Vertical, 0);
    list.add_css_class("settings-list");
    for (index, row) in rows.iter().enumerate() {
        if index > 0 {
            list.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        }
        list.append(row);
    }
    group.append(&heading);
    group.append(&detail);
    group.append(&list);
    group
}

fn settings_row(title: &str, description: &str, control: &impl IsA<gtk::Widget>) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 20);
    row.add_css_class("settings-row");
    let text = gtk::Box::new(gtk::Orientation::Vertical, 2);
    text.set_hexpand(true);
    let title = gtk::Label::new(Some(title));
    title.set_halign(gtk::Align::Start);
    title.set_wrap(true);
    let description = gtk::Label::new(Some(description));
    description.set_halign(gtk::Align::Start);
    description.set_wrap(true);
    description.add_css_class("settings-description");
    text.append(&title);
    text.append(&description);
    control.set_halign(gtk::Align::End);
    control.set_valign(gtk::Align::Center);
    row.append(&text);
    row.append(control);
    row
}

fn duration_entry(value: DurationMs) -> gtk::Entry {
    let entry = gtk::Entry::builder()
        .text(value.to_string())
        .width_chars(10)
        .max_width_chars(16)
        .placeholder_text("10m")
        .build();
    entry.set_input_purpose(gtk::InputPurpose::FreeForm);
    entry
}

fn parse_duration(entry: &gtk::Entry, field_name: &str) -> Result<DurationMs, String> {
    entry
        .text()
        .trim()
        .parse()
        .map_err(|_| format!("{field_name} must be a duration such as 20s, 10m, or 1h 15m."))
}

fn save_and_reload(config: &AppConfig) -> Result<String, String> {
    hyprbreathe_config::save(config).map_err(|error| error.to_string())?;
    let executable = std::env::current_exe().map_err(|error| {
        format!("Configuration was saved, but hyprbreathe could not reload: {error}")
    })?;
    let output = ProcessCommand::new(executable)
        .arg("reload")
        .output()
        .map_err(|error| {
            format!("Configuration was saved, but hyprbreathe could not reload: {error}")
        })?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr);
        let detail = detail.trim();
        return Ok(if detail.is_empty() {
            "Saved. Start the hyprbreathe daemon to apply the changes.".into()
        } else {
            format!("Saved, but the daemon did not reload: {detail}")
        });
    }
    let message = String::from_utf8_lossy(&output.stdout);
    let message = message.trim();
    Ok(if message.is_empty() {
        "Saved and reloaded.".into()
    } else {
        format!("Saved. {message}")
    })
}

fn set_status(label: &gtk::Label, message: &str, error: bool) {
    label.set_text(message);
    if error {
        label.add_css_class("settings-error");
    } else {
        label.remove_css_class("settings-error");
    }
}

fn strict_mode_index(value: StrictMode) -> u32 {
    match value {
        StrictMode::Off => 0,
        StrictMode::Delay => 1,
        StrictMode::Entire => 2,
    }
}

fn strict_mode_from_index(value: u32) -> StrictMode {
    match value {
        0 => StrictMode::Off,
        2 => StrictMode::Entire,
        _ => StrictMode::Delay,
    }
}

fn completion_sound_index(value: CompletionSound) -> u32 {
    match value {
        CompletionSound::WarmRise => 0,
        CompletionSound::SoftBloom => 1,
        CompletionSound::DeepHalo => 2,
        CompletionSound::CleanChime => 3,
    }
}

fn completion_sound_from_index(value: u32) -> CompletionSound {
    match value {
        1 => CompletionSound::SoftBloom,
        2 => CompletionSound::DeepHalo,
        3 => CompletionSound::CleanChime,
        _ => CompletionSound::WarmRise,
    }
}

fn display_mode_index(value: DisplayMode) -> u32 {
    match value {
        DisplayMode::All => 0,
        DisplayMode::Focused => 1,
        DisplayMode::Cursor => 2,
        DisplayMode::Primary => 3,
        DisplayMode::Configured => 4,
        DisplayMode::DimAllContentOne => 5,
    }
}

fn display_mode_from_index(value: u32) -> DisplayMode {
    match value {
        0 => DisplayMode::All,
        1 => DisplayMode::Focused,
        2 => DisplayMode::Cursor,
        3 => DisplayMode::Primary,
        4 => DisplayMode::Configured,
        _ => DisplayMode::DimAllContentOne,
    }
}

fn content_selector_index(value: ContentSelector) -> u32 {
    match value {
        ContentSelector::Focused => 0,
        ContentSelector::Cursor => 1,
        ContentSelector::Primary => 2,
        ContentSelector::Configured => 3,
    }
}

fn content_selector_from_index(value: u32) -> ContentSelector {
    match value {
        1 => ContentSelector::Cursor,
        2 => ContentSelector::Primary,
        3 => ContentSelector::Configured,
        _ => ContentSelector::Focused,
    }
}

fn pointer_mode_index(value: PointerMode) -> u32 {
    match value {
        PointerMode::Controls => 0,
        PointerMode::Block => 1,
        PointerMode::None => 2,
    }
}

fn pointer_mode_from_index(value: u32) -> PointerMode {
    match value {
        0 => PointerMode::Controls,
        2 => PointerMode::None,
        _ => PointerMode::Block,
    }
}

fn install_css() {
    let provider = gtk::CssProvider::new();
    provider.load_from_string(
        r#"
        .settings-intro-title {
            font-size: 1.35em;
            font-weight: 700;
        }
        .settings-section-title {
            font-size: 1.08em;
            font-weight: 700;
        }
        .settings-description {
            opacity: 0.72;
        }
        .settings-list {
            background-color: alpha(@theme_fg_color, 0.045);
            border: 1px solid alpha(@theme_fg_color, 0.14);
            border-radius: 8px;
        }
        .settings-row {
            padding: 13px 16px;
            min-height: 42px;
        }
        .settings-status-bar {
            border-top: 1px solid alpha(@theme_fg_color, 0.12);
        }
        .settings-status {
            padding: 10px 16px;
            opacity: 0.78;
        }
        .settings-error {
            color: @error_color;
            opacity: 1;
        }
        .settings-update-link {
            padding: 0;
            min-height: 0;
        }
        .settings-secret {
            font-family: monospace;
        }
        "#,
    );
    if let Some(display) = gtk::gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tailscale_hostnames_become_relay_urls() {
        assert_eq!(
            normalize_relay_input("lambda-1.example.ts.net:8787").unwrap(),
            "ws://lambda-1.example.ts.net:8787/ws"
        );
        assert_eq!(
            normalize_relay_input("wss://breaks.example.net/custom").unwrap(),
            "wss://breaks.example.net/custom"
        );
        assert!(normalize_relay_input("ws://host/ws#hyprbreathe=secret").is_err());
    }

    #[test]
    fn version_comparison() {
        assert!(is_newer_version("v0.2.0", "0.1.5"));
        assert!(is_newer_version("0.1.6", "0.1.5"));
        assert!(is_newer_version("v1.0.0", "0.9.9"));
        assert!(!is_newer_version("v0.1.5", "0.1.5"));
        assert!(!is_newer_version("v0.1.4", "0.1.5"));
        assert!(!is_newer_version("not-a-tag", "0.1.5"));
        assert!(!is_newer_version("v0.2", "0.1.5"));
    }

    #[test]
    fn selector_indexes_round_trip() {
        for value in [
            DisplayMode::All,
            DisplayMode::Focused,
            DisplayMode::Cursor,
            DisplayMode::Primary,
            DisplayMode::Configured,
            DisplayMode::DimAllContentOne,
        ] {
            assert_eq!(display_mode_from_index(display_mode_index(value)), value);
        }
        for value in [StrictMode::Off, StrictMode::Delay, StrictMode::Entire] {
            assert_eq!(strict_mode_from_index(strict_mode_index(value)), value);
        }
        for value in [
            CompletionSound::WarmRise,
            CompletionSound::SoftBloom,
            CompletionSound::DeepHalo,
            CompletionSound::CleanChime,
        ] {
            assert_eq!(
                completion_sound_from_index(completion_sound_index(value)),
                value
            );
        }
    }
}
