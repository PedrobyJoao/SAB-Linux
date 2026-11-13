use chrono::{Local, NaiveDate};

use super::*;

const NOON: u64 = 43_200_000;
const DAY_MS: u64 = 86_400_000;

fn clock(day: u32, ms: u64) -> ClockSample {
    let date = NaiveDate::from_ymd_opt(2026, 11, 13 + day).unwrap();
    let midnight = date
        .and_hms_opt(0, 0, 0)
        .unwrap()
        .and_local_timezone(Local)
        .earliest()
        .unwrap();
    ClockSample {
        monotonic_ms: u64::from(day) * DAY_MS + ms,
        boottime_ms: u64::from(day) * DAY_MS + ms,
        wall_unix_ms: midnight.timestamp_millis() as u64 + ms,
    }
}

fn scheduler(enabled: bool) -> Scheduler {
    let mut config = hyprbreathe_config::defaults();
    config.daily_config_lock.enabled = enabled;
    start(config, clock(0, NOON))
}

fn start(config: AppConfig, now: ClockSample) -> Scheduler {
    Scheduler::new(config, "boot".into(), now, "test".into())
}

fn edited(config: &AppConfig) -> AppConfig {
    let mut config = config.clone();
    config.schedule.mini.interval = DurationMs(900_000);
    config.schedule.mini.duration = DurationMs(30_000);
    config.schedule.long.interval = DurationMs(3_600_000);
    config.schedule.long.duration = DurationMs(600_000);
    config.schedule.rest.interval = DurationMs(10_800_000);
    config.schedule.rest.duration = DurationMs(2_400_000);
    config.enough.enabled = true;
    config.enough.limit = DurationMs(21_600_000);
    config.enough.poweroff_countdown = DurationMs(60_000);
    config
}

fn assert_rules(timer: &Scheduler, expected: &AppConfig) {
    assert_eq!(timer.config.schedule, expected.schedule);
    assert_eq!(timer.config.enough, expected.enough);
    let lock = &timer.config.daily_config_lock;
    assert_eq!(lock, &expected.daily_config_lock);
}

fn load(
    timer: &Scheduler,
    config: AppConfig,
    // A different boot identity exercises reboot recovery.
    boot: &str,
    now: ClockSample,
) -> Scheduler {
    let json = serde_json::to_string(&timer.snapshot()).unwrap();
    let snapshot = serde_json::from_str(&json).unwrap();
    let socket = "test".into();
    Scheduler::restore(config, boot.into(), now, socket, snapshot)
}

#[test]
fn disabled_lock_and_initial_config_apply_immediately() {
    let mut timer = scheduler(false);
    let config = edited(&timer.config);
    timer.replace_config(config.clone(), clock(0, NOON));
    assert_rules(&timer, &config);
    assert!(!timer.has_pending_config());

    let mut config = config;
    config.daily_config_lock.enabled = true;
    let now = clock(0, NOON);
    let timer = start(config.clone(), now);
    assert_rules(&timer, &config);
    assert!(!timer.has_pending_config());
}

#[test]
fn only_schedule_and_enough_wait_for_local_midnight() {
    let mut timer = scheduler(true);
    let original = timer.config.clone();
    let mut config = edited(&original);
    config.strict.mode = StrictMode::Entire;
    config.postpone.mini.enabled = false;
    config.completion.manual_resume = true;
    config.notifications.enabled = false;
    config.content.show_message = false;
    config.recovery.wake_grace = DurationMs(7_000);
    timer.replace_config(config.clone(), clock(0, NOON));
    assert_rules(&timer, &original);
    assert_eq!(timer.config.strict, config.strict);
    assert_eq!(timer.config.postpone, config.postpone);
    assert_eq!(timer.config.completion, config.completion);
    assert_eq!(timer.config.notifications, config.notifications);
    assert_eq!(timer.config.content, config.content);
    assert_eq!(timer.config.recovery, config.recovery);
    assert!(timer.has_pending_config());

    timer.handle_event(SchedulerEvent::Tick, clock(0, DAY_MS - 1));
    assert_rules(&timer, &original);
    timer.handle_event(SchedulerEvent::Tick, clock(1, 0));
    assert_rules(&timer, &config);
    assert!(!timer.has_pending_config());
}

