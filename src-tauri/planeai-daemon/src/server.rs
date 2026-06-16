use std::io::Read;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;
use tokio::sync::Notify;

use crate::protocol::{Request, Response};
use crate::session::SessionManager;

const IDLE_TIMEOUT: Duration = Duration::from_secs(30);

pub async fn run(sock_path: PathBuf) {
    let listener = UnixListener::bind(&sock_path).expect("failed to bind daemon socket");
    tracing::info!("listening on {:?}", sock_path);

    let manager = Arc::new(SessionManager::new());
    let client_count = Arc::new(AtomicUsize::new(0));
    let activity = Arc::new(Notify::new());

    // Idle shutdown task
    let mgr = manager.clone();
    let cc = client_count.clone();
    let act = activity.clone();
    let shutdown = Arc::new(Notify::new());
    let shut = shutdown.clone();
    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = act.notified() => continue,
                _ = tokio::time::sleep(IDLE_TIMEOUT) => {
                    if cc.load(Ordering::Relaxed) == 0 && !mgr.has_live_sessions().await {
                        tracing::info!("idle timeout reached, shutting down");
                        shut.notify_one();
                        return;
                    }
                }
            }
        }
    });

    loop {
        tokio::select! {
            _ = shutdown.notified() => break,
            result = listener.accept() => {
                match result {
                    Ok((stream, _)) => {
                        client_count.fetch_add(1, Ordering::Relaxed);
                        activity.notify_one();
                        let mgr = manager.clone();
                        let cc = client_count.clone();
                        let act = activity.clone();
                        tokio::spawn(async move {
                            handle_client(stream, mgr, cc.clone(), act.clone()).await;
                            cc.fetch_sub(1, Ordering::Relaxed);
                            act.notify_one();
                        });
                    }
                    Err(e) => {
                        tracing::error!("accept error: {e}");
                    }
                }
            }
        }
    }
}

async fn handle_client(
    stream: tokio::net::UnixStream,
    manager: Arc<SessionManager>,
    _client_count: Arc<AtomicUsize>,
    activity: Arc<Notify>,
) {
    let (reader, mut writer) = stream.into_split();
    let mut lines = BufReader::new(reader).lines();

    while let Ok(Some(line)) = lines.next_line().await {
        activity.notify_one();
        let req: Request = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                let resp = Response::Error {
                    message: format!("invalid request: {e}"),
                };
                let _ = send_response(&mut writer, &resp).await;
                continue;
            }
        };

        let resp = handle_request(&req, &manager, &mut writer).await;
        let _ = send_response(&mut writer, &resp).await;
    }
}

