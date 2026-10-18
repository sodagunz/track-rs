//! One detached owner for all potentially blocking backend work, including drop.
use super::{discover_ports, display_name, select_port, DeviceBrowser, InputQueue, InputSender};
use std::{
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc::{self, Receiver, SyncSender},
        Arc,
    },
    time::{Duration, Instant},
};

pub(super) const DEADLINE: Duration = Duration::from_secs(5);
const WAKE: Duration = Duration::from_millis(10);

#[derive(Clone, Copy)]
pub(super) enum Operation {
    List,
    Connect(usize),
}

pub(super) enum Payload {
    List(DeviceBrowser),
    Connected(InputQueue, String),
}

pub(super) struct Reply {
    pub generation: u64,
    pub result: Result<Payload, String>,
    pub accept: SyncSender<bool>,
}

struct Job {
    generation: u64,
    epoch: u64,
    deadline: Instant,
    operation: Operation,
}

// The factory, not the backend or its connection, crosses the thread seam.
trait Backend {
    type Prepared;
    fn prepare(&mut self, operation: Operation) -> anyhow::Result<(Self::Prepared, Payload)>;
    fn commit(&mut self, prepared: Self::Prepared);
    fn disconnect(&mut self);
}

pub(super) struct Worker {
    jobs: SyncSender<Job>,
    pub replies: Receiver<Reply>,
    epoch: Arc<AtomicU64>,
}

impl Worker {
    pub fn new() -> anyhow::Result<Self> {
        Self::spawn(Real::default)
    }

    fn spawn<B: Backend + 'static>(factory: impl FnOnce() -> B + Send + 'static) -> anyhow::Result<Self> {
        let (jobs, requests) = mpsc::sync_channel::<Job>(1);
        let (results, replies) = mpsc::sync_channel(1);
        let epoch = Arc::new(AtomicU64::new(0));
        let signal = epoch.clone();
        // Intentionally do not join: an OS call may never return. No replacement
        // worker is spawned after timeout; connections are never dropped by the UI.
        std::thread::Builder::new().name("midi-backend".into()).spawn(move || {
            let mut backend = factory();
            let mut observed = 0;
            loop {
                let current = signal.load(Ordering::SeqCst);
                if current != observed {
                    backend.disconnect();
                    observed = current;
                }
                let job = match requests.recv_timeout(WAKE) {
                    Ok(job) => job,
                    Err(mpsc::RecvTimeoutError::Timeout) => continue,
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                };
                let current = signal.load(Ordering::SeqCst);
                if current != observed {
                    backend.disconnect();
                    observed = current;
                }
                if job.epoch != current || Instant::now() >= job.deadline {
                    continue;
                }
                let prepared = backend.prepare(job.operation);
                let (decision, accepted) = mpsc::sync_channel(1);
                let (candidate, result) = match prepared {
                    Ok((candidate, payload)) => (Some(candidate), Ok(payload)),
                    Err(error) => (None, Err(format!("{error:#}"))),
                };
                if results
                    .try_send(Reply {
                        generation: job.generation,
                        result,
                        accept: decision,
                    })
                    .is_err()
                {
                    break;
                }
                // The old connection/list stays usable until the UI accepts the
                // result. Timeout, disconnect and stale results never commit it.
                if accepted.recv().unwrap_or(false) && job.epoch == signal.load(Ordering::SeqCst) {
                    if let Some(candidate) = candidate {
                        backend.commit(candidate);
                    }
                }
            }
        })?;
        Ok(Self { jobs, replies, epoch })
    }

    pub fn request(&self, generation: u64, operation: Operation, deadline: Instant) -> anyhow::Result<()> {
        self.jobs
            .try_send(Job {
                generation,
                epoch: self.epoch.load(Ordering::SeqCst),
                deadline,
                operation,
            })
            .map_err(|_| anyhow::anyhow!("Backend busy or unavailable"))
    }

    pub fn disconnect(&self) {
        self.epoch.fetch_add(1, Ordering::SeqCst);
    }
}

