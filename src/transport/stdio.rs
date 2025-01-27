use super::{JsonRpcMessage, Transport};
use crate::McpError;
use std::io::{self, Read};
use std::io::{BufRead, Write};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};

pub struct TimeoutBufReader<R> {
    inner: io::BufReader<R>,
}

impl<R: Read> TimeoutBufReader<R> {
    pub fn new(inner: R) -> Self {
        Self {
            inner: io::BufReader::new(inner),
        }
    }

    pub fn read_line(&mut self, buf: &mut String) -> io::Result<usize> {
        self.inner.read_line(buf)
    }
}

/// ClientStdioTransport launches a child process and communicates with it via stdio
pub struct ClientStdioTransport {
    stdin: Arc<Mutex<Option<std::process::ChildStdin>>>,
    stdout: Arc<Mutex<Option<TimeoutBufReader<std::process::ChildStdout>>>>,
    child: Arc<Mutex<Option<Child>>>,
    program: String,
    args: Vec<String>,
}

impl ClientStdioTransport {
    pub fn new(program: &str, args: &[&str]) -> Result<Self, McpError> {
        Ok(ClientStdioTransport {
            stdin: Arc::new(Mutex::new(None)),
            stdout: Arc::new(Mutex::new(None)),
            child: Arc::new(Mutex::new(None)),
            program: program.to_string(),
            args: args.iter().map(|&s| s.to_string()).collect(),
        })
    }
}

impl Transport for ClientStdioTransport {
    fn send(&self, message: &JsonRpcMessage) -> Result<(), McpError> {
        let mut stdin_guard = self.stdin.lock().unwrap();
        let stdin = stdin_guard
            .as_mut()
            .ok_or_else(|| McpError::TransportNotOpen)?;

        let serialized = serde_json::to_string(message)?;
        stdin.write_all(serialized.as_bytes())?;
        stdin.write_all(b"\n")?;
        stdin.flush()?;

        Ok(())
    }

    fn receive(&self) -> Result<JsonRpcMessage, McpError> {
        let mut stdout_guard = self.stdout.lock().unwrap();
        let stdout = stdout_guard
            .as_mut()
            .ok_or_else(|| McpError::TransportNotOpen)?;

        let mut line = String::new();
        stdout.read_line(&mut line)?;

        let message: JsonRpcMessage = serde_json::from_str(&line)?;
        Ok(message)
    }

    fn open(&self) -> Result<(), McpError> {
        let mut child = Command::new(&self.program)
            .args(&self.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| McpError::StdinNotAvailable)?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| McpError::StdoutNotAvailable)?;

        *self.stdin.lock().unwrap() = Some(stdin);
        *self.stdout.lock().unwrap() = Some(TimeoutBufReader::new(stdout));
        *self.child.lock().unwrap() = Some(child);

        Ok(())
    }

    fn close(&self) -> Result<(), McpError> {
        if let Some(mut child) = self.child.lock().unwrap().take() {
            let _ = child.kill(); // Kill child process
            let _ = child.wait(); // Wait for process cleanup
        }

        // Drop stdin and stdout to unblock any pending operations
        *self.stdin.lock().unwrap() = None;
        *self.stdout.lock().unwrap() = None;

        Ok(())
    }
}
