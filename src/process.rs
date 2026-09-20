use crate::protocol::{self, Account, Event};
use anyhow::{Context, Result, anyhow};
use std::{
    io::{self, BufRead, BufReader, Read, Write},
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex, mpsc},
    thread,
};

const MAX_LINE: usize = 2 * 1024 * 1024;

#[derive(Debug)]
pub enum Message {
    Progress(String),
    Result {
        ok: bool,
        cancelled: bool,
        message: String,
        accounts: Option<Vec<Account>>,
        error: Option<String>,
    },
    Failed(String),
}

pub struct Operation {
    input: Arc<Mutex<ChildInput>>,
    events: mpsc::Receiver<Message>,
    thread: Option<thread::JoinHandle<()>>,
}
struct ChildInput {
    child: Option<Child>,
    stdin: Option<std::process::ChildStdin>,
}

impl Operation {
    pub fn start(backend: PathBuf, action: &str, args: &[String]) -> Result<Self> {
        Self::start_with_environment(backend, action, args, &[])
    }

    fn start_with_environment(
        backend: PathBuf,
        action: &str,
        args: &[String],
        environment: &[(String, String)],
    ) -> Result<Self> {
        let required_capability = action.to_string();
        let mut command = Command::new(&backend);
        command.envs(environment.iter().map(|(key, value)| (key, value)));
        command
            .arg("frontend")
            .arg(action)
            .args(args)
            .arg("--contract-version")
            .arg("1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        std::os::windows::process::CommandExt::creation_flags(&mut command, 0x08000000);
        let mut child = command
            .spawn()
            .with_context(|| format!("start backend {}", backend.display()))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow!("backend stdin unavailable"))?;
        let stdout = match child.stdout.take() {
            Some(value) => value,
            None => {
                drop(stdin);
                let _ = child.wait();
                return Err(anyhow!("backend stdout unavailable"));
            }
        };
        let stderr = match child.stderr.take() {
            Some(value) => value,
            None => {
                drop(stdin);
                let _ = child.wait();
                return Err(anyhow!("backend stderr unavailable"));
            }
        };
        let (tx, rx) = mpsc::channel();
        let input = Arc::new(Mutex::new(ChildInput {
            child: Some(child),
            stdin: Some(stdin),
        }));
        let thread_input = input.clone();
        let thread = thread::spawn(move || {
            let stderr_thread = thread::spawn(move || {
                let mut reader = BufReader::new(stderr);
                let mut buffer = [0_u8; 8192];
                while reader.read(&mut buffer).unwrap_or(0) != 0 {}
            });
            let mut reader = BufReader::new(stdout);
            let mut terminal: Option<Message> = None;
            let mut saw_hello = false;
            loop {
                let bytes = match read_line_bounded(&mut reader, MAX_LINE) {
                    Ok(Some(bytes)) => bytes,
                    Ok(None) => break,
                    Err(_) => {
                        terminal = Some(Message::Failed("backend response exceeded limit".into()));
                        break;
                    }
                };
                let line = String::from_utf8_lossy(&bytes);
                match serde_json::from_str::<Event>(&line) {
                    Ok(Event::Hello {
                        version: 1,
                        capabilities,
                    }) if !saw_hello => {
                        if !capabilities
                            .iter()
                            .any(|value| value == &required_capability)
                            || !capabilities.iter().any(|value| value == "cancel")
                        {
                            terminal =
                                Some(Message::Failed("backend lacks status capability".into()));
                            break;
                        }
                        saw_hello = true;
                    }
                    Ok(Event::Hello { .. }) => {
                        terminal = Some(Message::Failed("invalid backend hello".into()));
                        break;
                    }
                    Ok(Event::ErrorResponse {
                        version: 1,
                        code,
                        message,
                    }) if saw_hello => {
                        terminal = Some(Message::Failed(format!("{code}: {message}")));
                        break;
                    }
                    Ok(Event::Progress {
                        version: 1,
                        message,
                        ..
                    }) if saw_hello => {
                        let _ = tx.send(Message::Progress(message));
                    }
                    Ok(Event::Result {
                        version: 1,
                        ok,
                        cancelled,
                        message,
                        accounts,
                        error,
                    }) if saw_hello => {
                        terminal = Some(Message::Result {
                            ok,
                            cancelled,
                            message,
                            accounts,
                            error: error.map(|value| format!("{}: {}", value.code, value.message)),
                        });
                        break;
                    }
                    Ok(_) => {
                        terminal = Some(Message::Failed(
                            "unsupported backend protocol version".into(),
                        ));
                        break;
                    }
                    Err(_) => {
                        terminal =
                            Some(Message::Failed("invalid backend protocol response".into()));
                        break;
                    }
                }
            }
            if let Ok(mut input) = thread_input.lock() {
                input.stdin.take();
            }
            let _ = io::copy(&mut reader, &mut io::sink());
            let child = thread_input
                .lock()
                .ok()
                .and_then(|mut input| input.child.take());
            let status = child.and_then(|mut child| child.wait().ok());
            let _ = stderr_thread.join();
            let succeeded = status
                .as_ref()
                .is_some_and(std::process::ExitStatus::success);
            if let Some(Message::Result { ok, .. }) = &terminal
                && *ok != succeeded
            {
                terminal = Some(Message::Failed(
                    "backend exit status disagreed with result".into(),
                ));
            }
            if terminal.is_none() {
                terminal = Some(Message::Failed(format!(
                    "backend exited ({})",
                    status
                        .map(|value| value.to_string())
                        .unwrap_or_else(|| "unknown".into())
                )));
            }
            if let Some(message) = terminal {
                let _ = tx.send(message);
            }
        });
        Ok(Self {
            input,
            events: rx,
            thread: Some(thread),
        })
    }
    pub fn cancel(&self) {
        if let Ok(mut input) = self.input.lock()
            && let Some(stdin) = input.stdin.as_mut()
        {
            let _ = writeln!(
                stdin,
                "{}",
                serde_json::to_string(&protocol::Cancel { cancel: true }).unwrap()
            );
            let _ = stdin.flush();
        }
    }
    pub fn try_event(&self) -> Option<Message> {
        self.events.try_recv().ok()
    }
}

