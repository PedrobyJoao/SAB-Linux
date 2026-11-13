use super::*;

fn clock(ms: u64) -> ClockSample {
    clock_after_sleep(ms, 0)
}

fn clock_after_sleep(ms: u64, sleep_ms: u64) -> ClockSample {
    ClockSample {
        monotonic_ms: ms,
        boottime_ms: ms + sleep_ms,
        wall_unix_ms: 1_700_000_000_000 + ms + sleep_ms,
    }
}

fn scheduler() -> Scheduler {
    scheduler_at(clock(0))
}

fn scheduler_at(now: ClockSample) -> Scheduler {
    let mut config = hyprbreathe_config::defaults();
    config.enough.enabled = true;
    config.enough.limit = DurationMs(1_000);
    config.enough.poweroff_countdown = DurationMs(200);
    Scheduler::new(config, "boot".into(), now, "test".into())
}

fn acknowledge(scheduler: &mut Scheduler, ms: u64) -> Vec<Effect> {
    let session_id = scheduler.enough.session.as_ref().unwrap().id;
    scheduler
        .handle_command(&Command::AcknowledgeEnough { session_id }, clock(ms))
        .unwrap()
}

fn restore(scheduler: &Scheduler, ms: u64, boot: &str) -> Scheduler {
    restore_at(scheduler, clock(ms), boot)
}

fn restore_at(timer: &Scheduler, now: ClockSample, boot: &str) -> Scheduler {
    // Exercise the actual persistence representation, not just cloning.
    let json = serde_json::to_string(&timer.snapshot()).unwrap();
    let snapshot = serde_json::from_str(&json).unwrap();
    Scheduler::restore(
        timer.config.clone(),
        boot.into(),
        now,
        "test".into(),
        snapshot,
    )
}

#[test]
fn disabled_enough_does_not_accumulate_or_trigger() {
    let mut scheduler = scheduler();
    scheduler.config.enough.enabled = false;
    scheduler.handle_event(SchedulerEvent::Tick, clock(1_000));
    assert_eq!(scheduler.enough.used_ms, 0);
    assert!(scheduler.enough.session.is_none());
}

#[test]
fn idle_pauses_daily_usage_without_recovery_or_double_counting() {
    let mut scheduler = scheduler();
    scheduler.handle_event(SchedulerEvent::DisplaysOff(true), clock(400));
    scheduler.handle_event(SchedulerEvent::LockStarted, clock(500));
    scheduler.handle_event(SchedulerEvent::SuspendStarted, clock(600));
    let after_sleep = |ms| clock_after_sleep(ms, 10_000);
    scheduler.handle_event(SchedulerEvent::SuspendEnded, after_sleep(700));
    scheduler.handle_event(SchedulerEvent::LockEnded, after_sleep(800));
    scheduler.handle_event(SchedulerEvent::Tick, after_sleep(1_200));
    assert_eq!(scheduler.enough.used_ms, 400);
    scheduler.handle_event(SchedulerEvent::DisplaysOff(false), after_sleep(1_400));
    scheduler.handle_event(SchedulerEvent::Tick, after_sleep(2_000));
    assert_eq!(scheduler.enough.used_ms, 1_000);
    assert!(scheduler.enough.session.is_some());
}

#[test]
fn enough_supersedes_a_due_break_and_waits_for_acknowledgment() {
    let mut scheduler = scheduler();
    scheduler.config.schedule.mini.interval = DurationMs(1_000);
    let effects = scheduler.handle_event(SchedulerEvent::Tick, clock(1_000));
    assert!(matches!(effects.as_slice(),
        [Effect::StartEnoughOverlay(spec)]
            if spec.deadline_boot_ms.is_none()));
    assert_eq!(scheduler.status(clock(1_000)).state, "enough");
    assert!(scheduler.shortcut_inhibition_enabled());
    assert!(
        scheduler
            .handle_event(SchedulerEvent::Tick, clock(5_000),)
            .is_empty()
    );
    for command in [Command::Mini, Command::Postpone, Command::ResumeBreak] {
        assert_eq!(
            scheduler.handle_command(&command, clock(5_000)),
            Err(SchedulerError::EnoughActive)
        );
    }
    let stale = Command::AcknowledgeEnough {
        session_id: BreakSessionId::new(),
    };
    assert_eq!(
        scheduler.handle_command(&stale, clock(5_000)),
        Err(SchedulerError::NotAwaitingEnough)
    );
    let effects = acknowledge(&mut scheduler, 5_000);
    assert!(matches!(effects.as_slice(),
        [Effect::StartEnoughOverlay(spec)]
            if spec.deadline_boot_ms == Some(5_200)));
    assert!(!scheduler.shortcut_inhibition_enabled());
    let session_id = scheduler.enough.session.as_ref().unwrap().id;
    assert_eq!(
        scheduler.handle_command(&Command::AcknowledgeEnough { session_id }, clock(5_100),),
        Err(SchedulerError::NotAwaitingEnough)
    );
    assert_eq!(scheduler.status(clock(5_100)).remaining_ms, Some(100));
}

