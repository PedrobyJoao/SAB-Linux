use hyprbreathe_core::{AppConfig as Config, ClockSample as Clock};
use hyprbreathe_core::{EnoughConfig, ScheduleConfig};
use serde::{Deserialize, Serialize};

use crate::enough::local_day;

/// Today's rules and the latest accepted preferences for tomorrow.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DailyConfigLock {
    day: String,
    active: Rules,
    pending: Option<Rules>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Rules {
    enabled: bool,
    schedule: ScheduleConfig,
    enough: EnoughConfig,
}

impl Rules {
    fn from_config(config: &Config) -> Self {
        Self {
            enabled: config.daily_config_lock.enabled,
            schedule: config.schedule.clone(),
            enough: config.enough.clone(),
        }
    }

    fn apply(&self, config: &mut Config) {
        config.daily_config_lock.enabled = self.enabled;
        config.schedule = self.schedule.clone();
        config.enough = self.enough.clone();
    }
}

impl DailyConfigLock {
    pub(crate) fn new(config: &Config, now: Clock) -> Self {
        Self {
            day: local_day(now.wall_unix_ms).0,
            active: Rules::from_config(config),
            pending: None,
        }
    }

    pub(crate) fn advance(&mut self, cfg: &mut Config, now: Clock) {
        let day = local_day(now.wall_unix_ms).0;
        // A backwards clock adjustment must not unlock today's rules.
        if day <= self.day {
            return;
        }
        self.day = day;
        if let Some(rules) = self.pending.take() {
            self.active = rules;
            self.active.apply(cfg);
        }
    }

    pub(crate) fn replace(&mut self, cfg: &mut Config, now: Clock) {
        let requested = Rules::from_config(cfg);
        self.advance(cfg, now);
        if !self.active.enabled {
            if requested.enabled {
                // Enabling freezes the rules already active today.
                self.active.enabled = true;
            } else {
                self.active = requested.clone();
            }
        }
        let changed = requested != self.active;
        self.pending = changed.then_some(requested);
        self.active.apply(cfg);
    }

    pub(crate) fn has_pending(&self) -> bool {
        self.pending.is_some()
    }
}
