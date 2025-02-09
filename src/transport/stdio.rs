use super::{JsonRpcMessage, Transport};
use crate::McpError;
use std::io::{self, Read};
use std::io::{BufRead, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::{Arc, Mutex};
use tracing::debug;

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
pub struct StdioTransport {
    stdin: Arc<Mutex<Option<ChildStdin>>>,
    stdout: Arc<Mutex<Option<TimeoutBufReader<ChildStdout>>>>,
    child: Arc<Mutex<Option<Child>>>,
    program: String,
    args: Vec<String>,
}

impl StdioTransport {
    pub fn new(program: &str, args: &[&str]) -> Result<Self, McpError> {
        println!("Creating StdioTransport for {} with args: {:?}", program, args);
        Ok(StdioTransport {
            stdin: Arc::new(Mutex::new(None)),
            stdout: Arc::new(Mutex::new(None)),
            child: Arc::new(Mutex::new(None)),
            program: program.to_string(),
            args: args.iter().map(|&s| s.to_string()).collect(),
        })
    }
}

impl Transport for StdioTransport {
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
        debug!("stdio: waiting on messge");
        stdout.read_line(&mut line)?;
        debug!("stdio: Received message: {:?}", line);

        let message: JsonRpcMessage = serde_json::from_str(&line)?;
        Ok(message)
    }

    fn open(&self) -> Result<(), McpError> {
        println!("StdioTransport: Opening transport");
        let mut child = Command::new(&self.program)
            .args(&self.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()?;

        let pid = child.id();
        println!("StdioTransport: Started child process with PID {}", pid);

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

        println!("StdioTransport: Transport opened successfully");
        Ok(())
    }

    fn close(&self) -> Result<(), McpError> {
        println!("StdioTransport: Starting close");
        if let Some(mut child) = self.child.lock().unwrap().take() {
            let pid = child.id();
            println!("StdioTransport: Killing child process {}", pid);
            let _ = child.kill();
            println!("StdioTransport: Waiting for child process {}", pid);
            let _ = child.wait();
            println!("StdioTransport: Child process {} terminated", pid);
        }

        println!("StdioTransport: Dropping stdin/stdout");
        // Drop stdin and stdout to unblock any pending operations
        *self.stdin.lock().unwrap() = None;
        *self.stdout.lock().unwrap() = None;

        println!("StdioTransport: Close completed");
        Ok(())
    }
}