fn read_line_bounded<R: BufRead>(reader: &mut R, limit: usize) -> io::Result<Option<Vec<u8>>> {
    let mut output = Vec::new();
    loop {
        let (take, complete) = {
            let chunk = reader.fill_buf()?;
            if chunk.is_empty() {
                return Ok((!output.is_empty()).then_some(output));
            }
            let take = chunk
                .iter()
                .position(|byte| *byte == b'\n')
                .map_or(chunk.len(), |index| index + 1);
            if output.len().saturating_add(take) > limit {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "line limit exceeded",
                ));
            }
            output.extend_from_slice(&chunk[..take]);
            (take, chunk[..take].contains(&b'\n'))
        };
        reader.consume(take);
        if complete {
            return Ok(Some(output));
        }
    }
}

impl Drop for Operation {
    fn drop(&mut self) {
        self.cancel();
        if let Ok(mut input) = self.input.lock() {
            input.stdin.take();
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Message, Operation, read_line_bounded};
    use std::io::{BufReader, Cursor};
    use std::time::{Duration, Instant};

    #[test]
    fn bounded_reader_rejects_oversized_lines() {
        let mut reader = Cursor::new(b"123456789\n".to_vec());
        assert!(read_line_bounded(&mut reader, 4).is_err());
    }

    #[test]
    fn bounded_reader_preserves_multiple_frames() {
        let mut reader = BufReader::with_capacity(4, Cursor::new(b"one\ntwo\n".to_vec()));
        assert_eq!(
            read_line_bounded(&mut reader, 16).unwrap(),
            Some(b"one\n".to_vec())
        );
        assert_eq!(
            read_line_bounded(&mut reader, 16).unwrap(),
            Some(b"two\n".to_vec())
        );
        assert_eq!(read_line_bounded(&mut reader, 16).unwrap(), None);
    }

    #[test]
    fn real_go_status_error_cancel_and_shutdown() {
        let Some(backend) = std::env::var_os("AIU_TEST_BACKEND") else {
            eprintln!("Set AIU_TEST_BACKEND to exercise the pinned Go subprocess (required in CI)");
            return;
        };
        let temporary = tempfile::tempdir().unwrap();
        let environment: Vec<_> = [
            ("AIU_CONFIG_DIR", "aiu"),
            ("CLAUDE_CONFIG_DIR", "claude"),
            ("CODEX_HOME", "codex"),
        ]
        .into_iter()
        .map(|(key, directory)| {
            (
                key.to_string(),
                temporary
                    .path()
                    .join(directory)
                    .to_string_lossy()
                    .into_owned(),
            )
        })
        .collect();
        let start = |action: &str, args: &[String]| {
            Operation::start_with_environment(backend.clone().into(), action, args, &environment)
                .unwrap()
        };
        let terminal = |operation: &Operation| {
            loop {
                let event = operation
                    .events
                    .recv_timeout(Duration::from_secs(10))
                    .unwrap();
                if !matches!(event, Message::Progress(_)) {
                    break event;
                }
            }
        };
        let status = start("status", &[]);
        match terminal(&status) {
            Message::Result {
                ok: true,
                accounts: Some(accounts),
                ..
            } => assert!(accounts.is_empty()),
            other => panic!("real status failed: {other:?}"),
        }
        drop(status);
        let error = start("switch", &["claude:missing@example.test#".into()]);
        assert!(matches!(
            terminal(&error),
            Message::Result {
                ok: false,
                cancelled: false,
                error: Some(_),
                ..
            }
        ));
        drop(error);
        for explicit_cancel in [true, false] {
            let login = start("login", &["--no-open".into()]);
            loop {
                match login.events.recv_timeout(Duration::from_secs(10)).unwrap() {
                    Message::Progress(message) if message.contains("waiting for browser") => break,
                    Message::Progress(_) => {}
                    other => panic!("login never entered waiting state: {other:?}"),
                }
            }
            let before = Instant::now();
            if explicit_cancel {
                login.cancel();
                assert!(matches!(
                    terminal(&login),
                    Message::Result {
                        ok: false,
                        cancelled: true,
                        ..
                    }
                ));
            }
            // Dropping an active operation is the frontend exit path. It sends
            // cancellation, closes stdin, and joins the worker after child.wait.
            drop(login);
            assert!(before.elapsed() < Duration::from_secs(5));
        }
        assert!(!temporary.path().join("aiu/tokens.json").exists());
        assert!(!temporary.path().join("aiu/tokens.dpapi").exists());
    }
}
