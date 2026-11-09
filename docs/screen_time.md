### Screen time and idle recovery

Mini, Long, and Rest each have an accumulated screen-time counter.
While a display is on and the session is unlocked, each counter
increases one second per second. Reading and watching movies count
as screen time, whereas keyboard and mouse inactivity do not imply idle.

Idle means the session is locked, all displays are off/disabled,
or the computer is asleep. With `[idle].enabled = true` (the default),
each idle second removes one second from each counter, stopping at
zero. There is no waiting threshold, abrupt full reset, or recovery
credit saved for future use. Disabling this option pauses the
counters while idle without reducing them.

A break becomes due when its counter reaches its interval. If
several tiers are due, only the largest is shown. Scheduled breaks
do not add screen time. Completing a Mini resets its counter;
completing a Long resets Mini and Long; completing a Rest resets
all three. Postponing a break does not reset accumulated screen time.

Display power and session lock come from Hyprland's reported state,
checked every second. Sleep is tracked independently through logind.
Dimming alone does not count as idle. If you leave
without locking or switching off the displays, time still counts.
A physical monitor power button may not update the reported state.
If a display-power or lock query fails, the daemon warns and retries.
Unknown state does not grant recovery; a known lock or display-off
state still counts as idle even if the other query fails.

Counters survive a daemon restart; rebooting starts a fresh schedule.
This scheduling update also starts a fresh schedule when replacing
an older saved-state format.
