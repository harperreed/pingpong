// ABOUTME: Captive-portal connectivity probe over plain HTTP.
// ABOUTME: Classifies network as Online, CaptivePortal, or Offline.

// Public items here are wired into the running app in later hardening tasks;
// until then they are exercised only by unit tests. Temporary; remove once wired.
#![allow(dead_code)]

#[derive(Debug, Clone, PartialEq)]
pub enum ProbeResult {
    Online,
    CaptivePortal { url: String },
    Offline,
}
