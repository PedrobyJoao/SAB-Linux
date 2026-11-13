v0.2.0:

- [x] change name
- [x] change versions
- [x] remove cmds: pause, reset, toggle, resume
- [x] remove oppacity option, there should be NO transparacy at all
- [x] remove skip option and cmd
- [x] one postponement by default
- [x] improve idle identification
- [x] cicd: review ci/cd (for PRs)
- [x] remove: idle, wake grace options

v0.3.0:

- [x] remove from CICD: bootstrap sha and release_as in release-please-config.json
- [x] enough timer: it turns off the computer
      (this one would be nicer to have a countdown timer popup somewhere
      so that the user can close their apps themselves)
- [x] persistent on reboot (add as config)
- [ ] changes to SOME config only applied for the next day (add as config)
- [ ] weekly schedule with different timers
- [ ] some way of seeing screen time easily: tray, popup corner window,
      small window UI opened with shortcut
      (if this is not implemented, enough notification should come way earlier
      several times)
- [ ] change name to StopAndBreathe (as it will later support X11)
- [ ] another repo
- [ ] remove old configs: startup

v0.3.1:

- [ ] ui: DON'T DARE TO GET YOUR PHONE (permanent msg somewhere in the reminder)
  - recommend to put phone away whie on pc
- [ ] ui: messages bigger than time
- [ ] ui: nice background collor
- [ ] plum village bell sound
- [ ] plum village inspiration mention readm
- [ ] cicd: sec scan
- [ ] cicd: dependabot
- [ ] calendar meetings disabling reminders
- [ ] put limits on configurable timers by users (e.g.:
      don't allow postponemnet duration of 30min)
- [ ] popup counter for all reminders

v0.3.2:

- [ ] remove postpone cmd
- [ ] decide about messages defaults
- [ ] lol rust builds takes an eternity, solves that or another pl?
- [ ] AUR publish auth
- [ ] docs: recommend not having cellphone near you to not be a fallback
- [ ] homemanager module
- [ ] nixpkgs?
- [ ] mindfulness pretty frontend
- [ ] remove strict option
- [ ] docs: remove unnecessary things
  - [ ] docs: which coop docs to keep
- [ ] don't necessarily include UI in the package

vFuture:

- [ ] support X11 and other compositors (make it modular for that)
- [ ] allow users to set their own reminder schedules (e.g.:
      I want to take Long at 09:00, 09:30...)
