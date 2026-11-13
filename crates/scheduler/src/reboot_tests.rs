use super::*;

const WALL_MS: u64 = 1_700_000_000_000;

fn clock(ms: u64) -> ClockSample {
    ClockSample {
        monotonic_ms: 100_000 + ms,
        boottime_ms: 200_000 + ms,
        wall_unix_ms: WALL_MS + ms,
    }
}

fn reboot_clock(ms: u64) -> ClockSample {
    ClockSample {
        monotonic_ms: 10,
        boottime_ms: 20,
        wall_unix_ms: WALL_MS + ms,
    }
}

fn scheduler() -> Scheduler {
    let mut config = hyprbreathe_config::defaults();
    config.enough.enabled = true;
    config.enough.limit = DurationMs(1_000);
    config.enough.poweroff_countdown = DurationMs(200);
    Scheduler::new(config, "old-boot".into(), clock(0), "test".into())
}

fn restore(timer: &Scheduler, now: ClockSample) -> Scheduler {
    let json = serde_json::to_string(&timer.snapshot()).unwrap();
    let snapshot = serde_json::from_str(&json).unwrap();
    Scheduler::restore(
        timer.config.clone(),
        "new-boot".into(),
        now,
        "test".into(),
        snapshot,
    )
}

fn counters(ms: u64) -> ScreenTime {
    ScreenTime {
        mini_ms: ms,
        long_ms: ms,
        rest_ms: ms,
    }
}

#[test]
fn reboot_recovers_all_timers_independently_of_startup_setting() {
    for recover_state in [false, true] {
        let mut timer = scheduler();
        timer.config.startup.recover_state = recover_state;
        timer.handle_event(SchedulerEvent::Tick, clock(800));
        let context = context_in_mut(&mut timer.state).unwrap();
        context.screen_time.long_ms = 1_400;
        context.screen_time.rest_ms = 2_000;
        let now = reboot_clock(1_100);
        let mut timer = restore(&timer, now);
        let status = timer.status(now);
        let expected = ScreenTime {
            mini_ms: 500,
            long_ms: 1_100,
            rest_ms: 1_700,
        };
        assert_eq!(status.screen_time, expected);
        assert_eq!(status.enough.used_ms, 800);
        assert_eq!(status.remaining_ms, Some(600_000 - 500));
        assert_eq!(timer.snapshot().boot_id, "new-boot");
        assert_eq!(timer.snapshot().last_clock, now);

        let later = ClockSample {
            monotonic_ms: now.monotonic_ms + 100,
            boottime_ms: now.boottime_ms + 100,
            wall_unix_ms: now.wall_unix_ms + 100,
        };
        timer.handle_event(SchedulerEvent::Tick, later);
        let mut expected = expected;
        expected.advance(100, false);
        assert_eq!(timer.status(later).screen_time, expected);
        assert_eq!(timer.status(later).enough.used_ms, 900);
    }
}

#[test]
fn reboot_idle_recovery_cannot_bank_time() {
    let mut timer = scheduler();
    timer.handle_event(SchedulerEvent::Tick, clock(800));
    let now = reboot_clock(5_000);
    let mut timer = restore(&timer, now);
    assert_eq!(timer.status(now).screen_time, counters(0));
    let later = ClockSample {
        monotonic_ms: now.monotonic_ms + 100,
        boottime_ms: now.boottime_ms + 100,
        wall_unix_ms: now.wall_unix_ms + 100,
    };
    timer.handle_event(SchedulerEvent::Tick, later);
    assert_eq!(timer.status(later).screen_time, counters(100));
    assert_eq!(timer.enough.used_ms, 900);
}

#[test]
fn backwards_wall_clock_does_not_add_or_erase_usage() {
    let mut timer = scheduler();
    timer.handle_event(SchedulerEvent::Tick, clock(800));
    let now = reboot_clock(500);
    let timer = restore(&timer, now);
    assert_eq!(timer.status(now).screen_time, counters(800));
    assert_eq!(timer.enough.used_ms, 800);
}

#[test]
fn reboot_drops_old_boot_flags_and_postponement_deadlines() {
    let mut timer = scheduler();
    timer.handle_event(SchedulerEvent::Tick, clock(800));
    let context = context_in_mut(&mut timer.state).unwrap();
    context.pending = Some(PendingBreak {
        id: DueBreakId::new(),
        kind: BreakKind::Mini,
        postpone_count: 1,
        manual: false,
        not_before_mono_ms: 9_000_000,
        mirrored: None,
    });
    timer.handle_event(SchedulerEvent::LockStarted, clock(800));
    let now = reboot_clock(900);
    let timer = restore(&timer, now);
    assert!(matches!(timer.state(), SchedulerState::Running { .. }));
    assert_eq!(timer.screen_state, ScreenState::default());
    assert!(timer.context().unwrap().pending.is_none());
    assert_eq!(timer.status(now).screen_time, counters(700));
    assert_eq!(timer.status(now).remaining_ms, Some(600_000 - 700));
}