#[test]
fn reminder_breaks_do_not_count_towards_enough() {
    let mut scheduler = scheduler();
    scheduler.config.schedule.mini.duration = DurationMs(100);
    scheduler
        .handle_command(&Command::Mini, clock(400))
        .unwrap();
    scheduler.handle_event(SchedulerEvent::Tick, clock(500));
    assert_eq!(scheduler.enough.used_ms, 400);
    scheduler.handle_event(SchedulerEvent::Tick, clock(1_100));
    assert_eq!(scheduler.enough.used_ms, 1_000);
    assert!(scheduler.enough.session.is_some());
}

#[test]
fn triggered_session_latches_configuration_across_reload() {
    let mut scheduler = scheduler();
    scheduler.handle_event(SchedulerEvent::Tick, clock(1_000));
    let mut updated = scheduler.config.clone();
    updated.enough.enabled = false;
    updated.enough.limit = DurationMs(90_000);
    updated.enough.poweroff_countdown = DurationMs(60_000);
    scheduler.replace_config(updated);
    acknowledge(&mut scheduler, 1_100);
    assert_eq!(
        scheduler.enough.session.as_ref().unwrap().deadline_boot_ms,
        Some(1_300)
    );
    assert!(matches!(
        scheduler
            .handle_event(SchedulerEvent::Tick, clock(1_300),)
            .as_slice(),
        [Effect::RequestPoweroff { .. }]
    ));
}

#[test]
fn daemon_restart_preserves_usage_prompt_and_started_deadline() {
    let mut scheduler = scheduler();
    scheduler.handle_event(SchedulerEvent::Tick, clock(400));
    let mut scheduler = restore(&scheduler, 600, "boot");
    assert_eq!(scheduler.enough.used_ms, 600);
    scheduler.handle_event(SchedulerEvent::Tick, clock(1_000));
    scheduler.config.startup.recover_state = false;
    let mut scheduler = restore(&scheduler, 1_100, "boot");
    assert!(matches!(scheduler.startup_effects(clock(1_100)).as_slice(),
        [Effect::StartEnoughOverlay(spec)]
            if spec.deadline_boot_ms.is_none()));
    acknowledge(&mut scheduler, 1_100);
    let mut scheduler = restore(&scheduler, 1_250, "boot");
    assert_eq!(
        scheduler.enough.session.as_ref().unwrap().deadline_boot_ms,
        Some(1_300)
    );
    assert!(matches!(
        scheduler
            .handle_event(SchedulerEvent::Tick, clock(1_300),)
            .as_slice(),
        [Effect::RequestPoweroff { .. }]
    ));
    assert!(
        scheduler
            .handle_event(SchedulerEvent::Tick, clock(1_400),)
            .is_empty()
    );
    let mut scheduler = restore(&scheduler, 1_500, "boot");
    assert!(matches!(
        scheduler
            .handle_event(SchedulerEvent::Tick, clock(1_500),)
            .as_slice(),
        [Effect::RequestPoweroff { .. }]
    ));
}

#[test]
fn enough_accounting_is_independent_of_break_recovery() {
    let mut scheduler = scheduler();
    scheduler.handle_event(SchedulerEvent::Tick, clock(400));
    scheduler.config.startup.recover_state = false;
    assert_eq!(restore(&scheduler, 600, "boot").enough.used_ms, 600);

    scheduler.config.startup.recover_state = true;
    scheduler.config.recovery.missed_break = MissedBreakPolicy::Reset;
    scheduler.config.schedule.mini.interval = DurationMs(500);
    scheduler.handle_event(SchedulerEvent::Tick, clock(400));
    let restored = restore(&scheduler, 700, "boot");
    assert_eq!(restored.enough.used_ms, 700);
    assert_eq!(
        restored.status(clock(700)).screen_time,
        ScreenTime::default()
    );
}