async fn handle_request(
    req: &Request,
    manager: &SessionManager,
    writer: &mut tokio::net::unix::OwnedWriteHalf,
) -> Response {
    match req {
        Request::CreateSession {
            session_id,
            command,
            args,
            cwd,
            env,
        } => match manager
            .create(session_id.clone(), command, args, cwd, env)
            .await
        {
            Ok(()) => Response::Ok { message: None },
            Err(e) => Response::Error { message: e },
        },

        Request::Attach { session_id } => {
            let session = match manager.get(session_id).await {
                Some(s) => s,
                None => {
                    return Response::Error {
                        message: "session not found".to_string(),
                    }
                }
            };

            // Send scrollback replay first
            let scrollback = session.scrollback.lock().await.contents();
            if !scrollback.is_empty() {
                let _ = send_data_frame(writer, &scrollback).await;
            }

            // Stream live output until session dies or client disconnects
            let mut rx = session.output_tx.subscribe();
            loop {
                tokio::select! {
                    result = rx.recv() => {
                        match result {
                            Ok(data) => {
                                if send_data_frame(writer, &data).await.is_err() {
                                    break;
                                }
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                                break;
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                        }
                    }
                    _ = check_alive(&session) => {
                        // Session exited, notify and break
                        let exit_resp = Response::SessionExited {
                            session_id: session_id.clone(),
                        };
                        let _ = send_response(writer, &exit_resp).await;
                        break;
                    }
                }
            }

            Response::Ok {
                message: Some("detached".to_string()),
            }
        }

        Request::Detach => Response::Ok { message: None },

        Request::Write { session_id, data } => {
            let session = match manager.get(session_id).await {
                Some(s) => s,
                None => {
                    return Response::Error {
                        message: "session not found".to_string(),
                    }
                }
            };
            let bytes = base64_decode(data);
            match session.write_input(&bytes).await {
                Ok(()) => Response::Ok { message: None },
                Err(e) => Response::Error { message: e },
            }
        }

        Request::Resize {
            session_id,
            rows,
            cols,
        } => {
            let session = match manager.get(session_id).await {
                Some(s) => s,
                None => {
                    return Response::Error {
                        message: "session not found".to_string(),
                    }
                }
            };
            match session.resize(*rows, *cols).await {
                Ok(()) => Response::Ok { message: None },
                Err(e) => Response::Error { message: e },
            }
        }

        Request::Kill { session_id } => {
            let session = match manager.get(session_id).await {
                Some(s) => s,
                None => {
                    return Response::Error {
                        message: "session not found".to_string(),
                    }
                }
            };
            match session.kill().await {
                Ok(()) => {
                    manager.remove(session_id).await;
                    Response::Ok { message: None }
                }
                Err(e) => Response::Error { message: e },
            }
        }

        Request::List => {
            let sessions = manager.list().await;
            Response::SessionList { sessions }
        }

        Request::Ping => Response::Pong,
    }
}

async fn send_response(
    writer: &mut tokio::net::unix::OwnedWriteHalf,
    resp: &Response,
) -> Result<(), ()> {
    let mut json = serde_json::to_string(resp).unwrap_or_default();
    json.push('\n');
    writer.write_all(json.as_bytes()).await.map_err(|_| ())
}

/// Send a length-prefixed binary data frame: 4-byte BE length + raw bytes.
async fn send_data_frame(
    writer: &mut tokio::net::unix::OwnedWriteHalf,
    data: &[u8],
) -> Result<(), ()> {
    let len = (data.len() as u32).to_be_bytes();
    writer.write_all(&len).await.map_err(|_| ())?;
    writer.write_all(data).await.map_err(|_| ())
}

/// Waits until session is no longer alive.
async fn check_alive(session: &crate::session::Session) {
    loop {
        if !*session.alive.read().await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

fn base64_decode(s: &str) -> Vec<u8> {
    // Simple base64 decode without extra dependency
    use std::io::Read;
    let mut decoder = base64_reader(s.as_bytes());
    let mut out = Vec::new();
    let _ = decoder.read_to_end(&mut out);
    out
}

/// Minimal base64 decode (standard alphabet, no padding required).
fn base64_reader(input: &[u8]) -> impl Read + '_ {
    struct B64Reader<'a> {
        input: &'a [u8],
        pos: usize,
        buf: [u8; 3],
        buf_len: usize,
        buf_pos: usize,
    }

    impl<'a> Read for B64Reader<'a> {
        fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
            let mut written = 0;
            while written < out.len() {
                if self.buf_pos < self.buf_len {
                    out[written] = self.buf[self.buf_pos];
                    self.buf_pos += 1;
                    written += 1;
                    continue;
                }
                // Decode next 4 chars
                let mut sextet = [0u8; 4];
                let mut count = 0;
                while count < 4 && self.pos < self.input.len() {
                    let c = self.input[self.pos];
                    self.pos += 1;
                    if let Some(v) = decode_char(c) {
                        sextet[count] = v;
                        count += 1;
                    } else if c == b'=' {
                        sextet[count] = 0;
                        count += 1;
                    }
                }
                if count == 0 {
                    break;
                }
                self.buf[0] = (sextet[0] << 2) | (sextet[1] >> 4);
                self.buf[1] = (sextet[1] << 4) | (sextet[2] >> 2);
                self.buf[2] = (sextet[2] << 6) | sextet[3];
                self.buf_len = match count {
                    4 => 3,
                    3 => 2,
                    _ => 1,
                };
                self.buf_pos = 0;
            }
            Ok(written)
        }
    }

    fn decode_char(c: u8) -> Option<u8> {
        match c {
            b'A'..=b'Z' => Some(c - b'A'),
            b'a'..=b'z' => Some(c - b'a' + 26),
            b'0'..=b'9' => Some(c - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }

    B64Reader {
        input,
        pos: 0,
        buf: [0; 3],
        buf_len: 0,
        buf_pos: 0,
    }
}
