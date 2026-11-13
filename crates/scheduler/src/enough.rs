use chrono::{Local, TimeZone};
use hyprbreathe_core::{BreakSessionId, ClockSample, DurationMs};
use hyprbreathe_core::{EnoughConfig as Config, EnoughOverlaySpec};
use serde::{Deserialize, Serialize};

use crate::Effect;

/// Independent of break cycles and their idle recovery rules.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnoughTimer {
    pub day: Option<String>,
    pub used_ms: u64,
    pub session: Option<EnoughSession>,
    #[serde(skip)]
    poweroff_in_flight: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnoughSession {
    pub id: BreakSessionId,
    pub poweroff_countdown: DurationMs,
    pub deadline_boot_ms: Option<u64>,
    pub error: Option<String>,
}

impl EnoughTimer {
    pub(crate) fn reboot(&mut self, cfg: &Config, now: ClockSample) {
        // Normalize the local day without counting powered-off time.
        let _ = self.advance(cfg, now, now, false);
        self.poweroff_in_flight = false;
        if let Some(session) = &mut self.session {
            session.id = BreakSessionId::new();
            session.deadline_boot_ms = None;
            session.error = None;
        }
    }

    pub fn advance(
        &mut self,
        config: &Config,
        previous: ClockSample,
        now: ClockSample,
        working: bool,
    ) -> Vec<Effect> {
        let (day, since_midnight) = local_day(now.wall_unix_ms);
        let mut elapsed = now.monotonic_ms.saturating_sub(previous.monotonic_ms);
        let mut effects = Vec::new();
        if self.day.as_ref() != Some(&day) {
            if let Some(session) = self.session.take() {
                effects.push(Effect::StopOverlay {
                    session_id: session.id,
                });
            }
            self.day = Some(day);
            self.used_ms = 0;
            self.poweroff_in_flight = false;
            elapsed = elapsed.min(since_midnight);
        }
        if config.enabled && working && self.session.is_none() {
            self.used_ms = self.used_ms.saturating_add(elapsed);
        }
        effects
    }

    pub fn trigger(&mut self, config: &Config) -> bool {
        if !config.enabled || self.session.is_some() || self.used_ms < config.limit.as_millis() {
            return false;
        }
        self.session = Some(EnoughSession {
            id: BreakSessionId::new(),
            poweroff_countdown: config.poweroff_countdown,
            deadline_boot_ms: None,
            error: None,
        });
        true
    }

    pub fn acknowledge(
        &mut self,
        id: BreakSessionId,
        now: ClockSample,
    ) -> Option<EnoughOverlaySpec> {
        let session = self.session.as_mut()?;
        if session.id != id || session.deadline_boot_ms.is_some() {
            return None;
        }
        session.deadline_boot_ms = Some(
            now.boottime_ms
                .saturating_add(session.poweroff_countdown.as_millis()),
        );
        self.overlay()
    }

    pub fn request_poweroff(&mut self, now: ClockSample) -> Option<Effect> {
        let session = self.session.as_ref()?;
        if self.poweroff_in_flight
            || session.error.is_some()
            || session.deadline_boot_ms? > now.boottime_ms
        {
            return None;
        }
        self.poweroff_in_flight = true;
        Some(Effect::RequestPoweroff {
            session_id: session.id,
        })
    }

    pub fn poweroff_failed(
        &mut self,
        id: BreakSessionId,
        reason: String,
    ) -> Option<EnoughOverlaySpec> {
        let session = self.session.as_mut()?;
        if session.id != id {
            return None;
        }
        session.error = Some(reason);
        self.overlay()
    }

    pub fn overlay(&self) -> Option<EnoughOverlaySpec> {
        self.session.as_ref().map(|session| EnoughOverlaySpec {
            session_id: session.id,
            poweroff_countdown: session.poweroff_countdown,
            deadline_boot_ms: session.deadline_boot_ms,
            error: session.error.clone(),
        })
    }
}

pub(crate) fn local_day(wall_ms: u64) -> (String, u64) {
    let timestamp = i64::try_from(wall_ms).unwrap_or(i64::MAX);
    let local = Local
        .timestamp_millis_opt(timestamp)
        .single()
        .expect("valid system clock timestamp");
    let midnight = local
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .expect("valid midnight")
        .and_local_timezone(Local)
        .earliest()
        .unwrap_or(local);
    let elapsed = local
        .signed_duration_since(midnight)
        .num_milliseconds()
        .max(0) as u64;
    (local.date_naive().to_string(), elapsed)
}