#[test]
fn latest_preferences_win_and_reverting_cancels_pending_changes() {
    let mut timer = scheduler(true);
    let original = timer.config.clone();
    let mut config = edited(&original);
    timer.replace_config(config.clone(), clock(0, NOON));
    timer.replace_config(original.clone(), clock(0, NOON + 1));
    assert!(!timer.has_pending_config());
    config.schedule.mini.interval = DurationMs(1_200_000);
    timer.replace_config(config.clone(), clock(0, NOON + 2));
    // Unrelated saves must keep saved, not effective, preferences.
    config.tray.enabled = false;
    timer.replace_config(config.clone(), clock(0, NOON + 3));
    assert_rules(&timer, &original);
    assert!(!timer.config.tray.enabled);
    timer.handle_event(SchedulerEvent::Tick, clock(1, 0));
    assert_rules(&timer, &config);
    assert!(!timer.config.tray.enabled);
}

#[test]
fn stricter_schedule_changes_are_also_deferred() {
    let mut timer = scheduler(true);
    let original = timer.config.clone();
    let mut config = original.clone();
    config.schedule.mini.interval = DurationMs(300_000);
    config.schedule.mini.duration = DurationMs(60_000);
    timer.replace_config(config.clone(), clock(0, NOON));
    assert_rules(&timer, &original);
    timer.handle_event(SchedulerEvent::Tick, clock(1, 0));
    assert_rules(&timer, &config);
}

#[test]
fn enabling_freezes_current_rules_and_disabling_waits() {
    let mut timer = scheduler(false);
    let mut original = timer.config.clone();
    let mut config = edited(&original);
    config.daily_config_lock.enabled = true;
    timer.replace_config(config.clone(), clock(0, NOON));
    original.daily_config_lock.enabled = true;
    assert_rules(&timer, &original);

    config.daily_config_lock.enabled = false;
    timer.replace_config(config.clone(), clock(0, NOON + 1));
    assert_rules(&timer, &original);
    assert!(timer.has_pending_config());
    timer.handle_event(SchedulerEvent::Tick, clock(1, 0));
    assert_rules(&timer, &config);
    assert!(!timer.has_pending_config());

    config.schedule.mini.interval = DurationMs(1_200_000);
    timer.replace_config(config.clone(), clock(1, 1));
    assert_rules(&timer, &config);
    assert!(!timer.has_pending_config());
}

#[test]
fn restarts_and_reboots_preserve_the_lock_without_break_recovery() {
    for recover in [false, true] {
        for boot in ["boot", "new-boot"] {
            let timer = scheduler(true);
            let original = timer.config.clone();
            // Startup must not bypass the lock with new edits.
            let mut config = edited(&original);
            config.daily_config_lock.enabled = false;
            config.startup.recover_state = recover;
            let now = clock(0, NOON + 1);
            let mut timer = load(&timer, config.clone(), boot, now);
            assert_rules(&timer, &original);
            assert!(timer.has_pending_config());
            timer = load(&timer, config.clone(), boot, clock(1, 1));
            assert_rules(&timer, &config);
            assert!(!timer.has_pending_config());
        }
    }
}

#[test]
fn accepted_pending_changes_survive_multiple_restarts() {
    let mut timer = scheduler(true);
    let original = timer.config.clone();
    let config = edited(&original);
    timer.replace_config(config.clone(), clock(0, NOON));
    let timer = load(&timer, config.clone(), "new", clock(0, NOON));
    assert_rules(&timer, &original);
    assert!(timer.has_pending_config());
    let timer = load(&timer, config.clone(), "new", clock(2, 0));
    assert_rules(&timer, &config);
    assert!(!timer.has_pending_config());
}

#[test]
fn old_snapshots_start_with_the_initial_configuration() {
    let timer = scheduler(true);
    let config = edited(&timer.config);
    let mut value = serde_json::to_value(timer.snapshot()).unwrap();
    value.as_object_mut().unwrap().remove("daily_config_lock");
    let snapshot = serde_json::from_value(value).unwrap();
    let timer = Scheduler::restore(
        config.clone(),
        "boot".into(),
        clock(0, NOON),
        "test".into(),
        snapshot,
    );
    assert_rules(&timer, &config);
    assert!(!timer.has_pending_config());
}

