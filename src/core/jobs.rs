use anyhow::{Result, bail};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
    mpsc,
};
use std::time::{Duration, Instant};

#[derive(Clone, Default)]
pub struct Control {
    pub cancel: Arc<AtomicBool>,
    pub done: Arc<AtomicUsize>,
    pub total: Arc<AtomicUsize>,
    pub foreground: Arc<AtomicUsize>,
}
impl Control {
    pub fn checkpoint(&self) -> Result<()> {
        loop {
            if self.cancel.load(Ordering::Relaxed) {
                bail!("Auftrag abgebrochen");
            }
            if self.foreground.load(Ordering::Relaxed) == 0 {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    pub fn progress(&self, done: usize, total: usize) {
        self.total.store(total, Ordering::Relaxed);
        self.done.store(done, Ordering::Relaxed);
    }
    pub fn stop(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

pub struct Task<T> {
    pub control: Control,
    pub label: String,
    pub started: Instant,
    receiver: mpsc::Receiver<Result<T>>,
}
impl<T: Send + 'static> Task<T> {
    pub fn start(
        label: impl Into<String>,
        work: impl FnOnce(Control) -> Result<T> + Send + 'static,
    ) -> Self {
        let control = Control::default();
        let worker_control = control.clone();
        let (sender, receiver) = mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let result =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| work(worker_control)))
                    .unwrap_or_else(|_| {
                        Err(anyhow::anyhow!(
                            "Interner Workerfehler; Auftrag wurde beendet"
                        ))
                    });
            let _ = sender.send(result);
        });
        Self {
            control,
            label: label.into(),
            receiver,
            started: Instant::now(),
        }
    }
    pub fn poll(&self) -> Option<Result<T>> {
        match self.receiver.try_recv() {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => {
                Some(Err(anyhow::anyhow!("Worker wurde unerwartet beendet")))
            }
        }
    }
}
impl<T> Drop for Task<T> {
    fn drop(&mut self) {
        self.control.stop();
    }
}

/// Processes never inherit a visible console window on Windows.
pub fn command(exe: impl AsRef<std::ffi::OsStr>) -> std::process::Command {
    let mut command = std::process::Command::new(exe);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000 | 0x00004000);
    }
    command
}

pub fn wait_child(
    child: &mut std::process::Child,
    control: &Control,
    timeout: Duration,
) -> Result<std::process::ExitStatus> {
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        if control.cancel.load(Ordering::Relaxed) || started.elapsed() > timeout {
            let _ = child.kill();
            let _ = child.wait();
            bail!("Prozess abgebrochen oder Zeitlimit erreicht");
        }
        std::thread::sleep(Duration::from_millis(30));
    }
}