#[derive(Default)]
struct Real {
    listed: Vec<(midir::MidiInputPort, String)>,
    active: Option<midir::MidiInputPort>,
    connection: Option<midir::MidiInputConnection<InputSender>>,
}

enum Prepared {
    List(Vec<(midir::MidiInputPort, String)>),
    Connected(midir::MidiInputPort, midir::MidiInputConnection<InputSender>),
}

impl Backend for Real {
    type Prepared = Prepared;
    fn prepare(&mut self, operation: Operation) -> anyhow::Result<(Prepared, Payload)> {
        let mut input = midir::MidiInput::new("track-rs input")?;
        input.ignore(midir::Ignore::None);
        let (available, browser) =
            discover_ports(input.ports(), |port| Ok(input.port_name(port)?), self.active.as_ref())?;
        match operation {
            Operation::List => Ok((Prepared::List(available), Payload::List(browser))),
            Operation::Connect(index) => {
                let (port, name) = select_port(&self.listed, &available, index)?;
                let (sender, queue) = InputQueue::new(Instant::now());
                let connection = input
                    .connect(port, "track-rs notes", |_, bytes, sender| sender.receive(bytes), sender)
                    .map_err(|error| anyhow::anyhow!("{error}"))?;
                Ok((
                    Prepared::Connected(port.clone(), connection),
                    Payload::Connected(queue, display_name(name)),
                ))
            }
        }
    }
    fn commit(&mut self, prepared: Prepared) {
        match prepared {
            Prepared::List(listed) => self.listed = listed,
            Prepared::Connected(port, connection) => {
                self.active = Some(port);
                self.connection = Some(connection);
            }
        }
    }
    fn disconnect(&mut self) {
        self.connection = None;
        self.active = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::midi::{InputBatch, Session};
    use std::sync::mpsc::Sender;

    const TEST_WAIT: Duration = Duration::from_secs(3);
    struct Fake {
        // Prove backend/connection ownership does not require Send.
        _thread_local: std::rc::Rc<()>,
        entered: Sender<()>,
        release: Receiver<bool>,
        committed: Sender<()>,
        stopped: Sender<()>,
        connection: Option<InputSender>,
    }
    impl Backend for Fake {
        type Prepared = Option<InputSender>;
        fn prepare(&mut self, operation: Operation) -> anyhow::Result<(Self::Prepared, Payload)> {
            self.entered.send(()).unwrap();
            anyhow::ensure!(self.release.recv_timeout(TEST_WAIT)?, "fake connection failed");
            Ok(match operation {
                Operation::List => (None, Payload::List(DeviceBrowser::default())),
                Operation::Connect(_) => {
                    let (sender, queue) = InputQueue::new(Instant::now());
                    (Some(sender), Payload::Connected(queue, "Fake".into()))
                }
            })
        }
        fn commit(&mut self, prepared: Self::Prepared) {
            if prepared.is_some() {
                self.connection = prepared;
            }
            self.committed.send(()).unwrap();
        }
        fn disconnect(&mut self) {
            self.connection = None;
        }
    }
    impl Drop for Fake {
        fn drop(&mut self) {
            let _ = self.stopped.send(());
        }
    }
    struct Harness {
        session: Session,
        entered: Receiver<()>,
        release: Sender<bool>,
        committed: Receiver<()>,
        stopped: Receiver<()>,
    }
    impl Harness {
        fn new() -> Self {
            let (start, entered) = mpsc::channel();
            let (release, gate) = mpsc::channel();
            let (commit, committed) = mpsc::channel();
            let (stop, stopped) = mpsc::channel();
            let worker = Worker::spawn(move || Fake {
                _thread_local: std::rc::Rc::new(()),
                entered: start,
                release: gate,
                committed: commit,
                stopped: stop,
                connection: None,
            })
            .unwrap();
            Self {
                session: Session {
                    worker: Some(worker),
                    ..Session::default()
                },
                entered,
                release,
                committed,
                stopped,
            }
        }
        fn start(&mut self, operation: Operation) {
            self.session.request_backend(operation).unwrap();
            self.entered.recv_timeout(TEST_WAIT).unwrap();
        }
        fn finish(&mut self, app: &mut crate::state::App) {
            let deadline = Instant::now() + TEST_WAIT;
            while self.session.pending_backend.is_some() {
                assert!(Instant::now() < deadline, "worker result did not arrive");
                self.session
                    .poll_backend(app, &mut crate::midi::tests::Silent, Instant::now());
                std::thread::yield_now();
            }
        }
        fn stop(self) {
            drop(self.session);
            self.stopped.recv_timeout(TEST_WAIT).unwrap();
        }
    }

    #[test]
    fn blocked_backend_keeps_input_usable_rejects_busy_and_times_out_once() {
        let mut h = Harness::new();
        let now = Instant::now();
        let (sender, queue) = InputQueue::new(now);
        h.session.queue = Some(queue);
        h.start(Operation::Connect(0));
        for _ in 0..100 {
            assert!(h
                .session
                .request_backend(Operation::List)
                .unwrap_err()
                .to_string()
                .contains("busy"));
        }
        sender.receive_at(&[0x90, 60, 100], now);
        assert!(matches!(h.session.queue.as_mut().unwrap().drain(), InputBatch::Notes(notes) if notes.len() == 1));
        let mut app = crate::state::App::default();
        let deadline = h.session.pending_backend.as_ref().unwrap().deadline;
        h.session
            .poll_backend(&mut app, &mut crate::midi::tests::Silent, deadline);
        assert!(app.status.contains("timed out"));
        assert!(h.session.queue.is_some());
        for _ in 0..100 {
            assert!(h.session.request_backend(Operation::List).is_err());
        }
        h.release.send(true).unwrap();
        let reply = h
            .session
            .worker
            .as_ref()
            .unwrap()
            .replies
            .recv_timeout(TEST_WAIT)
            .unwrap();
        reply.accept.send(false).unwrap();
        assert!(h.committed.try_recv().is_err());
        h.stop();
    }

    #[test]
    fn failed_candidate_preserves_old_input_and_success_requires_acceptance() {
        let mut h = Harness::new();
        let mut app = crate::state::App::default();
        let (old, queue) = InputQueue::new(Instant::now());
        h.session.queue = Some(queue);
        h.start(Operation::Connect(0));
        h.release.send(false).unwrap();
        h.finish(&mut app);
        assert!(app.status.contains("fake connection failed"));
        old.receive(&[0x90, 60, 1]);
        assert!(matches!(h.session.queue.as_mut().unwrap().drain(), InputBatch::Notes(notes) if notes.len() == 1));
        h.start(Operation::Connect(0));
        h.release.send(true).unwrap();
        h.finish(&mut app);
        h.committed.recv_timeout(TEST_WAIT).unwrap();
        assert!(app.status.contains("connected: Fake"));
        old.receive(&[0x90, 61, 1]);
        assert!(matches!(h.session.queue.as_mut().unwrap().drain(), InputBatch::Notes(notes) if notes.is_empty()));
        h.stop();
    }

    #[test]
    fn disconnect_during_blocked_connect_cannot_revive_input() {
        let mut h = Harness::new();
        let (old, queue) = InputQueue::new(Instant::now());
        h.session.queue = Some(queue);
        h.start(Operation::Connect(0));
        let mut app = crate::state::App::default();
        app.recording.armed = true;
        h.session.disconnect(&mut app, &mut crate::midi::tests::Silent);
        old.receive(&[0x90, 60, 1]);
        assert!(h.session.queue.is_none());
        assert!(!app.recording.armed);
        h.release.send(true).unwrap();
        let reply = h
            .session
            .worker
            .as_ref()
            .unwrap()
            .replies
            .recv_timeout(TEST_WAIT)
            .unwrap();
        // Even an erroneous acceptance cannot bypass the disconnect epoch.
        reply.accept.send(true).unwrap();
        h.start(Operation::List);
        assert!(h.committed.try_recv().is_err());
        h.release.send(true).unwrap();
        h.finish(&mut app);
        h.committed.recv_timeout(TEST_WAIT).unwrap();
        assert!(h.session.queue.is_none());
        h.stop();
    }

    #[test]
    fn escaped_discovery_does_not_reopen_browser_or_replace_snapshot() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let mut h = Harness::new();
        h.start(Operation::List);
        let mut app = crate::state::App::default();
        let now = Instant::now();
        h.session.terminal_key(
            &mut app,
            &mut crate::midi::tests::Silent,
            &mut crate::transport::Transport::default(),
            KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
            now,
            now,
        );
        h.release.send(true).unwrap();
        h.finish(&mut app);
        assert!(app.midi_browser.is_none());
        h.start(Operation::List);
        assert!(h.committed.try_recv().is_err());
        h.release.send(true).unwrap();
        h.finish(&mut app);
        h.committed.recv_timeout(TEST_WAIT).unwrap();
        assert!(app.midi_browser.is_some());
        h.stop();
    }

