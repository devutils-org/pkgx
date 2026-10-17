use pkgx_core::{
    command::{CommandOutput, CommandRunner, CommandSpec, ProcessRunner},
    Error,
};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
pub struct Session {
    pub canceled: Arc<AtomicBool>,
    pub sudo_requested: bool,
    #[cfg(unix)]
    signals: Vec<signal_hook::SigId>,
}
impl Session {
    pub fn new(sudo_requested: bool) -> std::io::Result<Self> {
        let canceled = Arc::new(AtomicBool::new(false));
        #[cfg(unix)]
        let signals = [signal_hook::consts::SIGINT, signal_hook::consts::SIGTERM]
            .into_iter()
            .map(|signal| signal_hook::flag::register(signal, canceled.clone()))
            .collect::<std::io::Result<Vec<_>>>()?;
        Ok(Self {
            canceled,
            sudo_requested,
            #[cfg(unix)]
            signals,
        })
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        #[cfg(unix)]
        for signal in &self.signals {
            signal_hook::low_level::unregister(*signal);
        }
    }
}
impl CommandRunner for Session {
    fn run(
        &self,
        spec: &CommandSpec,
        timeout: Duration,
        canceled: &AtomicBool,
    ) -> Result<CommandOutput, Error> {
        if canceled.load(Ordering::Relaxed) {
            return Err(Error::Canceled(spec.program.clone()));
        }
        ProcessRunner.run(spec, timeout, &self.canceled)
    }
}
