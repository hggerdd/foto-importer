use anyhow::{Result, bail};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
    mpsc,
};
use std::time::{Duration, Instant};

#[derive(Clone)]
pub struct Control {
    pub cancel: Arc<AtomicBool>,
    pub done: Arc<AtomicUsize>,
    pub total: Arc<AtomicUsize>,
    pub foreground: Arc<AtomicUsize>,
    pub background: bool,
    pub face_phase: Arc<Mutex<Option<FacePhase>>>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FacePhase {
    Analysis,
    References,
    Matching,
    Groups,
}
fn foreground_count() -> Arc<AtomicUsize> {
    static COUNT: std::sync::OnceLock<Arc<AtomicUsize>> = std::sync::OnceLock::new();
    COUNT.get_or_init(|| Arc::new(AtomicUsize::new(0))).clone()
}
fn last_foreground() -> &'static std::sync::Mutex<Option<Instant>> {
    static LAST: std::sync::OnceLock<std::sync::Mutex<Option<Instant>>> =
        std::sync::OnceLock::new();
    LAST.get_or_init(|| std::sync::Mutex::new(None))
}
impl Default for Control {
    fn default() -> Self {
        Self {
            cancel: Arc::new(AtomicBool::new(false)),
            done: Arc::new(AtomicUsize::new(0)),
            total: Arc::new(AtomicUsize::new(0)),
            foreground: foreground_count(),
            background: false,
            face_phase: Arc::new(Mutex::new(None)),
        }
    }
}
pub struct Foreground(Arc<AtomicUsize>);
pub fn foreground() -> Foreground {
    let count = foreground_count();
    count.fetch_add(1, Ordering::Relaxed);
    Foreground(count)
}
impl Drop for Foreground {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Relaxed);
        if let Ok(mut last) = last_foreground().lock() {
            *last = Some(Instant::now());
        }
    }
}

type LaneState = (
    std::sync::Mutex<std::collections::HashMap<String, std::collections::VecDeque<u64>>>,
    std::sync::Condvar,
);
fn lanes() -> &'static LaneState {
    static LANES: std::sync::OnceLock<LaneState> = std::sync::OnceLock::new();
    LANES.get_or_init(|| {
        (
            std::sync::Mutex::new(Default::default()),
            std::sync::Condvar::new(),
        )
    })
}
pub struct Lane {
    key: String,
    token: u64,
}
impl Drop for Lane {
    fn drop(&mut self) {
        let (lock, condition) = lanes();
        if let Ok(mut queues) = lock.lock() {
            if let Some(queue) = queues.get_mut(&self.key) {
                queue.retain(|t| *t != self.token);
                if queue.is_empty() {
                    queues.remove(&self.key);
                }
            }
            condition.notify_all();
        }
    }
}
impl Control {
    pub fn set_face_phase(&self, phase: FacePhase) {
        self.progress(0, 0);
        if let Ok(mut current) = self.face_phase.lock() {
            *current = Some(phase);
        }
    }
    pub fn face_phase(&self) -> Option<FacePhase> {
        self.face_phase.lock().ok().and_then(|current| *current)
    }
    pub fn checkpoint(&self) -> Result<()> {
        loop {
            if self.cancel.load(Ordering::Relaxed) {
                bail!("Auftrag abgebrochen");
            }
            let settled = last_foreground()
                .lock()
                .map(|last| last.is_none_or(|time| time.elapsed() >= Duration::from_millis(150)))
                .unwrap_or(true);
            if !self.background || self.foreground.load(Ordering::Relaxed) == 0 && settled {
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
    pub fn lane(&self, key: &str) -> Result<Lane> {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let token = NEXT.fetch_add(1, Ordering::Relaxed);
        let guard = Lane {
            key: key.into(),
            token,
        };
        let (lock, condition) = lanes();
        {
            let mut queues = lock
                .lock()
                .map_err(|_| anyhow::anyhow!("Auftragswarteschlange beschädigt"))?;
            queues.entry(key.into()).or_default().push_back(token);
        }
        loop {
            if self.cancel.load(Ordering::Relaxed) {
                bail!("Wartender Auftrag abgebrochen");
            }
            let queues = lock
                .lock()
                .map_err(|_| anyhow::anyhow!("Auftragswarteschlange beschädigt"))?;
            if queues.get(key).and_then(|q| q.front()) == Some(&token) {
                return Ok(guard);
            }
            drop(
                condition
                    .wait_timeout(queues, Duration::from_millis(30))
                    .map_err(|_| anyhow::anyhow!("Auftragswarteschlange beschädigt"))?,
            );
        }
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
        Self::start_control(label, Control::default(), work)
    }
    pub fn start_control(
        label: impl Into<String>,
        control: Control,
        work: impl FnOnce(Control) -> Result<T> + Send + 'static,
    ) -> Self {
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
