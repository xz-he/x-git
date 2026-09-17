use std::sync::atomic::{AtomicU8, Ordering};

const IDLE: u8 = 0;
const CLEANING: u8 = 1;
const COMPLETE: u8 = 2;

#[derive(Default)]
pub struct ConsoleExitGate(AtomicU8);
#[derive(Debug, PartialEq, Eq)]
pub enum ExitAction {
    BeginCleanup,
    PreventExit,
    AllowExit,
}
impl ConsoleExitGate {
    pub fn request(&self) -> ExitAction {
        match self
            .0
            .compare_exchange(IDLE, CLEANING, Ordering::SeqCst, Ordering::SeqCst)
        {
            Ok(_) => ExitAction::BeginCleanup,
            Err(COMPLETE) => ExitAction::AllowExit,
            Err(_) => ExitAction::PreventExit,
        }
    }
    pub fn complete(&self) {
        self.0.store(COMPLETE, Ordering::SeqCst);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn repeated_close_requests_cannot_bypass_pending_cleanup() {
        let gate = ConsoleExitGate::default();
        assert_eq!(gate.request(), ExitAction::BeginCleanup);
        assert_eq!(gate.request(), ExitAction::PreventExit);
        assert_eq!(gate.request(), ExitAction::PreventExit);
        gate.complete();
        assert_eq!(gate.request(), ExitAction::AllowExit);
        assert_eq!(gate.request(), ExitAction::AllowExit);
    }
}
