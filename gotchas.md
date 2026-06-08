# Gotchas

Hard-won lessons from working on pingpong. One bullet per trap, written as a rule so the
same mistake isn't repeated.

- **surge-ping needs no root in the common case.** It defaults to a DGRAM ("unprivileged")
  ICMP socket and automatically falls back to RAW. Don't add privilege checks or sudo
  hints up front. `Client::new` only fails when *both* socket types are denied — handle
  that one error gracefully (report it and back off) instead of assuming elevation is
  always required.

- **crossterm has no get-title API.** To save and restore the user's terminal title around
  the app, use the xterm title stack directly: write `\x1b[22;2t` to push (save) the title
  on enter and `\x1b[23;2t` to pop (restore) it on leave (see `terminal_enter`/
  `terminal_leave` in `tui.rs`). `crossterm::SetTitle` only *sets* a title (OSC 0/2); it
  cannot read the current one. OSC title writes are terminal-level, so they work fine on
  the alternate screen.

- **fish `$status` after a pipe is the last command's exit code.** In `cmd | tail`, `$status`
  reflects `tail`, not `cmd`. To check a tool's real exit (e.g. `cargo clippy -D warnings`),
  run it without a trailing pipe, inspect `$pipestatus`, or rely on a downstream signal
  (a passing `cargo test` proves compilation succeeded). Don't trust an `EXIT=$status`
  printed after a pipeline.

- **Validate config floats with `is_finite()` before building Durations.**
  `Duration::from_secs_f64(NaN)` and `from_secs_f64(INFINITY)` panic. A hand-edited TOML
  can carry either, so `Config::validate()` rejects non-finite (and too-small) `interval`
  and `timeout` and substitutes finite defaults. Any new float config that becomes a
  Duration needs the same guard.

- **In a binary crate, `pub` does not make an item reachable.** Items and struct/enum fields
  that are never *read* trip the `dead_code` lint under `cargo clippy -- -D warnings`.
  Critically, **test-only usage does NOT save a `pub fn` from `dead_code` in the binary
  target** — the `#[cfg(test)]` module is a separate compilation. Add `#[allow(dead_code)]`
  empirically (only on what clippy actually flags), with an evergreen comment explaining
  why it's unread, and remove the allow the moment the item is wired into live code.

- **ratatui 0.29 `Sparkline::data` takes `Vec<Option<u64>>`.** `Some(v)` is a bar, `None`
  renders as a gap (via `impl From<Option<u64>> for SparklineBar`). That maps cleanly onto
  ping history: real RTTs become bars, timeouts/errors become gaps — no sentinel value
  needed.

- **A resumed session can leave git HEAD detached at the commit SHA, not on the branch.**
  Commits then land on the detached HEAD and the branch is left behind. Before committing
  after a resume, run `git symbolic-ref -q HEAD`; if it's detached, confirm the old branch
  tip is an ancestor of HEAD (`git merge-base --is-ancestor`) and then `git checkout -B
  <branch>` to fast-forward the branch onto HEAD without orphaning anything.

- **The ping loop resolves DNS once per connection attempt, not per ping.** It resolves,
  reuses the IP for the inner ping loop, and re-resolves only after several consecutive
  failures — that re-resolve is the mechanism for handling IP changes and reconnects.
  Don't "optimize" it to resolve on every ping (that would also block a worker on each
  iteration; the blocking lookup still belongs in `spawn_blocking`).
