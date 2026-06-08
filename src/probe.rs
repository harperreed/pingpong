// ABOUTME: Captive-portal connectivity probe over plain HTTP.
// ABOUTME: Classifies network as Online, CaptivePortal, or Offline.

// ProbeResult is referenced only from code the binary's entry point does not reach
// (other leaf modules and tests), so the compiler reports it as dead.
#![allow(dead_code)]

#[derive(Debug, Clone, PartialEq)]
pub enum ProbeResult {
    Online,
    CaptivePortal { url: String },
    Offline,
}
