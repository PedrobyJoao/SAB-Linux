use hyprbreathe_coop::{CoopPhase, CoopPolicy, CoopSnapshot, ScheduledBreak, SharedBreak};
use hyprbreathe_core::{
    AppConfig, BreakKind, BreakSessionId, ClockSample, Command, DueBreakId, DurationMs,
    MissedBreakPolicy, OverlaySpec, StrictMode,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

mod enough;
pub use enough::{EnoughSession, EnoughTimer};

#[cfg(test)]
mod enough_tests;

#[cfg(test)]
mod reboot_tests;

pub const STATE_SCHEMA_VERSION: u32 = 2;
const MAX_POSTPONEMENTS: u32 = 1;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScreenTime {
    pub mini_ms: u64,
    pub long_ms: u64,
    pub rest_ms: u64,
}

impl ScreenTime {
    fn advance(&mut self, elapsed: u64, recovering: bool) {
        for counter in [&mut self.mini_ms, &mut self.long_ms, &mut self.rest_ms] {
            *counter = if recovering {
                counter.saturating_sub(elapsed)
            } else {
                counter.saturating_add(elapsed)
            };
        }
    }

    fn next(&self, config: &AppConfig) -> (BreakKind, u64) {
        [
            (BreakKind::Rest, config.schedule.rest.interval, self.rest_ms),
            (BreakKind::Long, config.schedule.long.interval, self.long_ms),
            (BreakKind::Mini, config.schedule.mini.interval, self.mini_ms),
        ]
        .into_iter()
        .map(|(kind, interval, elapsed)| (kind, interval.as_millis().saturating_sub(elapsed)))
        .min_by_key(|(_, remaining)| *remaining)
        .expect("three break tiers")
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScreenState {
    pub displays_off: bool,
    pub locked: bool,
    pub sleeping: bool,
}

impl ScreenState {
    fn idle(self) -> bool {
        self.displays_off || self.locked || self.sleeping
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingBreak {
    pub id: DueBreakId,
    pub kind: BreakKind,
    pub postpone_count: u32,
    pub manual: bool,
    pub not_before_mono_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mirrored: Option<MirroredBreakPolicy>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MirroredBreakPolicy {
    pub duration_ms: u64,
    pub strict_duration_ms: u64,
    pub strict_entire: bool,
    pub manual_resume: bool,
    pub can_postpone: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduleContext {
    pub screen_time: ScreenTime,
    pub next_due_mono_ms: u64,
    pub minis_since_long: u32,
    #[serde(default)]
    pub longs_since_rest: u32,
    pub pending: Option<PendingBreak>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActiveBreak {
    pub context: ScheduleContext,
    pub due: PendingBreak,
    pub session_id: BreakSessionId,
    pub started_boot_ms: u64,
    pub ends_boot_ms: u64,
    pub strict_until_boot_ms: u64,
    #[serde(default)]
    pub manual_resume: bool,
    #[serde(default)]
    pub completion_sound_emitted: bool,
    #[serde(default)]
    pub resume_requested: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum SchedulerState {
    Running {
        context: ScheduleContext,
    },
    PreMiniBreak {
        context: ScheduleContext,
    },
    MiniBreak {
        active: ActiveBreak,
    },
    PreLongBreak {
        context: ScheduleContext,
    },
    LongBreak {
        active: ActiveBreak,
    },
    PreRestBreak {
        context: ScheduleContext,
    },
    RestBreak {
        active: ActiveBreak,
    },
    PausedUntil {
        inner: Box<SchedulerState>,
        paused_at: ClockSample,
        resume_mono_ms: u64,
    },
    Suspended {
        inner: Box<SchedulerState>,
    },
    Recovering {
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    pub schema_version: u32,
    pub boot_id: String,
    pub state: SchedulerState,
    pub last_clock: ClockSample,
    pub screen_state: ScreenState,
    #[serde(default)]
    pub enough: EnoughTimer,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchedulerEvent {
    Tick,
    PoweroffFailed {
        session_id: BreakSessionId,
        reason: String,
    },
    SuspendStarted,
    SuspendEnded,
    LockStarted,
    LockEnded,
    DisplaysOff(bool),
    DesktopState {
        displays_off: bool,
        locked: bool,
    },
    OverlayFailed {
        session_id: BreakSessionId,
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    StartEnoughOverlay(hyprbreathe_core::EnoughOverlaySpec),
    RequestPoweroff {
        session_id: BreakSessionId,
    },
    Notify {
        summary: String,
        body: String,
    },
    PlayCompletionSound,
    StartOverlay(OverlaySpec),
    StopOverlay {
        session_id: BreakSessionId,
    },
    OverlayDegraded {
        session_id: BreakSessionId,
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SchedulerStatus {
    pub state: String,
    pub paused: bool,
    pub break_kind: Option<BreakKind>,
    pub remaining_ms: Option<u64>,
    pub awaiting_resume: bool,
    pub minis_since_long: u32,
    pub longs_since_rest: u32,
    pub active_session: Option<BreakSessionId>,
    pub postpone_count: u32,
    pub can_postpone: bool,
    pub screen_time: ScreenTime,
    pub idle: bool,
    pub enough: EnoughTimer,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum SchedulerError {
    #[error("Enough is active; break actions are unavailable")]
    EnoughActive,
    #[error("this Enough session is not waiting for acknowledgment")]
    NotAwaitingEnough,
    #[error("no break is active")]
    NoActiveBreak,
    #[error("a break is already active")]
    BreakAlreadyActive,
    #[error("strict mode does not allow dismissal yet")]
    StrictMode,
    #[error("the postpone limit has been reached")]
    PostponeLimit,
    #[error("postponement is disabled for this break")]
    PostponeDisabled,
    #[error("the break has not finished or manual resume is disabled")]
    NotAwaitingResume,
    #[error("the completed break is waiting for manual resume")]
    AwaitingResume,
    #[error("command is handled by the daemon rather than the scheduler")]
    DaemonCommand,
}

#[derive(Debug, Clone)]
pub struct Scheduler {
    config: AppConfig,
    boot_id: String,
    socket_path: String,
    state: SchedulerState,
    last_clock: ClockSample,
    coop_policy: Option<CoopPolicy>,
    last_notified_due: Option<DueBreakId>,
    screen_state: ScreenState,
    enough: EnoughTimer,
}

impl Scheduler {
    pub fn new(config: AppConfig, boot_id: String, now: ClockSample, socket_path: String) -> Self {
        Self {
            state: Self::fresh_running(&config, now),
            config,
            boot_id,
            socket_path,
            last_clock: now,
            coop_policy: None,
            last_notified_due: None,
            screen_state: ScreenState::default(),
            enough: EnoughTimer::default(),
        }
    }

    pub fn restore(
        config: AppConfig,
        boot_id: String,
        now: ClockSample,
        socket_path: String,
        mut snapshot: Snapshot,
    ) -> Self {
        let same_boot = snapshot.boot_id == boot_id;
        let valid = snapshot.schema_version == STATE_SCHEMA_VERSION;
        if !same_boot && valid {
            let fresh = Self::new(config, boot_id, now, socket_path);
            return fresh.restore_reboot(snapshot);
        }
        if same_boot {
            // Account from the saved baseline before break recovery can
            // replace it. No overlay child exists during restoration.
            let working =
                !snapshot.screen_state.idle() && active_break_in(&snapshot.state).is_none();
            let _ = snapshot
                .enough
                .advance(&config.enough, snapshot.last_clock, now, working);
        }
        let mut state = if !config.startup.recover_state {
            Self::fresh_running(&config, now)
        } else if snapshot.schema_version != STATE_SCHEMA_VERSION {
            SchedulerState::Recovering {
                reason: format!("unsupported state schema {}", snapshot.schema_version),
            }
        } else if snapshot.boot_id != boot_id {
            Self::fresh_running(&config, now)
        } else {
            snapshot.state
        };

        let mut fresh = !config.startup.recover_state
            || !same_boot
            || snapshot.schema_version != STATE_SCHEMA_VERSION;
        if let MissedBreakPolicy::Reset = config.recovery.missed_break
            && state_is_overdue(&state, now)
        {
            state = Self::fresh_running(&config, now);
            fresh = true;
        }

        let mut scheduler = Self {
            config,
            boot_id,
            socket_path,
            state,
            last_clock: if fresh { now } else { snapshot.last_clock },
            coop_policy: None,
            last_notified_due: None,
            screen_state: if same_boot {
                snapshot.screen_state
            } else {
                ScreenState::default()
            },
            enough: if same_boot {
                snapshot.enough
            } else {
                EnoughTimer::default()
            },
        };
        if matches!(scheduler.state, SchedulerState::Recovering { .. }) {
            scheduler.state = Self::fresh_running(&scheduler.config, now);
        }
        scheduler.advance_screen_time(now);
        scheduler
    }

    fn restore_reboot(mut self, snapshot: Snapshot) -> Self {
        let now = self.last_clock;
        // Only usage crosses boots, not old pauses or break overlays.
        // Neither monotonic clock can be compared across boot IDs.
        let saved_context = context_in(&snapshot.state).cloned();
        if let Some(mut context) = saved_context {
            let idle_ms = now
                .wall_unix_ms
                .saturating_sub(snapshot.last_clock.wall_unix_ms);
            context.screen_time.advance(idle_ms, true);
            context.pending = None;
            refresh_schedule(&mut context, &self.config, now);
            self.state = SchedulerState::Running { context };
        }
        self.enough = snapshot.enough;
        self.enough.reboot(&self.config.enough, now);
        self
    }

    pub fn state(&self) -> &SchedulerState {
        &self.state
    }

    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            schema_version: STATE_SCHEMA_VERSION,
            boot_id: self.boot_id.clone(),
            state: self.state.clone(),
            last_clock: self.last_clock,
            screen_state: self.screen_state,
            enough: self.enough.clone(),
        }
    }

    pub fn replace_config(&mut self, config: AppConfig) {
        self.config = config;
    }

    pub fn startup_effects(&self, now: ClockSample) -> Vec<Effect> {
        if let Some(spec) = self.enough.overlay() {
            return vec![Effect::StartEnoughOverlay(spec)];
        }
        if matches!(
            self.state,
            SchedulerState::Suspended { .. } | SchedulerState::PausedUntil { .. }
        ) {
            return Vec::new();
        }
        self.active_break()
            .filter(|active| active.manual_resume || active.ends_boot_ms > now.boottime_ms)
            .map(|active| vec![Effect::StartOverlay(self.overlay_spec(active, now))])
            .unwrap_or_default()
    }

    pub fn handle_event(&mut self, event: SchedulerEvent, now: ClockSample) -> Vec<Effect> {
        let mut effects = self.advance_enough(now);
        if self.enough.session.is_some() {
            self.last_clock = now;
            match event {
                SchedulerEvent::SuspendStarted => {
                    self.screen_state.sleeping = true;
                }
                SchedulerEvent::SuspendEnded => {
                    self.screen_state.sleeping = false;
                }
                SchedulerEvent::LockStarted => {
                    self.screen_state.locked = true;
                }
                SchedulerEvent::LockEnded => {
                    self.screen_state.locked = false;
                }
                SchedulerEvent::DisplaysOff(off) => {
                    self.screen_state.displays_off = off;
                }
                SchedulerEvent::DesktopState {
                    displays_off,
                    locked,
                } => {
                    self.screen_state.displays_off = displays_off;
                    self.screen_state.locked = locked;
                }
                SchedulerEvent::Tick if !self.screen_state.sleeping => {
                    effects.extend(self.enough.request_poweroff(now));
                }
                SchedulerEvent::PoweroffFailed { session_id, reason } => {
                    effects.extend(
                        self.enough
                            .poweroff_failed(session_id, reason)
                            .map(Effect::StartEnoughOverlay),
                    );
                }
                SchedulerEvent::OverlayFailed { session_id, .. }
                    if self
                        .enough
                        .session
                        .as_ref()
                        .is_some_and(|session| session.id == session_id) =>
                {
                    effects.extend(self.enough.overlay().map(Effect::StartEnoughOverlay));
                }
                _ => {}
            }
            return effects;
        }
        if !self.screen_state.idle() && self.enough.trigger(&self.config.enough) {
            if let Some(active) = self.active_break() {
                effects.push(Effect::StopOverlay {
                    session_id: active.session_id,
                });
            }
            self.state = Self::fresh_running(&self.config, now);
            self.last_clock = now;
            effects.extend(self.enough.overlay().map(Effect::StartEnoughOverlay));
            // Still consume the incoming desktop/sleep signal.
            effects.extend(self.handle_event(event, now));
            return effects;
        }
        effects.extend(self.handle_break_event(event, now));
        effects
    }

    fn advance_enough(&mut self, now: ClockSample) -> Vec<Effect> {
        let working = !self.screen_state.idle() && self.active_break().is_none();
        self.enough
            .advance(&self.config.enough, self.last_clock, now, working)
    }

    fn handle_break_event(&mut self, event: SchedulerEvent, now: ClockSample) -> Vec<Effect> {
        self.advance_screen_time(now);
        if !self.screen_state.idle()
            && matches!(
                event,
                SchedulerEvent::SuspendStarted
                    | SchedulerEvent::LockStarted
                    | SchedulerEvent::DisplaysOff(true)
                    | SchedulerEvent::DesktopState {
                        displays_off: true,
                        ..
                    }
                    | SchedulerEvent::DesktopState { locked: true, .. }
            )
        {
            self.last_notified_due = None;
        }
        match event {
            SchedulerEvent::PoweroffFailed { .. } => Vec::new(),
            SchedulerEvent::Tick => self.tick(now),
            SchedulerEvent::SuspendStarted => {
                self.screen_state.sleeping = true;
                self.suspend()
            }
            SchedulerEvent::SuspendEnded => {
                self.screen_state.sleeping = false;
                self.resume_suspended(now)
            }
            SchedulerEvent::LockStarted => {
                self.screen_state.locked = true;
                self.suspend()
            }
            SchedulerEvent::LockEnded => {
                self.screen_state.locked = false;
                self.resume_suspended(now)
            }
            SchedulerEvent::DisplaysOff(off) => {
                self.screen_state.displays_off = off;
                Vec::new()
            }
            SchedulerEvent::DesktopState {
                displays_off,
                locked,
            } => {
                self.screen_state.displays_off = displays_off;
                let was_locked = self.screen_state.locked;
                self.screen_state.locked = locked;
                if locked && !was_locked {
                    self.suspend()
                } else if !locked && was_locked {
                    self.resume_suspended(now)
                } else {
                    Vec::new()
                }
            }
            SchedulerEvent::OverlayFailed { session_id, reason } => {
                if self
                    .active_break()
                    .is_some_and(|active| active.session_id == session_id)
                {
                    vec![
                        Effect::OverlayDegraded {
                            session_id,
                            reason: reason.clone(),
                        },
                        Effect::Notify {
                            summary: "Break overlay unavailable".into(),
                            body: format!("The break is still active: {reason}"),
                        },
                    ]
                } else {
                    Vec::new()
                }
            }
        }
    }

    pub fn handle_command(
        &mut self,
        command: &Command,
        now: ClockSample,
    ) -> Result<Vec<Effect>, SchedulerError> {
        if let Command::AcknowledgeEnough { session_id } = command {
            return self
                .enough
                .acknowledge(*session_id, now)
                .map(|spec| vec![Effect::StartEnoughOverlay(spec)])
                .ok_or(SchedulerError::NotAwaitingEnough);
        }
        if self.enough.session.is_some() {
            return Err(SchedulerError::EnoughActive);
        }
        // Do not lose daily usage between a command and the next tick.
        // Day rollover and triggering are handled by handle_event.
        let _ = self.advance_enough(now);
        self.advance_screen_time(now);
        match command {
            Command::AcknowledgeEnough { .. } => unreachable!(),
            Command::ResumeBreak => self.resume_break(now),
            Command::Postpone => self.postpone(now),
            Command::Mini => self.manual_break(BreakKind::Mini, now),
            Command::Long => self.manual_break(BreakKind::Long, now),
            Command::Rest => self.manual_break(BreakKind::Rest, now),
            Command::Status
            | Command::Reload
            | Command::Outputs
            | Command::Doctor
            | Command::CoopHost { .. }
            | Command::CoopJoin { .. }
            | Command::CoopLeave
            | Command::CoopStatus => Err(SchedulerError::DaemonCommand),
        }
    }

    /// A guest overlay can reach its wall-clock deadline a fraction before the
    /// host due to clock and scheduler jitter. Queue only that forwarded
    /// request when it arrives within one second of the authoritative host
    /// deadline; local commands continue to require an already-finished break.
    pub fn handle_coop_resume_request(
        &mut self,
        now: ClockSample,
    ) -> Result<Vec<Effect>, SchedulerError> {
        self.advance_screen_time(now);
        let mut active = self
            .active_break()
            .cloned()
            .ok_or(SchedulerError::NoActiveBreak)?;
        if !active.manual_resume {
            return Err(SchedulerError::NotAwaitingResume);
        }
        if awaiting_manual_resume(&active, now) {
            return Ok(self.finish_active(active, now));
        }
        const EARLY_RESUME_TOLERANCE_MS: u64 = 1_000;
        if active.ends_boot_ms.saturating_sub(now.boottime_ms) > EARLY_RESUME_TOLERANCE_MS {
            return Err(SchedulerError::NotAwaitingResume);
        }
        active.resume_requested = true;
        self.state = match active.due.kind {
            BreakKind::Mini => SchedulerState::MiniBreak { active },
            BreakKind::Long => SchedulerState::LongBreak { active },
            BreakKind::Rest => SchedulerState::RestBreak { active },
        };
        Ok(Vec::new())
    }

    pub fn status(&self, now: ClockSample) -> SchedulerStatus {
        let mut schedule = self.context().cloned();
        if self.screen_state.idle()
            && let Some(context) = &mut schedule
        {
            refresh_schedule(context, &self.config, now);
        }
        let context = schedule.as_ref();
        let active = self.active_break();
        let paused = matches!(self.state, SchedulerState::PausedUntil { .. });
        let awaiting_resume = active.is_some_and(|active| awaiting_manual_resume(active, now));
        let remaining_ms = if let Some(active) = active {
            Some(active.ends_boot_ms.saturating_sub(now.boottime_ms))
        } else {
            context.map(|context| context.next_due_mono_ms.saturating_sub(now.monotonic_ms))
        };
        let mut status = SchedulerStatus {
            state: if self.screen_state.idle() && active.is_none() {
                "idle".into()
            } else {
                state_name(&self.state).into()
            },
            paused,
            break_kind: active.map(|active| active.due.kind).or_else(|| {
                context.and_then(|context| context.pending.as_ref().map(|due| due.kind))
            }),
            remaining_ms,
            awaiting_resume,
            minis_since_long: context.map_or(0, |context| context.minis_since_long),
            longs_since_rest: context.map_or(0, |context| context.longs_since_rest),
            active_session: active.map(|active| active.session_id),
            postpone_count: active
                .map(|active| active.due.postpone_count)
                .or_else(|| {
                    context
                        .and_then(|context| context.pending.as_ref().map(|due| due.postpone_count))
                })
                .unwrap_or(0),
            can_postpone: !paused
                && !awaiting_resume
                && active.is_some_and(|active| {
                    active.due.mirrored.as_ref().map_or_else(
                        || self.postpone_allowed(active, now),
                        |policy| policy.can_postpone,
                    )
                }),
            screen_time: context.map_or(ScreenTime::default(), |c| c.screen_time),
            idle: self.screen_state.idle(),
            enough: self.enough.clone(),
        };
        if let Some(session) = &self.enough.session {
            status.state = if session.error.is_some() {
                "poweroff-failed"
            } else if session.deadline_boot_ms.is_some() {
                "poweroff-countdown"
            } else {
                "enough"
            }
            .into();
            status.active_session = Some(session.id);
            status.break_kind = None;
            status.can_postpone = false;
            status.awaiting_resume = false;
            status.remaining_ms = session
                .deadline_boot_ms
                .map(|deadline| deadline.saturating_sub(now.boottime_ms));
        }
        status
    }

    /// Whether the active coordination policy asks this desktop to inhibit
    /// compositor shortcuts. The guest keeps its local integration mechanism,
    /// while the host decides whether inhibition is required.
    pub fn shortcut_inhibition_enabled(&self) -> bool {
        if let Some(session) = &self.enough.session {
            return session.deadline_boot_ms.is_none();
        }
        self.coop_policy.as_ref().map_or(
            self.config.strict.mode != StrictMode::Off && self.config.strict.inhibit_shortcuts,
            |policy| policy.inhibit_shortcuts,
        )
    }

    pub fn coop_snapshot(
        &mut self,
        host_id: uuid::Uuid,
        revision: u64,
        now: ClockSample,
    ) -> (CoopSnapshot, bool) {
        let prepared_pending = prepare_pending_for_coop(&mut self.state, &self.config);
        let status = self.status(now);
        let resume_at_unix_ms = match &self.state {
            SchedulerState::PausedUntil { resume_mono_ms, .. } => {
                Some(wall_deadline_from_mono(*resume_mono_ms, now))
            }
            _ => None,
        };
        let phase = if matches!(
            self.state,
            SchedulerState::Suspended { .. } | SchedulerState::Recovering { .. }
        ) || self.screen_state.idle()
        {
            CoopPhase::Unavailable {
                reason: state_name(&self.state).into(),
            }
        } else if let Some(active) = self.active_break() {
            CoopPhase::Break {
                active: SharedBreak {
                    due_id: active.due.id,
                    session_id: active.session_id,
                    kind: active.due.kind,
                    started_unix_ms: wall_deadline_from_boot(active.started_boot_ms, now),
                    ends_unix_ms: wall_deadline_from_boot(active.ends_boot_ms, now),
                    strict_until_unix_ms: wall_deadline_from_boot(active.strict_until_boot_ms, now),
                    strict_entire: self.config.strict.mode == StrictMode::Entire,
                    manual_resume: active.manual_resume,
                    completion_sound_emitted: active.completion_sound_emitted,
                    can_postpone: self.postpone_available(active),
                },
            }
        } else if let Some(context) = self.context() {
            let due = context
                .pending
                .as_ref()
                .expect("co-op snapshot prepares a pending break");
            let duration_ms = self.break_duration(due.kind);
            CoopPhase::Working {
                next: ScheduledBreak {
                    due_id: due.id,
                    kind: due.kind,
                    starts_unix_ms: wall_deadline_from_mono(context.next_due_mono_ms, now),
                    duration_ms,
                    strict_duration_ms: self.strict_duration(duration_ms),
                    strict_entire: self.config.strict.mode == StrictMode::Entire,
                    manual_resume: self.config.completion.manual_resume,
                    can_postpone: self.postpone_available_for(due.kind, due.postpone_count),
                },
            }
        } else {
            CoopPhase::Unavailable {
                reason: state_name(&self.state).into(),
            }
        };

        (
            CoopSnapshot {
                host_id,
                revision,
                generated_unix_ms: now.wall_unix_ms,
                paused: status.paused,
                resume_at_unix_ms,
                phase,
                minis_since_long: status.minis_since_long,
                longs_since_rest: status.longs_since_rest,
                postpone_count: status.postpone_count,
                can_postpone: status.can_postpone,
                policy: Some(CoopPolicy {
                    notifications_enabled: self.config.notifications.enabled,
                    mini_notification_lead_ms: self.config.notifications.mini_lead.as_millis(),
                    long_notification_lead_ms: self.config.notifications.long_lead.as_millis(),
                    rest_notification_lead_ms: self.config.notifications.rest_lead.as_millis(),
                    allow_postpone_during_lockout: self.config.strict.allow_postpone_during_lockout,
                    inhibit_shortcuts: self.config.strict.mode != StrictMode::Off
                        && self.config.strict.inhibit_shortcuts,
                }),
            },
            prepared_pending,
        )
    }

    pub fn adopt_coop_snapshot(
        &mut self,
        snapshot: &CoopSnapshot,
        now: ClockSample,
    ) -> Vec<Effect> {
        self.coop_policy = snapshot.policy.clone();
        self.screen_state = ScreenState::default();
        let old_visible = visible_active(&self.state).map(|active| active.session_id);
        let context = |pending: Option<PendingBreak>, next_due_mono_ms: u64| ScheduleContext {
            screen_time: ScreenTime::default(),
            next_due_mono_ms,
            minis_since_long: snapshot.minis_since_long,
            longs_since_rest: snapshot.longs_since_rest,
            pending,
        };

        let base = match &snapshot.phase {
            CoopPhase::Working { next } => {
                let policy = MirroredBreakPolicy {
                    duration_ms: next.duration_ms,
                    strict_duration_ms: next.strict_duration_ms,
                    strict_entire: next.strict_entire,
                    manual_resume: next.manual_resume,
                    can_postpone: next.can_postpone,
                };
                let due = PendingBreak {
                    id: next.due_id,
                    kind: next.kind,
                    postpone_count: snapshot.postpone_count,
                    manual: false,
                    not_before_mono_ms: 0,
                    mirrored: Some(policy),
                };
                if now.wall_unix_ms >= next.starts_unix_ms {
                    let active = ActiveBreak {
                        context: context(None, now.monotonic_ms),
                        due,
                        session_id: BreakSessionId(next.due_id.0),
                        started_boot_ms: boot_deadline_from_wall(next.starts_unix_ms, now),
                        ends_boot_ms: boot_deadline_from_wall(
                            next.starts_unix_ms.saturating_add(next.duration_ms),
                            now,
                        ),
                        strict_until_boot_ms: boot_deadline_from_wall(
                            next.starts_unix_ms.saturating_add(next.strict_duration_ms),
                            now,
                        ),
                        manual_resume: next.manual_resume,
                        completion_sound_emitted: false,
                        resume_requested: false,
                    };
                    match next.kind {
                        BreakKind::Mini => SchedulerState::MiniBreak { active },
                        BreakKind::Long => SchedulerState::LongBreak { active },
                        BreakKind::Rest => SchedulerState::RestBreak { active },
                    }
                } else {
                    SchedulerState::Running {
                        context: context(
                            Some(due),
                            mono_deadline_from_wall(next.starts_unix_ms, now),
                        ),
                    }
                }
            }
            CoopPhase::Break { active } => {
                let ends_boot_ms = boot_deadline_from_wall(active.ends_unix_ms, now);
                let strict_until_boot_ms =
                    boot_deadline_from_wall(active.strict_until_unix_ms, now);
                let duration_ms = active.ends_unix_ms.saturating_sub(active.started_unix_ms);
                let strict_duration_ms = active
                    .strict_until_unix_ms
                    .saturating_sub(active.started_unix_ms);
                let active = ActiveBreak {
                    context: context(None, now.monotonic_ms),
                    due: PendingBreak {
                        id: active.due_id,
                        kind: active.kind,
                        postpone_count: snapshot.postpone_count,
                        manual: false,
                        not_before_mono_ms: 0,
                        mirrored: Some(MirroredBreakPolicy {
                            duration_ms,
                            strict_duration_ms,
                            strict_entire: active.strict_entire,
                            manual_resume: active.manual_resume,
                            can_postpone: active.can_postpone,
                        }),
                    },
                    session_id: active.session_id,
                    started_boot_ms: boot_deadline_from_wall(active.started_unix_ms, now),
                    ends_boot_ms,
                    strict_until_boot_ms,
                    manual_resume: active.manual_resume,
                    completion_sound_emitted: active.completion_sound_emitted,
                    resume_requested: false,
                };
                match active.due.kind {
                    BreakKind::Mini => SchedulerState::MiniBreak { active },
                    BreakKind::Long => SchedulerState::LongBreak { active },
                    BreakKind::Rest => SchedulerState::RestBreak { active },
                }
            }
            CoopPhase::Unavailable { reason } => SchedulerState::Recovering {
                reason: format!("co-op host unavailable: {reason}"),
            },
        };
        self.state = match (snapshot.paused, snapshot.resume_at_unix_ms) {
            (true, Some(ends_at)) => SchedulerState::PausedUntil {
                inner: Box::new(base),
                paused_at: now,
                resume_mono_ms: mono_deadline_from_wall(ends_at, now),
            },
            _ => base,
        };
        self.last_clock = now;

        let new_visible = visible_active(&self.state);
        let mut effects = Vec::with_capacity(2);
        if old_visible != new_visible.map(|active| active.session_id) {
            if let Some(session_id) = old_visible {
                effects.push(Effect::StopOverlay { session_id });
            }
            if let Some(active) = new_visible
                && (active.manual_resume || active.ends_boot_ms > now.boottime_ms)
            {
                effects.push(Effect::StartOverlay(self.overlay_spec(active, now)));
            }
        }
        effects
    }

    pub fn reset_after_coop_disconnect(&mut self, now: ClockSample) -> Vec<Effect> {
        self.coop_policy = None;
        self.last_clock = now;
        self.reset(now)
    }

    fn fresh_running(config: &AppConfig, now: ClockSample) -> SchedulerState {
        SchedulerState::Running {
            context: ScheduleContext {
                screen_time: ScreenTime::default(),
                next_due_mono_ms: now
                    .monotonic_ms
                    .saturating_add(ScreenTime::default().next(config).1),
                minis_since_long: 0,
                longs_since_rest: 0,
                pending: None,
            },
        }
    }

    fn advance_screen_time(&mut self, now: ClockSample) {
        let mono_delta = now
            .monotonic_ms
            .saturating_sub(self.last_clock.monotonic_ms);
        let boot_delta = now.boottime_ms.saturating_sub(self.last_clock.boottime_ms);
        self.last_clock = now;
        if self.coop_policy.is_some() {
            return;
        }
        let idle = self.screen_state.idle();
        let working = matches!(
            self.state,
            SchedulerState::Running { .. }
                | SchedulerState::PreMiniBreak { .. }
                | SchedulerState::PreLongBreak { .. }
                | SchedulerState::PreRestBreak { .. }
        );
        if let Some(context) = context_in_mut(&mut self.state) {
            if idle {
                context.screen_time.advance(boot_delta, true);
            } else if working {
                context.screen_time.advance(mono_delta, false);
            }
        }
        if working {
            let context = context_in_mut(&mut self.state).expect("working schedule has context");
            refresh_schedule(context, &self.config, now);
            let context = context.clone();
            self.state = SchedulerState::Running { context };
        }
    }

    fn tick(&mut self, now: ClockSample) -> Vec<Effect> {
        if self.screen_state.idle() && self.active_break().is_none() {
            return Vec::new();
        }
        match self.state.clone() {
            SchedulerState::Running { mut context } => {
                let kind = context
                    .pending
                    .as_ref()
                    .map(|due| due.kind)
                    .unwrap_or_else(|| context.screen_time.next(&self.config).0);
                let lead = self.notification_lead(kind);
                if now.monotonic_ms >= context.next_due_mono_ms.saturating_sub(lead) {
                    context.pending.get_or_insert_with(|| PendingBreak {
                        id: DueBreakId::new(),
                        kind,
                        postpone_count: 0,
                        manual: false,
                        not_before_mono_ms: 0,
                        mirrored: None,
                    });
                    if now.monotonic_ms >= context.next_due_mono_ms {
                        return self.begin_scheduled_break(kind, context, now);
                    }
                    let due_id = context
                        .pending
                        .as_ref()
                        .expect("pre-break context has a pending break")
                        .id;
                    self.state = match kind {
                        BreakKind::Mini => SchedulerState::PreMiniBreak { context },
                        BreakKind::Long => SchedulerState::PreLongBreak { context },
                        BreakKind::Rest => SchedulerState::PreRestBreak { context },
                    };
                    if self.notifications_enabled() && self.last_notified_due != Some(due_id) {
                        self.last_notified_due = Some(due_id);
                        return vec![Effect::Notify {
                            summary: format!("{} break soon", title_kind(kind)),
                            body: format!("Starts in {}", DurationMs::from_millis(lead)),
                        }];
                    }
                }
                Vec::new()
            }
            SchedulerState::PreMiniBreak { context }
                if now.monotonic_ms >= context.next_due_mono_ms =>
            {
                self.begin_scheduled_break(BreakKind::Mini, context, now)
            }
            SchedulerState::PreLongBreak { context }
                if now.monotonic_ms >= context.next_due_mono_ms =>
            {
                self.begin_scheduled_break(BreakKind::Long, context, now)
            }
            SchedulerState::PreRestBreak { context }
                if now.monotonic_ms >= context.next_due_mono_ms =>
            {
                self.begin_scheduled_break(BreakKind::Rest, context, now)
            }
            SchedulerState::MiniBreak { active }
            | SchedulerState::LongBreak { active }
            | SchedulerState::RestBreak { active }
                if now.boottime_ms >= active.ends_boot_ms =>
            {
                self.expire_active(active, now)
            }
            SchedulerState::PausedUntil {
                inner,
                paused_at,
                resume_mono_ms,
            } if now.monotonic_ms >= resume_mono_ms => {
                self.state = shift_state(
                    *inner,
                    now.monotonic_ms.saturating_sub(paused_at.monotonic_ms),
                    now.boottime_ms.saturating_sub(paused_at.boottime_ms),
                );
                self.startup_effects(now)
            }
            _ => Vec::new(),
        }
    }

    fn notification_lead(&self, kind: BreakKind) -> u64 {
        if let Some(policy) = &self.coop_policy {
            return match kind {
                BreakKind::Mini => policy.mini_notification_lead_ms,
                BreakKind::Long => policy.long_notification_lead_ms,
                BreakKind::Rest => policy.rest_notification_lead_ms,
            };
        }
        match kind {
            BreakKind::Mini => self.config.notifications.mini_lead.as_millis(),
            BreakKind::Long => self.config.notifications.long_lead.as_millis(),
            BreakKind::Rest => self.config.notifications.rest_lead.as_millis(),
        }
    }

    fn notifications_enabled(&self) -> bool {
        self.coop_policy
            .as_ref()
            .map_or(self.config.notifications.enabled, |policy| {
                policy.notifications_enabled
            })
    }

    fn begin_scheduled_break(
        &mut self,
        kind: BreakKind,
        context: ScheduleContext,
        now: ClockSample,
    ) -> Vec<Effect> {
        let due = context.pending.clone().unwrap_or_else(|| PendingBreak {
            id: DueBreakId::new(),
            kind,
            postpone_count: 0,
            manual: false,
            not_before_mono_ms: 0,
            mirrored: None,
        });
        self.begin_break(kind, context, due, now)
    }

    fn manual_break(
        &mut self,
        kind: BreakKind,
        now: ClockSample,
    ) -> Result<Vec<Effect>, SchedulerError> {
        if self.active_break().is_some() {
            return Err(SchedulerError::BreakAlreadyActive);
        }
        let context = self.context().cloned().unwrap_or(ScheduleContext {
            screen_time: ScreenTime::default(),
            next_due_mono_ms: now.monotonic_ms,
            minis_since_long: 0,
            longs_since_rest: 0,
            pending: None,
        });
        let due = PendingBreak {
            id: DueBreakId::new(),
            kind,
            postpone_count: 0,
            manual: true,
            not_before_mono_ms: 0,
            mirrored: None,
        };
        Ok(self.begin_break(kind, context, due, now))
    }

    fn begin_break(
        &mut self,
        kind: BreakKind,
        context: ScheduleContext,
        due: PendingBreak,
        now: ClockSample,
    ) -> Vec<Effect> {
        let duration = due
            .mirrored
            .as_ref()
            .map_or_else(|| self.break_duration(kind), |policy| policy.duration_ms);
        let ends_boot_ms = now.boottime_ms.saturating_add(duration);
        let strict_until_boot_ms =
            now.boottime_ms
                .saturating_add(due.mirrored.as_ref().map_or_else(
                    || self.strict_duration(duration),
                    |policy| policy.strict_duration_ms,
                ));
        let manual_resume = due
            .mirrored
            .as_ref()
            .map_or(self.config.completion.manual_resume, |policy| {
                policy.manual_resume
            });
        let session_id = BreakSessionId(due.id.0);
        let active = ActiveBreak {
            context,
            due,
            session_id,
            started_boot_ms: now.boottime_ms,
            ends_boot_ms,
            strict_until_boot_ms,
            manual_resume,
            completion_sound_emitted: false,
            resume_requested: false,
        };
        let effect = Effect::StartOverlay(self.overlay_spec(&active, now));
        self.state = match kind {
            BreakKind::Mini => SchedulerState::MiniBreak { active },
            BreakKind::Long => SchedulerState::LongBreak { active },
            BreakKind::Rest => SchedulerState::RestBreak { active },
        };
        vec![effect]
    }

    fn expire_active(&mut self, mut active: ActiveBreak, now: ClockSample) -> Vec<Effect> {
        if !active.manual_resume || active.resume_requested {
            return self.finish_active(active, now);
        }

        let play_completion_sound = !active.completion_sound_emitted;
        active.completion_sound_emitted = true;
        self.state = match active.due.kind {
            BreakKind::Mini => SchedulerState::MiniBreak { active },
            BreakKind::Long => SchedulerState::LongBreak { active },
            BreakKind::Rest => SchedulerState::RestBreak { active },
        };
        play_completion_sound
            .then_some(Effect::PlayCompletionSound)
            .into_iter()
            .collect()
    }

    fn finish_active(&mut self, active: ActiveBreak, now: ClockSample) -> Vec<Effect> {
        let play_completion_sound =
            now.boottime_ms >= active.ends_boot_ms && !active.completion_sound_emitted;
        let session_id = active.session_id;
        let mut context = active.context;
        context.screen_time.mini_ms = 0;
        match active.due.kind {
            BreakKind::Mini => {
                context.minis_since_long = context.minis_since_long.saturating_add(1);
            }
            BreakKind::Long => {
                context.minis_since_long = 0;
                context.screen_time.long_ms = 0;
                context.longs_since_rest = context.longs_since_rest.saturating_add(1);
            }
            BreakKind::Rest => {
                context.minis_since_long = 0;
                context.screen_time.long_ms = 0;
                context.longs_since_rest = 0;
                context.screen_time.rest_ms = 0;
            }
        }
        context.pending = None;
        refresh_schedule(&mut context, &self.config, now);
        self.state = SchedulerState::Running { context };
        let mut effects = Vec::with_capacity(2);
        if play_completion_sound {
            effects.push(Effect::PlayCompletionSound);
        }
        effects.push(Effect::StopOverlay { session_id });
        effects
    }

    fn resume_break(&mut self, now: ClockSample) -> Result<Vec<Effect>, SchedulerError> {
        let active = self
            .active_break()
            .cloned()
            .ok_or(SchedulerError::NoActiveBreak)?;
        if !awaiting_manual_resume(&active, now) {
            return Err(SchedulerError::NotAwaitingResume);
        }
        Ok(self.finish_active(active, now))
    }

    fn postpone(&mut self, now: ClockSample) -> Result<Vec<Effect>, SchedulerError> {
        let mut active = self
            .active_break()
            .cloned()
            .ok_or(SchedulerError::NoActiveBreak)?;
        if awaiting_manual_resume(&active, now) {
            return Err(SchedulerError::AwaitingResume);
        }
        let rule = match active.due.kind {
            BreakKind::Mini => &self.config.postpone.mini,
            BreakKind::Long => &self.config.postpone.long,
            BreakKind::Rest => &self.config.postpone.rest,
        };
        if !rule.enabled {
            return Err(SchedulerError::PostponeDisabled);
        }
        if self.dismissal_locked(&active, now) && !self.config.strict.allow_postpone_during_lockout
        {
            return Err(SchedulerError::StrictMode);
        }
        if active.due.postpone_count >= MAX_POSTPONEMENTS {
            return Err(SchedulerError::PostponeLimit);
        }
        active.due.postpone_count = active.due.postpone_count.saturating_add(1);
        active.due.not_before_mono_ms = now.monotonic_ms.saturating_add(rule.duration.as_millis());
        let session_id = active.session_id;
        let mut context = active.context;
        context.next_due_mono_ms = now.monotonic_ms.saturating_add(rule.duration.as_millis());
        context.pending = Some(active.due);
        self.state = SchedulerState::Running { context };
        Ok(vec![Effect::StopOverlay { session_id }])
    }

    fn suspend(&mut self) -> Vec<Effect> {
        if matches!(self.state, SchedulerState::Suspended { .. }) {
            return Vec::new();
        }
        let effects = self
            .active_break()
            .map(|active| Effect::StopOverlay {
                session_id: active.session_id,
            })
            .into_iter()
            .collect();
        self.state = SchedulerState::Suspended {
            inner: Box::new(self.state.clone()),
        };
        effects
    }

    fn resume_suspended(&mut self, now: ClockSample) -> Vec<Effect> {
        if self.screen_state.locked || self.screen_state.sleeping {
            return Vec::new();
        }
        let SchedulerState::Suspended { inner } = &self.state else {
            return Vec::new();
        };
        self.state = inner.as_ref().clone();
        if let Some(context) = context_in_mut(&mut self.state) {
            refresh_schedule(context, &self.config, now);
        }
        let wake_grace = self.config.recovery.wake_grace.as_millis();
        if wake_grace > 0 {
            self.state = SchedulerState::PausedUntil {
                inner: Box::new(self.state.clone()),
                paused_at: now,
                resume_mono_ms: now.monotonic_ms.saturating_add(wake_grace),
            };
            return Vec::new();
        }
        let mut effects = self.tick(now);
        if self.active_break().is_some() {
            effects.extend(self.startup_effects(now));
        }
        effects
    }

    fn reset(&mut self, now: ClockSample) -> Vec<Effect> {
        let effects = self
            .active_break()
            .map(|active| Effect::StopOverlay {
                session_id: active.session_id,
            })
            .into_iter()
            .collect();
        self.state = Self::fresh_running(&self.config, now);
        effects
    }

    fn break_duration(&self, kind: BreakKind) -> u64 {
        match kind {
            BreakKind::Mini => self.config.schedule.mini.duration.as_millis(),
            BreakKind::Long => self.config.schedule.long.duration.as_millis(),
            BreakKind::Rest => self.config.schedule.rest.duration.as_millis(),
        }
    }

    fn strict_duration(&self, break_duration: u64) -> u64 {
        match self.config.strict.mode {
            StrictMode::Off | StrictMode::Delay => 0,
            StrictMode::Entire => break_duration,
        }
    }

    fn overlay_spec(&self, active: &ActiveBreak, now: ClockSample) -> OverlaySpec {
        let message =
            if self.config.content.show_message && !self.config.content.messages.is_empty() {
                let index = message_index(
                    active.due.id.0.as_u128(),
                    self.config.content.messages.len(),
                );
                Some(self.config.content.messages[index].clone())
            } else {
                None
            };
        OverlaySpec {
            session_id: active.session_id,
            kind: active.due.kind,
            duration: DurationMs::from_millis(active.ends_boot_ms.saturating_sub(now.boottime_ms)),
            strict_remaining: DurationMs::from_millis(
                if active
                    .due
                    .mirrored
                    .as_ref()
                    .map_or(self.config.strict.mode == StrictMode::Entire, |policy| {
                        policy.strict_entire
                    })
                {
                    active.strict_until_boot_ms.saturating_sub(now.boottime_ms)
                } else {
                    0
                },
            ),
            can_postpone: active.due.mirrored.as_ref().map_or_else(
                || self.postpone_available(active),
                |policy| policy.can_postpone,
            ),
            manual_resume: active.manual_resume,
            allow_postpone_during_lockout: self
                .coop_policy
                .as_ref()
                .map_or(self.config.strict.allow_postpone_during_lockout, |policy| {
                    policy.allow_postpone_during_lockout
                }),
            inhibit_shortcuts: self.shortcut_inhibition_enabled(),
            message,
            socket_path: self.socket_path.clone(),
        }
    }

    fn postpone_allowed(&self, active: &ActiveBreak, now: ClockSample) -> bool {
        self.postpone_available(active)
            && (!self.dismissal_locked(active, now)
                || self.config.strict.allow_postpone_during_lockout)
    }

    /// Whether strict mode currently blocks postponing the break.
    /// Only `StrictMode::Entire` locks postponement for the whole
    /// break. Delay mode leaves postponement immediately available.
    fn dismissal_locked(&self, active: &ActiveBreak, now: ClockSample) -> bool {
        active
            .due
            .mirrored
            .as_ref()
            .map_or(self.config.strict.mode == StrictMode::Entire, |policy| {
                policy.strict_entire
            })
            && now.boottime_ms < active.strict_until_boot_ms
    }

    fn postpone_available(&self, active: &ActiveBreak) -> bool {
        self.postpone_available_for(active.due.kind, active.due.postpone_count)
    }

    fn postpone_available_for(&self, kind: BreakKind, postpone_count: u32) -> bool {
        let rule = match kind {
            BreakKind::Mini => &self.config.postpone.mini,
            BreakKind::Long => &self.config.postpone.long,
            BreakKind::Rest => &self.config.postpone.rest,
        };
        rule.enabled && postpone_count < MAX_POSTPONEMENTS
    }

    fn active_break(&self) -> Option<&ActiveBreak> {
        active_break_in(&self.state)
    }

    fn context(&self) -> Option<&ScheduleContext> {
        context_in(&self.state)
    }
}

fn active_break_in(state: &SchedulerState) -> Option<&ActiveBreak> {
    match state {
        SchedulerState::MiniBreak { active }
        | SchedulerState::LongBreak { active }
        | SchedulerState::RestBreak { active } => Some(active),
        SchedulerState::PausedUntil { inner, .. } | SchedulerState::Suspended { inner, .. } => {
            active_break_in(inner)
        }
        _ => None,
    }
}

fn visible_active(state: &SchedulerState) -> Option<&ActiveBreak> {
    match state {
        SchedulerState::MiniBreak { active }
        | SchedulerState::LongBreak { active }
        | SchedulerState::RestBreak { active } => Some(active),
        _ => None,
    }
}

fn prepare_pending_for_coop(state: &mut SchedulerState, config: &AppConfig) -> bool {
    match state {
        SchedulerState::Running { context }
        | SchedulerState::PreMiniBreak { context }
        | SchedulerState::PreLongBreak { context }
        | SchedulerState::PreRestBreak { context } => {
            if context.pending.is_some() {
                return false;
            }
            let kind = context.screen_time.next(config).0;
            context.pending = Some(PendingBreak {
                id: DueBreakId::new(),
                kind,
                postpone_count: 0,
                manual: false,
                not_before_mono_ms: 0,
                mirrored: None,
            });
            true
        }
        SchedulerState::PausedUntil { inner, .. } => prepare_pending_for_coop(inner, config),
        SchedulerState::MiniBreak { .. }
        | SchedulerState::LongBreak { .. }
        | SchedulerState::RestBreak { .. }
        | SchedulerState::Suspended { .. }
        | SchedulerState::Recovering { .. } => false,
    }
}

fn wall_deadline_from_mono(deadline_mono_ms: u64, now: ClockSample) -> u64 {
    translate_deadline(deadline_mono_ms, now.monotonic_ms, now.wall_unix_ms)
}

fn wall_deadline_from_boot(deadline_boot_ms: u64, now: ClockSample) -> u64 {
    translate_deadline(deadline_boot_ms, now.boottime_ms, now.wall_unix_ms)
}

fn mono_deadline_from_wall(deadline_wall_ms: u64, now: ClockSample) -> u64 {
    translate_deadline(deadline_wall_ms, now.wall_unix_ms, now.monotonic_ms)
}

fn boot_deadline_from_wall(deadline_wall_ms: u64, now: ClockSample) -> u64 {
    translate_deadline(deadline_wall_ms, now.wall_unix_ms, now.boottime_ms)
}

fn translate_deadline(deadline: u64, source_now: u64, destination_now: u64) -> u64 {
    if deadline >= source_now {
        destination_now.saturating_add(deadline - source_now)
    } else {
        destination_now.saturating_sub(source_now - deadline)
    }
}

fn context_in(state: &SchedulerState) -> Option<&ScheduleContext> {
    match state {
        SchedulerState::Running { context }
        | SchedulerState::PreMiniBreak { context }
        | SchedulerState::PreLongBreak { context }
        | SchedulerState::PreRestBreak { context } => Some(context),
        SchedulerState::MiniBreak { active }
        | SchedulerState::LongBreak { active }
        | SchedulerState::RestBreak { active } => Some(&active.context),
        SchedulerState::PausedUntil { inner, .. } | SchedulerState::Suspended { inner, .. } => {
            context_in(inner)
        }
        SchedulerState::Recovering { .. } => None,
    }
}

fn state_is_overdue(state: &SchedulerState, now: ClockSample) -> bool {
    match state {
        SchedulerState::Running { context }
        | SchedulerState::PreMiniBreak { context }
        | SchedulerState::PreLongBreak { context }
        | SchedulerState::PreRestBreak { context } => now.monotonic_ms >= context.next_due_mono_ms,
        SchedulerState::MiniBreak { active }
        | SchedulerState::LongBreak { active }
        | SchedulerState::RestBreak { active } => {
            !active.manual_resume && now.boottime_ms >= active.ends_boot_ms
        }
        SchedulerState::PausedUntil { .. }
        | SchedulerState::Suspended { .. }
        | SchedulerState::Recovering { .. } => false,
    }
}

fn shift_state(state: SchedulerState, mono_delta: u64, boot_delta: u64) -> SchedulerState {
    match state {
        SchedulerState::Running { mut context } => {
            shift_context(&mut context, mono_delta);
            SchedulerState::Running { context }
        }
        SchedulerState::PreMiniBreak { mut context } => {
            shift_context(&mut context, mono_delta);
            SchedulerState::PreMiniBreak { context }
        }
        SchedulerState::PreLongBreak { mut context } => {
            shift_context(&mut context, mono_delta);
            SchedulerState::PreLongBreak { context }
        }
        SchedulerState::PreRestBreak { mut context } => {
            shift_context(&mut context, mono_delta);
            SchedulerState::PreRestBreak { context }
        }
        SchedulerState::MiniBreak { mut active } => {
            shift_active(&mut active, mono_delta, boot_delta);
            SchedulerState::MiniBreak { active }
        }
        SchedulerState::LongBreak { mut active } => {
            shift_active(&mut active, mono_delta, boot_delta);
            SchedulerState::LongBreak { active }
        }
        SchedulerState::RestBreak { mut active } => {
            shift_active(&mut active, mono_delta, boot_delta);
            SchedulerState::RestBreak { active }
        }
        other => other,
    }
}

fn shift_context(context: &mut ScheduleContext, mono_delta: u64) {
    context.next_due_mono_ms = context.next_due_mono_ms.saturating_add(mono_delta);
}

fn context_in_mut(state: &mut SchedulerState) -> Option<&mut ScheduleContext> {
    match state {
        SchedulerState::Running { context }
        | SchedulerState::PreMiniBreak { context }
        | SchedulerState::PreLongBreak { context }
        | SchedulerState::PreRestBreak { context } => Some(context),
        SchedulerState::MiniBreak { active }
        | SchedulerState::LongBreak { active }
        | SchedulerState::RestBreak { active } => Some(&mut active.context),
        SchedulerState::PausedUntil { inner, .. } | SchedulerState::Suspended { inner, .. } => {
            context_in_mut(inner)
        }
        SchedulerState::Recovering { .. } => None,
    }
}

fn refresh_schedule(
    context: &mut ScheduleContext,
    config: &AppConfig,
    now: ClockSample,
) -> BreakKind {
    if let Some(due) = &context.pending {
        if due.mirrored.is_some() {
            return due.kind;
        }
        if due.manual {
            context.next_due_mono_ms = due.not_before_mono_ms;
            return due.kind;
        }
    }
    let (kind, remaining) = context.screen_time.next(config);
    if context.screen_time == ScreenTime::default() {
        context.pending = None;
    }
    let mut not_before = 0;
    if let Some(due) = &mut context.pending {
        if due.postpone_count > 0 {
            not_before = due.not_before_mono_ms;
            due.kind = kind;
        } else if due.kind != kind {
            context.pending = None;
        }
    }
    context.next_due_mono_ms = now.monotonic_ms.saturating_add(remaining).max(not_before);
    kind
}

fn shift_active(active: &mut ActiveBreak, mono_delta: u64, boot_delta: u64) {
    shift_context(&mut active.context, mono_delta);
    active.started_boot_ms = active.started_boot_ms.saturating_add(boot_delta);
    active.ends_boot_ms = active.ends_boot_ms.saturating_add(boot_delta);
    active.strict_until_boot_ms = active.strict_until_boot_ms.saturating_add(boot_delta);
}

fn state_name(state: &SchedulerState) -> &'static str {
    match state {
        SchedulerState::Running { .. } => "running",
        SchedulerState::PreMiniBreak { .. } => "pre-mini-break",
        SchedulerState::MiniBreak { .. } => "mini-break",
        SchedulerState::PreLongBreak { .. } => "pre-long-break",
        SchedulerState::LongBreak { .. } => "long-break",
        SchedulerState::PreRestBreak { .. } => "pre-rest-break",
        SchedulerState::RestBreak { .. } => "rest-break",
        SchedulerState::PausedUntil { .. } => "paused-until",
        SchedulerState::Suspended { .. } => "suspended",
        SchedulerState::Recovering { .. } => "recovering",
    }
}

fn title_kind(kind: BreakKind) -> &'static str {
    match kind {
        BreakKind::Mini => "Mini",
        BreakKind::Long => "Long",
        BreakKind::Rest => "Rest",
    }
}

fn message_index(due_id: u128, message_count: usize) -> usize {
    (due_id % message_count as u128) as usize
}

fn awaiting_manual_resume(active: &ActiveBreak, now: ClockSample) -> bool {
    active.manual_resume && now.boottime_ms >= active.ends_boot_ms
}

#[cfg(test)]
mod tests {
    use hyprbreathe_config::defaults;
    use proptest::prelude::*;

    use super::*;

    fn clock(ms: u64) -> ClockSample {
        ClockSample {
            monotonic_ms: ms,
            boottime_ms: ms,
            wall_unix_ms: 1_700_000_000_000 + ms,
        }
    }

    fn test_scheduler() -> Scheduler {
        let mut config = defaults();
        config.schedule.mini.interval = DurationMs::from_millis(1_000);
        config.schedule.mini.duration = DurationMs::from_millis(100);
        config.schedule.long.interval = DurationMs::from_millis(3_000);
        config.schedule.long.duration = DurationMs::from_millis(300);
        config.schedule.rest.interval = DurationMs::from_millis(8_000);
        config.schedule.rest.duration = DurationMs::from_millis(500);
        config.notifications.mini_lead = DurationMs::from_millis(100);
        config.notifications.long_lead = DurationMs::from_millis(100);
        config.notifications.rest_lead = DurationMs::from_millis(100);
        Scheduler::new(
            config,
            "boot".into(),
            clock(0),
            "/tmp/hyprbreathe.sock".into(),
        )
    }

    #[test]
    fn locking_reduces_accumulated_screen_time() {
        let mut scheduler = test_scheduler();
        scheduler.config.recovery.wake_grace = DurationMs::from_millis(0);
        scheduler.handle_event(SchedulerEvent::Tick, clock(800));
        scheduler.handle_event(SchedulerEvent::LockStarted, clock(800));
        scheduler.handle_event(SchedulerEvent::Tick, clock(1_000));
        assert_eq!(scheduler.status(clock(1_000)).remaining_ms, Some(400),);
        scheduler.handle_event(SchedulerEvent::LockEnded, clock(1_100));

        assert_eq!(scheduler.status(clock(1_100)).remaining_ms, Some(500),);
    }

    #[test]
    fn screen_off_delays_a_nearly_due_long_break() {
        let mut scheduler = test_scheduler();
        scheduler.config.schedule.mini.interval = DurationMs::from_millis(10_000);
        scheduler.handle_event(SchedulerEvent::DisplaysOff(true), clock(2_900));
        scheduler.handle_event(SchedulerEvent::DisplaysOff(false), clock(3_400));
        assert_eq!(scheduler.status(clock(3_400)).screen_time.long_ms, 2_400,);
        assert_eq!(scheduler.status(clock(3_400)).remaining_ms, Some(600),);
        assert!(
            scheduler
                .handle_event(SchedulerEvent::Tick, clock(3_900),)
                .iter()
                .all(|effect| !matches!(effect, Effect::StartOverlay(_),))
        );
        let effects = scheduler.handle_event(SchedulerEvent::Tick, clock(4_000));
        assert!(matches!(
            effects.as_slice(), [Effect::StartOverlay(spec)]
                if spec.kind == BreakKind::Long
        ));
    }

    #[test]
    fn recovery_cannot_bank_time_or_keep_an_old_warning() {
        let mut scheduler = test_scheduler();
        scheduler.handle_event(SchedulerEvent::Tick, clock(900));
        assert_eq!(scheduler.status(clock(900)).state, "pre-mini-break",);
        scheduler.handle_event(SchedulerEvent::DisplaysOff(true), clock(900));
        scheduler.handle_event(SchedulerEvent::Tick, clock(5_000));
        scheduler.handle_event(SchedulerEvent::DisplaysOff(false), clock(5_000));
        scheduler.handle_event(SchedulerEvent::Tick, clock(5_100));
        let status = scheduler.status(clock(5_100));
        assert_eq!(status.state, "running");
        assert_eq!(status.remaining_ms, Some(900));
        assert_eq!(
            status.screen_time,
            ScreenTime {
                mini_ms: 100,
                long_ms: 100,
                rest_ms: 100,
            }
        );
    }

    #[test]
    fn overlapping_idle_signals_recover_elapsed_time_once() {
        let mut scheduler = test_scheduler();
        scheduler.config.recovery.wake_grace = DurationMs(0);
        for (event, mono, boot) in [
            (SchedulerEvent::DisplaysOff(true), 800, 800),
            (
                SchedulerEvent::DesktopState {
                    displays_off: true,
                    locked: true,
                },
                900,
                900,
            ),
            (SchedulerEvent::SuspendStarted, 1_000, 1_000),
            (SchedulerEvent::SuspendEnded, 1_000, 1_300),
            (
                SchedulerEvent::DesktopState {
                    displays_off: true,
                    locked: false,
                },
                1_100,
                1_400,
            ),
        ] {
            scheduler.handle_event(
                event,
                ClockSample {
                    monotonic_ms: mono,
                    boottime_ms: boot,
                    wall_unix_ms: 1_700_000_000_000 + boot,
                },
            );
        }
        let now = ClockSample {
            monotonic_ms: 1_200,
            boottime_ms: 1_500,
            wall_unix_ms: 1_700_000_001_500,
        };
        assert!(scheduler.status(now).idle);
        scheduler.handle_event(SchedulerEvent::DisplaysOff(false), now);
        let status = scheduler.status(now);
        assert!(!status.idle);
        assert_eq!(
            status.screen_time,
            ScreenTime {
                mini_ms: 100,
                long_ms: 100,
                rest_ms: 100,
            }
        );
    }

    #[test]
    fn message_selection_uses_the_break_id() {
        assert_eq!(message_index(0, 3), 0);
        assert_eq!(message_index(1, 3), 1);
        assert_eq!(message_index(5, 3), 2);
    }

    #[test]
    fn manual_resume_waits_after_a_mini_break() {
        let mut scheduler = test_scheduler();
        scheduler.config.completion.manual_resume = true;
        let effects = scheduler.handle_command(&Command::Mini, clock(0)).unwrap();
        let [Effect::StartOverlay(spec)] = effects.as_slice() else {
            panic!("expected an overlay");
        };
        assert!(spec.manual_resume);
        assert_eq!(
            scheduler.handle_command(&Command::ResumeBreak, clock(99)),
            Err(SchedulerError::NotAwaitingResume)
        );

        let effects = scheduler.handle_event(SchedulerEvent::Tick, clock(100));
        assert_eq!(effects, vec![Effect::PlayCompletionSound]);
        assert!(
            scheduler
                .handle_event(SchedulerEvent::Tick, clock(101))
                .is_empty()
        );
        let status = scheduler.status(clock(100));
        assert!(status.awaiting_resume);
        assert!(!status.can_postpone);
        assert_eq!(
            scheduler.handle_command(&Command::Postpone, clock(100)),
            Err(SchedulerError::AwaitingResume)
        );

        let effects = scheduler
            .handle_command(&Command::ResumeBreak, clock(100))
            .unwrap();
        assert!(matches!(effects.as_slice(), [Effect::StopOverlay { .. }]));
        assert!(matches!(scheduler.state(), SchedulerState::Running { .. }));
        assert_eq!(scheduler.status(clock(100)).minis_since_long, 1);
        assert_eq!(scheduler.status(clock(100)).remaining_ms, Some(1_000));
    }

    #[test]
    fn manual_resume_applies_to_long_breaks() {
        let mut scheduler = test_scheduler();
        scheduler.config.completion.manual_resume = true;
        scheduler.handle_command(&Command::Long, clock(0)).unwrap();

        let effects = scheduler.handle_event(SchedulerEvent::Tick, clock(300));
        assert_eq!(effects, vec![Effect::PlayCompletionSound]);
        assert!(
            scheduler
                .handle_event(SchedulerEvent::Tick, clock(301))
                .is_empty()
        );
        assert!(scheduler.status(clock(300)).awaiting_resume);
        scheduler
            .handle_command(&Command::ResumeBreak, clock(300))
            .unwrap();
        assert!(matches!(scheduler.state(), SchedulerState::Running { .. }));
        assert_eq!(scheduler.status(clock(300)).minis_since_long, 0);
    }

    #[test]
    fn immediate_manual_resume_still_plays_the_completion_sound() {
        let mut scheduler = test_scheduler();
        scheduler.config.completion.manual_resume = true;
        scheduler.handle_command(&Command::Mini, clock(0)).unwrap();

        let effects = scheduler
            .handle_command(&Command::ResumeBreak, clock(100))
            .unwrap();
        assert!(matches!(
            effects.as_slice(),
            [Effect::PlayCompletionSound, Effect::StopOverlay { .. }]
        ));
    }

    #[test]
    fn restored_manual_break_does_not_replay_the_completion_sound() {
        let mut scheduler = test_scheduler();
        scheduler.config.completion.manual_resume = true;
        scheduler.handle_command(&Command::Mini, clock(0)).unwrap();
        assert_eq!(
            scheduler.handle_event(SchedulerEvent::Tick, clock(100)),
            vec![Effect::PlayCompletionSound]
        );

        let mut restored = Scheduler::restore(
            scheduler.config.clone(),
            "boot".into(),
            clock(101),
            "/tmp/hyprbreathe.sock".into(),
            scheduler.snapshot(),
        );
        assert!(
            restored
                .handle_event(SchedulerEvent::Tick, clock(101))
                .is_empty()
        );
    }

    #[test]
    fn automatic_resume_remains_the_default() {
        let mut scheduler = test_scheduler();
        scheduler.handle_command(&Command::Mini, clock(0)).unwrap();

        let effects = scheduler.handle_event(SchedulerEvent::Tick, clock(100));
        assert!(matches!(
            effects.as_slice(),
            [Effect::PlayCompletionSound, Effect::StopOverlay { .. }]
        ));
        assert!(!scheduler.status(clock(100)).awaiting_resume);
        assert!(matches!(scheduler.state(), SchedulerState::Running { .. }));
    }

    #[test]
    fn postponing_a_break_does_not_play_the_completion_sound() {
        let mut scheduler = test_scheduler();
        scheduler.handle_command(&Command::Mini, clock(0)).unwrap();

        let effects = scheduler
            .handle_command(&Command::Postpone, clock(20))
            .unwrap();
        assert!(matches!(effects.as_slice(), [Effect::StopOverlay { .. }]));
        assert!(
            scheduler
                .handle_event(SchedulerEvent::Tick, clock(100))
                .is_empty()
        );
    }

    #[test]
    fn postponing_preserves_accumulated_screen_time() {
        let mut scheduler = test_scheduler();
        scheduler.handle_event(SchedulerEvent::Tick, clock(1_000));
        scheduler
            .handle_command(&Command::Postpone, clock(1_020))
            .unwrap();
        assert_eq!(
            scheduler.status(clock(1_020)).screen_time,
            ScreenTime {
                mini_ms: 1_000,
                long_ms: 1_000,
                rest_ms: 1_000,
            }
        );
    }

    #[test]
    fn postponed_break_still_requires_its_screen_time() {
        let mut scheduler = test_scheduler();
        scheduler.config.postpone.mini.duration = DurationMs(200);
        scheduler.handle_event(SchedulerEvent::Tick, clock(1_000));
        scheduler
            .handle_command(&Command::Postpone, clock(1_020))
            .unwrap();
        scheduler.handle_event(SchedulerEvent::DisplaysOff(true), clock(1_020));
        scheduler.handle_event(SchedulerEvent::DisplaysOff(false), clock(3_020));
        scheduler.handle_event(SchedulerEvent::Tick, clock(3_220));
        assert!(scheduler.status(clock(3_220)).active_session.is_none());
        assert_eq!(scheduler.status(clock(3_220)).remaining_ms, Some(800),);
    }

    #[test]
    fn larger_due_tier_supersedes_a_postponed_mini() {
        let mut scheduler = test_scheduler();
        scheduler.config.postpone.mini.duration = DurationMs(2_500);
        scheduler.handle_event(SchedulerEvent::Tick, clock(1_000));
        scheduler
            .handle_command(&Command::Postpone, clock(1_020))
            .unwrap();
        scheduler.handle_event(SchedulerEvent::Tick, clock(3_520));
        assert_eq!(
            scheduler.status(clock(3_520)).break_kind,
            Some(BreakKind::Long),
        );
        assert_eq!(scheduler.status(clock(3_520)).postpone_count, 1,);
    }

    #[test]
    fn recovered_break_can_warn_on_its_next_approach() {
        let mut scheduler = test_scheduler();
        scheduler.handle_event(SchedulerEvent::Tick, clock(900));
        scheduler.handle_event(SchedulerEvent::DisplaysOff(true), clock(900));
        scheduler.handle_event(SchedulerEvent::DisplaysOff(false), clock(1_900));
        let effects = scheduler.handle_event(SchedulerEvent::Tick, clock(2_800));
        assert!(matches!(effects.as_slice(), [Effect::Notify { .. }]));
    }

    #[test]
    fn daemon_restart_accounts_for_locked_elapsed_time() {
        let mut scheduler = test_scheduler();
        scheduler.config.recovery.wake_grace = DurationMs(0);
        scheduler.handle_event(SchedulerEvent::LockStarted, clock(800));
        let mut restored = Scheduler::restore(
            scheduler.config.clone(),
            "boot".into(),
            clock(1_800),
            "socket".into(),
            scheduler.snapshot(),
        );
        restored.handle_event(
            SchedulerEvent::DesktopState {
                displays_off: false,
                locked: false,
            },
            clock(1_800),
        );
        assert_eq!(
            restored.status(clock(1_800)).screen_time,
            ScreenTime::default(),
        );
    }

    #[test]
    fn coop_guests_start_the_same_session_with_the_hosts_duration() {
        let mut host = test_scheduler();
        let mut guest = test_scheduler();
        let mut guest_config = guest.config.clone();
        guest_config.schedule.mini.duration = DurationMs::from_millis(800);
        guest.replace_config(guest_config);

        let (snapshot, _) = host.coop_snapshot(uuid::Uuid::nil(), 1, clock(0));
        assert!(guest.adopt_coop_snapshot(&snapshot, clock(50)).is_empty());

        let host_effects = host.handle_event(SchedulerEvent::Tick, clock(1_000));
        let guest_effects = guest.handle_event(SchedulerEvent::Tick, clock(1_000));
        let host_session = host.status(clock(1_000)).active_session.unwrap();
        let guest_status = guest.status(clock(1_000));

        assert_eq!(guest_status.active_session, Some(host_session));
        assert_eq!(guest_status.remaining_ms, Some(100));
        assert!(matches!(host_effects.as_slice(), [Effect::StartOverlay(_)]));
        assert!(matches!(
            guest_effects.as_slice(),
            [Effect::StartOverlay(_)]
        ));
    }

    #[test]
    fn coop_host_owns_manual_resume_and_notification_timing() {
        let mut host = test_scheduler();
        host.config.completion.manual_resume = true;
        host.config.notifications.enabled = true;
        host.config.notifications.mini_lead = DurationMs::from_millis(400);
        host.config.strict.mode = StrictMode::Entire;
        host.config.strict.allow_postpone_during_lockout = true;
        host.config.strict.inhibit_shortcuts = true;

        let mut guest = test_scheduler();
        guest.config.completion.manual_resume = false;
        guest.config.notifications.enabled = false;
        guest.config.notifications.mini_lead = DurationMs::from_millis(10);
        guest.config.strict.mode = StrictMode::Off;
        guest.config.strict.allow_postpone_during_lockout = false;
        guest.config.strict.inhibit_shortcuts = false;

        let (mut snapshot, _) = host.coop_snapshot(uuid::Uuid::nil(), 1, clock(0));
        guest.adopt_coop_snapshot(&snapshot, clock(0));
        let notification = guest.handle_event(SchedulerEvent::Tick, clock(600));
        assert!(matches!(notification.as_slice(), [Effect::Notify { .. }]));

        snapshot.revision = 2;
        guest.adopt_coop_snapshot(&snapshot, clock(610));
        assert!(
            guest
                .handle_event(SchedulerEvent::Tick, clock(610))
                .is_empty(),
            "regular snapshots must not repeat the same notification"
        );

        let effects = guest.handle_event(SchedulerEvent::Tick, clock(1_000));
        let [Effect::StartOverlay(spec)] = effects.as_slice() else {
            panic!("expected the mirrored break overlay");
        };
        assert!(spec.manual_resume);
        assert!(spec.allow_postpone_during_lockout);
        assert!(spec.inhibit_shortcuts);
        assert!(guest.status(clock(1_100)).awaiting_resume);
    }

    #[test]
    fn a_manual_resume_request_just_before_deadline_is_honored_at_zero() {
        let mut scheduler = test_scheduler();
        scheduler.config.completion.manual_resume = true;
        scheduler.handle_command(&Command::Mini, clock(0)).unwrap();

        assert!(
            scheduler
                .handle_coop_resume_request(clock(99))
                .unwrap()
                .is_empty()
        );
        let effects = scheduler.handle_event(SchedulerEvent::Tick, clock(100));
        assert!(matches!(
            effects.as_slice(),
            [Effect::PlayCompletionSound, Effect::StopOverlay { .. }]
        ));
        assert!(matches!(scheduler.state(), SchedulerState::Running { .. }));
    }

    #[test]
    fn a_working_snapshot_arriving_just_after_deadline_starts_without_flicker() {
        let mut host = test_scheduler();
        let mut guest = test_scheduler();
        let (snapshot, _) = host.coop_snapshot(uuid::Uuid::nil(), 1, clock(0));

        let effects = guest.adopt_coop_snapshot(&snapshot, clock(1_001));
        let [Effect::StartOverlay(spec)] = effects.as_slice() else {
            panic!("expected a late working snapshot to start its break");
        };
        let CoopPhase::Working { next } = &snapshot.phase else {
            panic!("host should still be working");
        };
        assert_eq!(spec.session_id.0, next.due_id.0);
        assert_eq!(spec.duration, DurationMs::from_millis(99));
    }

    #[test]
    fn coop_guest_uses_its_own_break_message_when_joining_mid_break() {
        let mut host = test_scheduler();
        host.handle_command(&Command::Mini, clock(0)).unwrap();
        let (snapshot, _) = host.coop_snapshot(uuid::Uuid::nil(), 1, clock(20));

        let mut guest = test_scheduler();
        let mut guest_config = guest.config.clone();
        guest_config.content.messages = vec!["A local message".into()];
        guest.replace_config(guest_config);
        let effects = guest.adopt_coop_snapshot(&snapshot, clock(20));

        let [Effect::StartOverlay(spec)] = effects.as_slice() else {
            panic!("expected the mirrored overlay to start");
        };
        assert_eq!(spec.message.as_deref(), Some("A local message"));
        assert_eq!(spec.duration, DurationMs::from_millis(80));
    }

    #[test]
    fn idle_does_not_dismiss_a_manual_resume_break() {
        let mut scheduler = test_scheduler();
        scheduler.config.completion.manual_resume = true;
        scheduler.handle_command(&Command::Long, clock(0)).unwrap();

        let effects = scheduler.handle_event(SchedulerEvent::DisplaysOff(true), clock(300));
        assert!(effects.is_empty());
        assert!(scheduler.status(clock(300)).awaiting_resume);
        assert!(matches!(
            scheduler.state(),
            SchedulerState::LongBreak { .. }
        ));
    }

    #[test]
    fn lock_recovery_restores_an_expired_manual_resume_break() {
        let mut scheduler = test_scheduler();
        scheduler.config.completion.manual_resume = true;
        scheduler.config.recovery.wake_grace = DurationMs::from_millis(0);
        scheduler.handle_command(&Command::Long, clock(0)).unwrap();
        scheduler.handle_event(SchedulerEvent::LockStarted, clock(10));

        let effects = scheduler.handle_event(SchedulerEvent::LockEnded, clock(600));
        let [Effect::PlayCompletionSound, Effect::StartOverlay(spec)] = effects.as_slice() else {
            panic!("expected the manual-resume overlay to be restored");
        };
        assert!(spec.manual_resume);
        assert_eq!(spec.duration, DurationMs::from_millis(0));
        assert!(scheduler.status(clock(600)).awaiting_resume);
    }

    #[test]
    fn running_enters_prebreak_then_break() {
        let mut scheduler = test_scheduler();
        let effects = scheduler.handle_event(SchedulerEvent::Tick, clock(900));
        assert!(matches!(
            scheduler.state(),
            SchedulerState::PreMiniBreak { .. }
        ));
        assert!(matches!(effects.as_slice(), [Effect::Notify { .. }]));

        let effects = scheduler.handle_event(SchedulerEvent::Tick, clock(1_000));
        assert!(matches!(
            scheduler.state(),
            SchedulerState::MiniBreak { .. }
        ));
        assert!(matches!(effects.as_slice(), [Effect::StartOverlay(_)]));
    }

    #[test]
    fn delay_mode_allows_immediate_postpone() {
        let mut scheduler = test_scheduler();
        let effects = scheduler.handle_command(&Command::Mini, clock(0)).unwrap();
        let Effect::StartOverlay(spec) = &effects[0] else {
            panic!("expected an overlay");
        };
        assert!(spec.can_postpone);
        // Delay mode enables postponement from the first frame.
        assert_eq!(spec.strict_remaining, DurationMs::from_millis(0));
        assert!(scheduler.status(clock(1)).can_postpone);
        assert!(
            scheduler
                .handle_command(&Command::Postpone, clock(1))
                .is_ok()
        );
    }

    #[test]
    fn entire_strict_mode_locks_postpone_for_the_whole_break() {
        let mut scheduler = test_scheduler();
        scheduler.config.strict.mode = StrictMode::Entire;
        let effects = scheduler.handle_command(&Command::Long, clock(0)).unwrap();
        let Effect::StartOverlay(spec) = &effects[0] else {
            panic!("expected an overlay");
        };
        // Entire mode keeps the controls locked for the whole break.
        assert_eq!(spec.strict_remaining, spec.duration);
        assert!(!scheduler.status(clock(10)).can_postpone);
        assert_eq!(
            scheduler.handle_command(&Command::Postpone, clock(10)),
            Err(SchedulerError::StrictMode)
        );

        // The postpone-during-lockout escape hatch still applies in Entire mode.
        scheduler.config.strict.allow_postpone_during_lockout = true;
        assert!(scheduler.status(clock(10)).can_postpone);
        assert!(
            scheduler
                .handle_command(&Command::Postpone, clock(10))
                .is_ok()
        );
    }

    #[test]
    fn each_break_allows_one_postpone_per_cycle() {
        for cmd in [Command::Mini, Command::Long, Command::Rest] {
            let mut s = test_scheduler();
            let effects = s.handle_command(&cmd, clock(0)).unwrap();
            let [Effect::StartOverlay(first)] = &effects[..] else {
                panic!("expected an initial overlay");
            };
            assert!(first.can_postpone);
            let due_id = s.active_break().unwrap().due.id;
            assert!(s.status(clock(20)).can_postpone);
            s.handle_command(&Command::Postpone, clock(20)).unwrap();
            assert_eq!(s.status(clock(20)).postpone_count, 1);

            let SchedulerState::Running { context } = s.state() else {
                panic!("expected a pending postponed break");
            };
            let due = context.next_due_mono_ms;
            let now = clock(due);
            let effects = s.handle_event(SchedulerEvent::Tick, now);
            let [Effect::StartOverlay(spec)] = &effects[..] else {
                panic!("expected a postponed overlay");
            };
            assert_eq!(s.active_break().unwrap().due.id, due_id);
            assert_eq!(spec.kind, first.kind);
            assert!(!spec.can_postpone);
            assert!(!s.status(clock(due + 20)).can_postpone);
            assert_eq!(
                s.handle_command(&Command::Postpone, clock(due + 20)),
                Err(SchedulerError::PostponeLimit)
            );

            let end = due + spec.duration.as_millis();
            s.handle_event(SchedulerEvent::Tick, clock(end));
            let SchedulerState::Running { context } = s.state() else {
                panic!("expected a completed break");
            };
            let next_due = context.next_due_mono_ms;
            let now = clock(next_due);
            let effects = s.handle_event(SchedulerEvent::Tick, now);
            let [Effect::StartOverlay(next)] = &effects[..] else {
                panic!("expected the next cycle's overlay");
            };
            assert_ne!(s.active_break().unwrap().due.id, due_id);
            assert!(next.can_postpone);
            assert!(s.status(clock(next_due)).can_postpone);
            assert_eq!(s.status(clock(next_due)).postpone_count, 0);
            s.handle_command(&Command::Postpone, clock(next_due))
                .unwrap();
        }
    }

    #[test]
    fn mini_postpone_can_be_disabled_independently() {
        let mut scheduler = test_scheduler();
        scheduler.config.postpone.mini.enabled = false;
        let effects = scheduler.handle_command(&Command::Mini, clock(0)).unwrap();
        let Effect::StartOverlay(spec) = &effects[0] else {
            panic!("expected an overlay");
        };
        assert!(!spec.can_postpone);
        assert!(!scheduler.status(clock(20)).can_postpone);
        assert_eq!(
            scheduler.handle_command(&Command::Postpone, clock(20)),
            Err(SchedulerError::PostponeDisabled)
        );
    }

    #[test]
    fn long_postpone_can_remain_enabled() {
        let mut scheduler = test_scheduler();
        scheduler.config.postpone.mini.enabled = false;
        scheduler.handle_command(&Command::Long, clock(0)).unwrap();
        assert!(scheduler.status(clock(20)).can_postpone);
        assert!(
            scheduler
                .handle_command(&Command::Postpone, clock(20))
                .is_ok()
        );
    }

    #[test]
    fn long_suspend_recovers_screen_time_to_zero() {
        let mut scheduler = test_scheduler();
        scheduler.config.recovery.wake_grace = DurationMs(0);
        scheduler.handle_event(SchedulerEvent::SuspendStarted, clock(100));
        scheduler.handle_event(SchedulerEvent::SuspendEnded, clock(700));
        assert!(matches!(scheduler.state(), SchedulerState::Running { .. }));
        assert_eq!(scheduler.status(clock(700)).remaining_ms, Some(1_000));
    }

    #[test]
    fn sleep_and_lock_advance_an_active_break() {
        use SchedulerEvent::{LockEnded, LockStarted};
        use SchedulerEvent::{SuspendEnded, SuspendStarted};

        for (start, end, mono_ms) in [
            (SuspendStarted, SuspendEnded, 10),
            (LockStarted, LockEnded, 50),
        ] {
            let mut timer = test_scheduler();
            timer.config.recovery.wake_grace = DurationMs(0);
            timer.handle_command(&Command::Mini, clock(0)).unwrap();
            let session = timer.status(clock(0)).active_session;
            timer.handle_event(start, clock(10));

            let mut now = clock(50);
            now.monotonic_ms = mono_ms;
            let effects = timer.handle_event(end, now);
            assert!(matches!(
                effects.as_slice(),
                [Effect::StartOverlay(spec)]
                    if spec.duration == DurationMs(50)
            ));
            assert_eq!(timer.status(now).active_session, session);

            now.boottime_ms += 50;
            now.monotonic_ms += 50;
            now.wall_unix_ms += 50;
            timer.handle_event(SchedulerEvent::Tick, now);
            assert!(timer.active_break().is_none());
        }
    }

    #[test]
    fn wake_grace_delays_overlay_recreation() {
        let mut scheduler = test_scheduler();
        scheduler.config.recovery.wake_grace = DurationMs::from_millis(50);
        scheduler.handle_command(&Command::Mini, clock(0)).unwrap();
        scheduler.handle_event(SchedulerEvent::SuspendStarted, clock(10));
        let effects = scheduler.handle_event(SchedulerEvent::SuspendEnded, clock(20));
        assert!(effects.is_empty());
        assert!(matches!(
            scheduler.state(),
            SchedulerState::PausedUntil { .. }
        ));

        let effects = scheduler.handle_event(SchedulerEvent::Tick, clock(70));
        assert!(matches!(effects.as_slice(), [Effect::StartOverlay(_)]));
    }

    #[test]
    fn daemon_restart_preserves_an_active_break_and_its_deadline() {
        let mut active = test_scheduler();
        active.handle_command(&Command::Mini, clock(0)).unwrap();
        let session = active.status(clock(0)).active_session;
        let mut restored = Scheduler::restore(
            active.config.clone(),
            "boot".into(),
            clock(10),
            "/tmp/hyprbreathe.sock".into(),
            active.snapshot(),
        );
        let status = restored.status(clock(10));
        assert_eq!(status.active_session, session);
        assert_eq!(status.remaining_ms, Some(90));
        assert!(matches!(
            restored.startup_effects(clock(10)).as_slice(),
            [Effect::StartOverlay(_)]
        ));

        restored.handle_event(SchedulerEvent::Tick, clock(100));
        assert!(restored.active_break().is_none());
    }

    #[test]
    fn reset_missed_break_policy_discards_overdue_deadline() {
        let scheduler = test_scheduler();
        let mut config = scheduler.config.clone();
        config.recovery.missed_break = MissedBreakPolicy::Reset;
        let restored = Scheduler::restore(
            config,
            "boot".into(),
            clock(2_000),
            "/tmp/hyprbreathe.sock".into(),
            scheduler.snapshot(),
        );
        assert_eq!(restored.status(clock(2_000)).remaining_ms, Some(1_000));
    }

    #[test]
    fn stale_overlay_failure_does_not_affect_active_break() {
        let mut scheduler = test_scheduler();
        scheduler.handle_command(&Command::Mini, clock(0)).unwrap();
        let effects = scheduler.handle_event(
            SchedulerEvent::OverlayFailed {
                session_id: BreakSessionId::new(),
                reason: "stale child".into(),
            },
            clock(1),
        );
        assert!(effects.is_empty());
        assert!(matches!(
            scheduler.state(),
            SchedulerState::MiniBreak { .. }
        ));
    }

    #[test]
    fn three_tier_cadence_reaches_a_rest_break() {
        let mut scheduler = test_scheduler();
        let mut starts = Vec::new();
        let mut last_session = None;
        for time in (0..=10_000).step_by(10) {
            scheduler.handle_event(SchedulerEvent::Tick, clock(time));
            let status = scheduler.status(clock(time));
            if status.active_session != last_session {
                if let (Some(_), Some(kind)) = (status.active_session, status.break_kind) {
                    starts.push((kind, status.longs_since_rest));
                }
                last_session = status.active_session;
            }
        }
        assert_eq!(
            starts,
            vec![
                (BreakKind::Mini, 0),
                (BreakKind::Mini, 0),
                (BreakKind::Long, 0),
                (BreakKind::Mini, 1),
                (BreakKind::Mini, 1),
                (BreakKind::Long, 1),
                (BreakKind::Mini, 2),
                (BreakKind::Rest, 2),
            ]
        );
        let status = scheduler.status(clock(10_000));
        assert_eq!(status.minis_since_long, 0);
        assert_eq!(status.longs_since_rest, 0);
    }

    #[test]
    fn tiers_trigger_without_completed_smaller_breaks() {
        let mut scheduler = test_scheduler();
        scheduler.config.schedule.mini.interval = DurationMs::from_millis(10_000);
        scheduler.handle_event(SchedulerEvent::Tick, clock(3_000));
        assert_eq!(
            scheduler.status(clock(3_000)).break_kind,
            Some(BreakKind::Long),
        );

        let mut scheduler = test_scheduler();
        scheduler.config.schedule.mini.interval = DurationMs::from_millis(10_000);
        scheduler.config.schedule.long.interval = DurationMs::from_millis(10_000);
        scheduler.handle_event(SchedulerEvent::Tick, clock(8_000));
        assert_eq!(
            scheduler.status(clock(8_000)).break_kind,
            Some(BreakKind::Rest),
        );
    }

    #[test]
    fn completed_breaks_pause_and_reset_the_covered_tiers() {
        let mut scheduler = test_scheduler();
        scheduler.handle_event(SchedulerEvent::Tick, clock(1_000));
        scheduler.handle_event(SchedulerEvent::Tick, clock(1_100));
        assert_eq!(
            scheduler.status(clock(1_100)).screen_time,
            ScreenTime {
                mini_ms: 0,
                long_ms: 1_000,
                rest_ms: 1_000
            }
        );
        for time in [2_100, 2_200, 3_200, 3_500] {
            scheduler.handle_event(SchedulerEvent::Tick, clock(time));
        }
        assert_eq!(
            scheduler.status(clock(3_500)).screen_time,
            ScreenTime {
                mini_ms: 0,
                long_ms: 0,
                rest_ms: 3_000
            }
        );
        scheduler
            .handle_command(&Command::Rest, clock(3_500))
            .unwrap();
        scheduler.handle_event(SchedulerEvent::Tick, clock(4_000));
        assert_eq!(
            scheduler.status(clock(4_000)).screen_time,
            ScreenTime::default()
        );
    }

    #[test]
    fn rest_postpone_uses_its_own_rule_and_preserves_the_kind() {
        let mut scheduler = test_scheduler();
        scheduler.config.postpone.rest.duration = DurationMs::from_millis(700);
        scheduler.handle_command(&Command::Rest, clock(0)).unwrap();
        scheduler
            .handle_command(&Command::Postpone, clock(20))
            .unwrap();

        let due = match scheduler.state() {
            SchedulerState::Running { context } => context.next_due_mono_ms,
            state => panic!("unexpected state {state:?}"),
        };
        assert_eq!(due, 720);
        let effects = scheduler.handle_event(SchedulerEvent::Tick, clock(due));
        let [Effect::StartOverlay(spec)] = effects.as_slice() else {
            panic!("expected the postponed overlay");
        };
        assert_eq!(spec.kind, BreakKind::Rest);
        assert_eq!(
            scheduler.handle_command(&Command::Postpone, clock(due + 20)),
            Err(SchedulerError::PostponeLimit)
        );
    }

    #[test]
    fn daemon_restart_preserves_screen_time() {
        let mut scheduler = test_scheduler();
        scheduler.handle_event(SchedulerEvent::Tick, clock(500));
        let snapshot = scheduler.snapshot();
        let mut restored = Scheduler::restore(
            scheduler.config.clone(),
            "boot".into(),
            clock(500),
            "/tmp/hyprbreathe.sock".into(),
            snapshot,
        );
        restored.handle_event(SchedulerEvent::Tick, clock(600));
        assert_eq!(
            restored.status(clock(600)).screen_time,
            ScreenTime {
                mini_ms: 600,
                long_ms: 600,
                rest_ms: 600,
            },
        );
    }

    #[test]
    fn seven_simulated_days_preserve_scheduler_invariants() {
        let mut scheduler = test_scheduler();
        for time in (0..=7 * 24 * 60 * 60 * 1_000).step_by(10_000) {
            scheduler.handle_event(SchedulerEvent::Tick, clock(time));
            let status = scheduler.status(clock(time));
            assert!(status.active_session.is_none() || status.break_kind.is_some());
            assert!(status.postpone_count <= 1);
        }
    }

    proptest! {
        #[test]
        fn arbitrary_monotonic_ticks_keep_status_consistent(
            deltas in prop::collection::vec(0_u16..5_000, 1..500)
        ) {
            let mut scheduler = test_scheduler();
            let mut time = 0_u64;
            for delta in deltas {
                time = time.saturating_add(u64::from(delta));
                scheduler.handle_event(SchedulerEvent::Tick, clock(time));
                let status = scheduler.status(clock(time));
                prop_assert!(status.active_session.is_none() || status.break_kind.is_some());
                prop_assert!(status.postpone_count <= 1);
            }
        }
    }
}