#[test]
fn midnight_does_not_restart_an_active_break() {
    let mut timer = scheduler(true);
    let now = clock(0, DAY_MS - 1_000);
    timer.handle_command(&Command::Mini, now).unwrap();
    let active = timer.active_break().unwrap().clone();
    let config = edited(&timer.config);
    timer.replace_config(config.clone(), now);
    let now = clock(1, 0);
    let effects = timer.handle_event(SchedulerEvent::Tick, now);
    assert_rules(&timer, &config);
    let current = timer.active_break().unwrap();
    assert_eq!(current.session_id, active.session_id);
    assert_eq!(current.ends_boot_ms, active.ends_boot_ms);
    assert!(effects.is_empty());
}

#[test]
fn backwards_date_does_not_activate_pending_rules() {
    let mut timer = scheduler(true);
    timer.handle_event(SchedulerEvent::Tick, clock(1, NOON));
    let original = timer.config.clone();
    let config = edited(&original);
    timer.replace_config(config.clone(), clock(1, NOON));
    timer.handle_event(SchedulerEvent::Tick, clock(0, NOON));
    assert_rules(&timer, &original);
    assert!(timer.has_pending_config());
    timer.handle_event(SchedulerEvent::Tick, clock(2, 0));
    assert_rules(&timer, &config);
}

#[test]
fn enough_stays_enforced_until_midnight_and_then_resets() {
    let mut config = hyprbreathe_config::defaults();
    config.daily_config_lock.enabled = true;
    config.enough.enabled = true;
    config.enough.limit = DurationMs(1_000);
    let now = clock(0, NOON);
    let mut timer = start(config.clone(), now);
    timer.handle_event(SchedulerEvent::Tick, clock(0, NOON + 1_000));
    let session_id = timer.enough.session.as_ref().unwrap().id;

    let mut requested = config.clone();
    requested.enough.enabled = false;
    requested.daily_config_lock.enabled = false;
    timer.replace_config(requested.clone(), clock(0, NOON + 1_000));
    assert_rules(&timer, &config);
    let command = Command::AcknowledgeEnough { session_id };
    timer
        .handle_command(&command, clock(0, NOON + 1_001))
        .unwrap();
    assert!(timer.enough.session.is_some());

    let now = clock(1, 0);
    let effects = timer.handle_event(SchedulerEvent::Tick, now);
    assert_rules(&timer, &requested);
    assert!(timer.enough.session.is_none());
    assert_eq!(timer.enough.used_ms, 0);
    assert!(effects.contains(&Effect::StopOverlay { session_id }));
    assert!(
        !effects
            .iter()
            .any(|e| { matches!(e, Effect::RequestPoweroff { .. }) })
    );
}

#[test]
fn reload_after_midnight_queues_new_edits_for_the_following_day() {
    let mut timer = scheduler(true);
    let first = edited(&timer.config);
    timer.replace_config(first.clone(), clock(0, NOON));
    let mut second = first.clone();
    second.schedule.mini.duration = DurationMs(45_000);
    second.enough.limit = DurationMs(18_000_000);
    second.tray.enabled = false;

    timer.replace_config(second.clone(), clock(1, 0));
    assert_rules(&timer, &first);
    assert!(!timer.config.tray.enabled);
    assert!(timer.has_pending_config());
    timer.handle_event(SchedulerEvent::Tick, clock(2, 0));
    assert_rules(&timer, &second);
    assert!(!timer.has_pending_config());
}

#[test]
fn first_command_after_midnight_uses_the_new_break_duration() {
    let mut timer = scheduler(true);
    let mut config = timer.config.clone();
    config.schedule.mini.duration = DurationMs(45_000);
    timer.replace_config(config.clone(), clock(0, NOON));

    let now = clock(1, 0);
    let effects = timer.handle_command(&Command::Mini, now).unwrap();
    let active = timer.active_break().unwrap();
    assert_rules(&timer, &config);
    assert_eq!(active.ends_boot_ms, now.boottime_ms + 45_000);
    assert!(!timer.has_pending_config());
    let [Effect::StartOverlay(spec)] = effects.as_slice() else {
        panic!("expected the manual break overlay");
    };
    assert_eq!(spec.duration, config.schedule.mini.duration);
}