#[test]
fn enough_reappears_and_needs_a_new_click_in_every_phase() {
    for phase in ["prompt", "countdown", "poweroff", "failed"] {
        let mut timer = scheduler();
        timer.handle_event(SchedulerEvent::Tick, clock(1_000));
        let old_id = timer.enough.session.as_ref().unwrap().id;
        let session_id = old_id;
        let command = Command::AcknowledgeEnough { session_id };
        if phase != "prompt" {
            timer.handle_command(&command, clock(1_000)).unwrap();
        }
        if matches!(phase, "poweroff" | "failed") {
            timer.handle_event(SchedulerEvent::Tick, clock(1_200));
        }
        if phase == "failed" {
            timer.handle_event(
                SchedulerEvent::PoweroffFailed {
                    session_id: old_id,
                    reason: "Permission denied".into(),
                },
                clock(1_200),
            );
        }
        let now = reboot_clock(2_000);
        let mut timer = restore(&timer, now);
        assert_eq!(timer.enough.used_ms, 1_000);
        let effects = timer.startup_effects(now);
        assert!(matches!(effects.as_slice(),
            [Effect::StartEnoughOverlay(spec)]
                if spec.deadline_boot_ms.is_none()
                    && spec.error.is_none()));
        assert_eq!(
            timer.handle_command(&command, now),
            Err(SchedulerError::NotAwaitingEnough),
        );

        let later = ClockSample {
            monotonic_ms: now.monotonic_ms + 5_000,
            boottime_ms: now.boottime_ms + 5_000,
            wall_unix_ms: now.wall_unix_ms + 5_000,
        };
        let effects = timer.handle_event(SchedulerEvent::Tick, later);
        assert!(effects.is_empty());
        let session_id = timer.enough.session.as_ref().unwrap().id;
        let command = Command::AcknowledgeEnough { session_id };
        timer.handle_command(&command, later).unwrap();
        let deadline = later.boottime_ms + 200;
        assert_eq!(
            timer.enough.session.as_ref().unwrap().deadline_boot_ms,
            Some(deadline),
        );
        let expired = ClockSample {
            monotonic_ms: later.monotonic_ms + 200,
            boottime_ms: deadline,
            wall_unix_ms: later.wall_unix_ms + 200,
        };
        assert_eq!(
            timer.handle_event(SchedulerEvent::Tick, expired),
            vec![Effect::RequestPoweroff { session_id }],
        );
    }
}

#[test]
fn reboot_after_local_midnight_clears_enough_and_its_countdown() {
    use chrono::{Local, TimeZone};

    let day = Local
        .timestamp_millis_opt(WALL_MS as i64)
        .single()
        .unwrap()
        .date_naive();
    let midnight = day
        .succ_opt()
        .unwrap()
        .and_hms_opt(0, 0, 0)
        .unwrap()
        .and_local_timezone(Local)
        .earliest()
        .unwrap();
    for acknowledged in [false, true] {
        let mut timer = scheduler();
        timer.handle_event(SchedulerEvent::Tick, clock(1_000));
        if acknowledged {
            let session = timer.enough.session.as_ref().unwrap();
            let session_id = session.id;
            let command = Command::AcknowledgeEnough { session_id };
            timer.handle_command(&command, clock(1_000)).unwrap();
        }
        let now = ClockSample {
            wall_unix_ms: midnight.timestamp_millis() as u64 + 1_000,
            ..reboot_clock(0)
        };
        let mut timer = restore(&timer, now);
        assert_eq!(timer.enough.used_ms, 0);
        assert!(timer.enough.session.is_none());
        assert!(timer.startup_effects(now).is_empty());
        let effects = timer.handle_event(SchedulerEvent::Tick, now);
        assert!(effects.is_empty());
    }
}

#[test]
fn reboot_does_not_restore_an_unsupported_snapshot() {
    let mut timer = scheduler();
    timer.handle_event(SchedulerEvent::Tick, clock(800));
    let mut snapshot = timer.snapshot();
    snapshot.schema_version += 1;
    let now = reboot_clock(900);
    let timer = Scheduler::restore(
        timer.config,
        "new-boot".into(),
        now,
        "test".into(),
        snapshot,
    );
    assert_eq!(timer.status(now).screen_time, counters(0));
    assert_eq!(timer.enough, EnoughTimer::default());
}