    #[test]
    fn stale_connect_reply_cannot_replace_a_newer_request() {
        let mut h = Harness::new();
        h.start(Operation::Connect(0));
        let mut app = crate::state::App::default();
        h.session.disconnect(&mut app, &mut crate::midi::tests::Silent);
        h.session.request_backend(Operation::List).unwrap();
        h.release.send(true).unwrap();
        // The main thread must discard the stale result before the worker can
        // begin the newer request. Its acceptance channel prevents replacement.
        let limit = Instant::now() + TEST_WAIT;
        loop {
            h.session
                .poll_backend(&mut app, &mut crate::midi::tests::Silent, Instant::now());
            if h.entered.try_recv().is_ok() {
                break;
            }
            assert!(Instant::now() < limit);
            std::thread::yield_now();
        }
        assert!(h.session.queue.is_none());
        assert!(h.committed.try_recv().is_err());
        assert!(h.session.pending_backend.is_some());
        h.release.send(true).unwrap();
        h.finish(&mut app);
        h.committed.recv_timeout(TEST_WAIT).unwrap();
        assert!(h.session.queue.is_none());
        assert!(app.midi_browser.is_some());
        h.stop();
    }

    #[test]
    fn request_channel_has_exactly_one_waiting_slot() {
        let mut h = Harness::new();
        h.start(Operation::Connect(0));
        let worker = h.session.worker.as_ref().unwrap();
        worker.request(2, Operation::List, Instant::now() + TEST_WAIT).unwrap();
        assert!(worker.request(3, Operation::List, Instant::now() + TEST_WAIT).is_err());
        h.release.send(true).unwrap();
        worker
            .replies
            .recv_timeout(TEST_WAIT)
            .unwrap()
            .accept
            .send(false)
            .unwrap();
        h.entered.recv_timeout(TEST_WAIT).unwrap();
        h.release.send(true).unwrap();
        worker
            .replies
            .recv_timeout(TEST_WAIT)
            .unwrap()
            .accept
            .send(false)
            .unwrap();
        h.stop();
    }

    #[test]
    fn shutdown_does_not_wait_for_blocked_backend() {
        let mut h = Harness::new();
        h.start(Operation::Connect(0));
        drop(h.session);
        // Releasing only after dropping proves shutdown doesn't join the worker.
        h.release.send(true).unwrap();
        h.stopped.recv_timeout(TEST_WAIT).unwrap();
        assert!(h.committed.try_recv().is_err());
    }
}
