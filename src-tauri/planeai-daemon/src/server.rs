use crate::protocol::{Request, Response, SessionInfoDto};
use crate::registry::SessionRegistry;
use crate::transport::{DaemonListener, DaemonStream};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{broadcast, Mutex, Notify};
use tokio::task::JoinSet;

const SHUTDOWN_GRACE: Duration = Duration::from_secs(30);

pub struct DaemonServer {
    registry: Arc<Mutex<SessionRegistry>>,
    buffer_capacity: usize,
    event_tx: broadcast::Sender<Response>,
    client_count: Arc<std::sync::atomic::AtomicUsize>,
    activity: Arc<Notify>,
}

impl DaemonServer {
    pub fn new(buffer_capacity: usize) -> Self {
        let (event_tx, _) = broadcast::channel(128);
        Self {
            registry: Arc::new(Mutex::new(SessionRegistry::new())),
            buffer_capacity,
            event_tx,
            client_count: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            activity: Arc::new(Notify::new()),
        }
    }

    pub async fn run(
        self: Arc<Self>,
        listener: DaemonListener,
        mut shutdown: tokio::sync::watch::Receiver<()>,
    ) {
        let mut tasks = JoinSet::new();

        // Spawn exit-event poller
        let server = Arc::clone(&self);
        tasks.spawn(async move { server.poll_exits().await });

        // Spawn shutdown timer
        let server = Arc::clone(&self);
        let mut shutdown_rx = shutdown.clone();
        tasks.spawn(async move { server.shutdown_timer(&mut shutdown_rx).await });

        loop {
            tokio::select! {
                result = listener.accept() => {
                    match result {
                        Ok(stream) => {
                            let server = Arc::clone(&self);
                            tasks.spawn(async move { server.handle_connection(stream).await });
                        }
                        Err(e) => {
                            tracing::error!("accept error: {e}");
                        }
                    }
                }
                _ = shutdown.changed() => break,
            }
        }

        tasks.shutdown().await;
    }

    async fn handle_connection(self: Arc<Self>, stream: DaemonStream) {
        use std::sync::atomic::Ordering;
        self.client_count.fetch_add(1, Ordering::SeqCst);
        self.activity.notify_one();

        let mut event_rx = self.event_tx.subscribe();
        let (reader, mut writer) = tokio::io::split(stream);
        let mut reader = BufReader::new(reader);

        let mut line_buf = String::new();
        loop {
            line_buf.clear();
            tokio::select! {
                result = reader.read_line(&mut line_buf) => {
                    match result {
                        Ok(0) | Err(_) => break,
                        Ok(_) => {
                            let resp = match serde_json::from_str::<Request>(line_buf.trim()) {
                                Ok(req) => self.handle_request(req).await,
                                Err(e) => Response::error(e.to_string()),
                            };
                            let mut out = serde_json::to_string(&resp).unwrap();
                            out.push('\n');
                            if writer.write_all(out.as_bytes()).await.is_err() {
                                break;
                            }
                        }
                    }
                }
                Ok(event) = event_rx.recv() => {
                    let mut out = serde_json::to_string(&event).unwrap();
                    out.push('\n');
                    if writer.write_all(out.as_bytes()).await.is_err() {
                        break;
                    }
                }
            }
        }

        self.client_count
            .fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
        self.activity.notify_one();
    }

    async fn handle_request(&self, req: Request) -> Response {
        let mut reg = self.registry.lock().await;
        match req {
            Request::Spawn {
                session_id,
                command,
                args,
                cwd,
                env,
            } => {
                let args_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
                match reg.spawn(
                    &session_id,
                    &command,
                    &args_refs,
                    cwd.as_deref(),
                    env.as_ref(),
                    self.buffer_capacity,
                ) {
                    Ok(()) => {
                        self.activity.notify_one();
                        Response::ok(Some(session_id))
                    }
                    Err(e) => Response::error(e.to_string()),
                }
            }
            Request::Kill { session_id } => match reg.kill(&session_id) {
                Ok(()) => Response::ok(Some(session_id)),
                Err(e) => Response::error(e.to_string()),
            },
            Request::Resize {
                session_id,
                cols,
                rows,
            } => match reg.get(&session_id) {
                Some(s) => match s.resize(cols, rows) {
                    Ok(()) => Response::ok(Some(session_id)),
                    Err(e) => Response::error(e.to_string()),
                },
                None => Response::error(format!("session not found: {session_id}")),
            },
            Request::List => {
                let sessions = reg
                    .list()
                    .into_iter()
                    .map(|s| SessionInfoDto {
                        session_id: s.session_id,
                        alive: s.alive,
                    })
                    .collect();
                Response::Sessions { sessions }
            }
            Request::Attach { session_id } => {
                if reg.get(&session_id).is_some() {
                    Response::ok(Some(session_id))
                } else {
                    Response::error(format!("session not found: {session_id}"))
                }
            }
            Request::Detach { session_id } => Response::ok(Some(session_id)),
        }
    }

    /// Periodically check for dead sessions and broadcast exit events.
    async fn poll_exits(self: Arc<Self>) {
        loop {
            tokio::time::sleep(Duration::from_millis(500)).await;
            let dead = self.registry.lock().await.remove_dead();
            for id in dead {
                let _ = self.event_tx.send(Response::event("exited", &id));
            }
        }
    }

    /// Shutdown timer: exits process when no clients and no sessions for SHUTDOWN_GRACE.
    async fn shutdown_timer(&self, shutdown_rx: &mut tokio::sync::watch::Receiver<()>) {
        use std::sync::atomic::Ordering;
        loop {
            // Wait until conditions are met
            loop {
                if self.client_count.load(Ordering::SeqCst) == 0
                    && self.registry.lock().await.is_empty()
                {
                    break;
                }
                self.activity.notified().await;
            }

            // Start countdown
            tokio::select! {
                _ = tokio::time::sleep(SHUTDOWN_GRACE) => {
                    // Recheck conditions
                    if self.client_count.load(Ordering::SeqCst) == 0
                        && self.registry.lock().await.is_empty()
                    {
                        tracing::info!("shutdown timer expired, exiting");
                        // Signal shutdown
                        std::process::exit(0);
                    }
                }
                _ = self.activity.notified() => {
                    // Activity occurred, restart loop
                    continue;
                }
                _ = shutdown_rx.changed() => return,
            }
        }
    }
}