#[test]
fn expired_poweroff_is_requested_after_resume_and_failure_is_visible() {
    let mut scheduler = scheduler();
    scheduler.handle_event(SchedulerEvent::Tick, clock(1_000));
    acknowledge(&mut scheduler, 1_000);
    scheduler.handle_event(SchedulerEvent::SuspendStarted, clock(1_100));
    assert!(
        scheduler
            .handle_event(SchedulerEvent::Tick, clock_after_sleep(1_100, 500),)
            .is_empty()
    );
    let resumed = clock_after_sleep(1_150, 8_900);
    let deadline = scheduler
        .enough
        .session
        .as_ref()
        .unwrap()
        .deadline_boot_ms
        .unwrap();
    assert!(resumed.monotonic_ms < deadline);
    assert!(resumed.boottime_ms > deadline);
    scheduler.handle_event(SchedulerEvent::SuspendEnded, resumed);
    let effects = scheduler.handle_event(SchedulerEvent::Tick, resumed);
    assert!(matches!(
        effects.as_slice(),
        [Effect::RequestPoweroff { .. }]
    ));
    let session_id = scheduler.enough.session.as_ref().unwrap().id;
    let effects = scheduler.handle_event(
        SchedulerEvent::PoweroffFailed {
            session_id,
            reason: "Permission denied".into(),
        },
        resumed,
    );
    assert!(matches!(effects.as_slice(),
        [Effect::StartEnoughOverlay(spec)]
            if spec.error.as_deref() == Some("Permission denied")));
    assert_eq!(scheduler.status(resumed).state, "poweroff-failed");
    let mut scheduler = restore_at(&scheduler, clock_after_sleep(1_151, 8_900), "boot");
    assert!(
        scheduler
            .handle_event(SchedulerEvent::Tick, clock_after_sleep(1_152, 8_900),)
            .is_empty()
    );
}

fn next_midnight_elapsed() -> u64 {
    use chrono::{Local, TimeZone};
    let today = Local
        .timestamp_millis_opt(clock(0).wall_unix_ms as i64)
        .single()
        .unwrap()
        .date_naive();
    let midnight = today
        .succ_opt()
        .unwrap()
        .and_hms_opt(0, 0, 0)
        .unwrap()
        .and_local_timezone(Local)
        .earliest()
        .unwrap();
    midnight.timestamp_millis() as u64 - clock(0).wall_unix_ms
}

#[test]
fn local_midnight_clears_usage_and_either_active_phase() {
    let elapsed = next_midnight_elapsed();
    for acknowledged in [false, true] {
        let mut scheduler = scheduler();
        scheduler.handle_event(SchedulerEvent::Tick, clock(1_000));
        if acknowledged {
            acknowledge(&mut scheduler, 1_000);
        }
        let session_id = scheduler.enough.session.as_ref().unwrap().id;
        let effects = scheduler.handle_event(SchedulerEvent::Tick, clock(elapsed));
        assert!(effects.contains(&Effect::StopOverlay { session_id }));
        assert!(
            !effects
                .iter()
                .any(|effect| matches!(effect, Effect::RequestPoweroff { .. }))
        );
        assert!(scheduler.enough.session.is_none());
        assert_eq!(scheduler.enough.used_ms, 0);
    }
}

#[test]
fn overnight_resume_clears_expired_poweroff_and_starts_new_allowance() {
    let start = next_midnight_elapsed() - 2_000;
    let mut scheduler = scheduler_at(clock(start));
    scheduler.handle_event(SchedulerEvent::Tick, clock(start + 1_000));
    acknowledge(&mut scheduler, start + 1_000);
    let session_id = scheduler.enough.session.as_ref().unwrap().id;
    scheduler.handle_event(SchedulerEvent::SuspendStarted, clock(start + 1_100));

    // Sleep crosses both the power-off deadline and local midnight.
    // Monotonic time stops; boottime and wall time keep advancing.
    let resumed = clock_after_sleep(start + 1_100, 3_000);
    let effects = scheduler.handle_event(SchedulerEvent::SuspendEnded, resumed);
    assert!(effects.contains(&Effect::StopOverlay { session_id }));
    assert!(
        !effects
            .iter()
            .any(|effect| { matches!(effect, Effect::RequestPoweroff { .. }) })
    );
    assert!(scheduler.enough.session.is_none());
    assert_eq!(scheduler.enough.used_ms, 0);

    let effects = scheduler.handle_event(
        SchedulerEvent::Tick,
        clock_after_sleep(start + 1_200, 3_000),
    );
    assert!(
        !effects
            .iter()
            .any(|effect| { matches!(effect, Effect::RequestPoweroff { .. }) })
    );
    assert_eq!(scheduler.enough.used_ms, 100);
    assert!(scheduler.enough.session.is_none());
}

#[test]
fn old_snapshots_without_enough_are_accepted() {
    let scheduler = scheduler();
    let mut value = serde_json::to_value(scheduler.snapshot()).unwrap();
    value.as_object_mut().unwrap().remove("enough");
    let snapshot: Snapshot = serde_json::from_value(value).unwrap();
    assert_eq!(snapshot.enough, EnoughTimer::default());
}
