//! Minimal OpenSSH askpass endpoint; this module never initializes Tauri, logging or a database.
//! The loopback bearer capability is single-use and expires with the probe. The password is
//! never an environment variable, process argument or file. Only OpenSSH's askpass stdout
//! pipe receives it. This does not isolate mutually untrusted processes of the same OS user.
use super::super::credentials::SecretString;
use std::io::{self, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, TcpListener, TcpStream};
use std::process::Command;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::{Duration, Instant};
use zeroize::Zeroizing;

const MARKER: &str = "CC_SWITCH_VPS_ASKPASS";
const ENDPOINT: &str = "CC_SWITCH_VPS_ASKPASS_ENDPOINT";
const TOKEN: &str = "CC_SWITCH_VPS_ASKPASS_TOKEN";
const TOKEN_LENGTH: usize = 64;
const MAX_PASSWORD: usize = 4096;

pub(super) struct Broker {
    listener: TcpListener,
    token: String,
    password: Option<Arc<SecretString>>,
}

impl Broker {
    pub(super) fn new(password: Arc<SecretString>) -> io::Result<Self> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
        listener.set_nonblocking(true)?;
        Ok(Self {
            listener,
            token: format!(
                "{}{}",
                uuid::Uuid::new_v4().simple(),
                uuid::Uuid::new_v4().simple()
            ),
            password: Some(password),
        })
    }

    pub(super) fn configure(&self, command: &mut Command) -> io::Result<()> {
        command
            .env("SSH_ASKPASS", std::env::current_exe()?)
            .env("SSH_ASKPASS_REQUIRE", "force")
            .env("DISPLAY", "cc-switch-askpass")
            .env(MARKER, "1")
            .env(ENDPOINT, self.listener.local_addr()?.to_string())
            .env(TOKEN, &self.token);
        Ok(())
    }

    /// Poll from the runner, not a detached worker: cancellation/drop leaves no listening task.
    pub(super) fn poll(&mut self, cancelled: &AtomicBool, deadline: Instant) -> io::Result<()> {
        if cancelled.load(Ordering::SeqCst) || Instant::now() >= deadline {
            self.password.take();
            return Ok(());
        }
        if self.password.is_none() {
            return Ok(());
        }
        let (mut connection, peer) = match self.listener.accept() {
            Ok(connection) => connection,
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(()),
            Err(error) => return Err(error),
        };
        if !peer.ip().is_loopback() {
            return Ok(());
        }
        // Accepted sockets can inherit nonblocking mode on Windows/BSD. Use timed
        // blocking I/O so a fragmented token does not fail immediately.
        connection.set_nonblocking(false)?;
        connection.set_read_timeout(Some(Duration::from_millis(20)))?;
        connection.set_write_timeout(Some(Duration::from_millis(20)))?;
        let mut token = [0; TOKEN_LENGTH];
        if connection.read_exact(&mut token).is_err()
            || token
                .iter()
                .zip(self.token.as_bytes())
                .fold(0u8, |diff, (a, b)| diff | (a ^ b))
                != 0
            || cancelled.load(Ordering::SeqCst)
            || Instant::now() >= deadline
        {
            return Ok(());
        }
        let password = self.password.take().expect("checked single-use password");
        let bytes = password.expose().as_bytes();
        // Consume even if the recipient disconnects: never reuse a password capability.
        let _ = connection
            .write_all(&(bytes.len() as u32).to_be_bytes())
            .and_then(|_| connection.write_all(bytes));
        Ok(())
    }
}

pub(super) fn clear_environment(command: &mut Command) {
    command
        .env_remove(MARKER)
        .env_remove(ENDPOINT)
        .env_remove(TOKEN);
}

fn request_password(endpoint: &str, token: &str, output: &mut impl Write) -> io::Result<()> {
    let endpoint: SocketAddrV4 = endpoint.parse().map_err(|_| io::ErrorKind::InvalidInput)?;
    if *endpoint.ip() != Ipv4Addr::LOCALHOST
        || endpoint.port() == 0
        || token.len() != TOKEN_LENGTH
        || !token.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    let mut connection =
        TcpStream::connect_timeout(&SocketAddr::V4(endpoint), Duration::from_secs(2))?;
    connection.set_read_timeout(Some(Duration::from_secs(3)))?;
    connection.set_write_timeout(Some(Duration::from_secs(1)))?;
    connection.write_all(token.as_bytes())?;
    let mut size = [0; 4];
    connection.read_exact(&mut size)?;
    let size = u32::from_be_bytes(size) as usize;
    if size == 0 || size > MAX_PASSWORD {
        return Err(io::ErrorKind::InvalidData.into());
    }
    let mut password = Zeroizing::new(vec![0; size]);
    connection.read_exact(&mut password)?;
    if password
        .iter()
        .any(|byte| matches!(*byte, 0 | b'\n' | b'\r'))
    {
        return Err(io::ErrorKind::InvalidData.into());
    }
    output.write_all(&password)?;
    output.write_all(b"\n")?;
    output.flush()
}

/// MUST be the first operation in main. Even a malformed helper request exits without startup.
pub fn dispatch() -> Option<i32> {
    std::env::var_os(MARKER)?;
    let result = (|| {
        if std::env::var(MARKER).ok().as_deref() != Some("1")
            || std::env::var("SSH_ASKPASS_PROMPT").is_ok_and(|value| value == "confirm")
        {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        let mut args = std::env::args_os().skip(1);
        let prompt = args.next().ok_or(io::ErrorKind::InvalidInput)?;
        let prompt = prompt.to_str().ok_or(io::ErrorKind::InvalidInput)?;
        if args.next().is_some()
            || prompt.len() > 1024
            || !prompt.to_ascii_lowercase().contains("password")
            || !prompt.trim_end().ends_with(':')
        {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        let endpoint = std::env::var(ENDPOINT).map_err(|_| io::ErrorKind::InvalidInput)?;
        let token = std::env::var(TOKEN).map_err(|_| io::ErrorKind::InvalidInput)?;
        request_password(&endpoint, &token, &mut io::stdout().lock())
    })();
    // No stderr/logging: helper diagnostics must never include secret data or askpass prompts.
    Some(if result.is_ok() { 0 } else { 1 })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helper_branch_is_before_all_application_startup() {
        let source = include_str!("../../../main.rs");
        let main = source.split("fn main() {").nth(1).unwrap();
        assert!(main
            .trim_start()
            .starts_with("if let Some(code) = cc_switch_lib::vps::ssh::askpass::dispatch()"));
    }

    #[test]
    #[serial_test::serial]
    fn malformed_helper_dispatch_exits_without_initializing_application() {
        struct Restore(Option<std::ffi::OsString>);
        impl Drop for Restore {
            fn drop(&mut self) {
                match &self.0 {
                    Some(value) => std::env::set_var(MARKER, value),
                    None => std::env::remove_var(MARKER),
                }
            }
        }
        let _restore = Restore(std::env::var_os(MARKER));
        std::env::remove_var(MARKER);
        assert_eq!(dispatch(), None);
        std::env::set_var(MARKER, "malformed");
        assert_eq!(dispatch(), Some(1));
    }

    #[test]
    fn broker_delivers_once_without_password_in_command_or_environment() {
        let secret = " password with spaces ";
        let mut broker = Broker::new(Arc::new(SecretString::new(secret.into()))).unwrap();
        let mut command = Command::new("ssh");
        broker.configure(&mut command).unwrap();
        assert!(!format!("{command:?}").contains(secret));
        assert!(!format!("{:?}", broker.password).contains(secret));
        let endpoint = broker.listener.local_addr().unwrap().to_string();
        let token = broker.token.clone();
        let client = std::thread::spawn(move || {
            let mut output = Vec::new();
            request_password(&endpoint, &token, &mut output).unwrap();
            output
        });
        let cancelled = AtomicBool::new(false);
        let deadline = Instant::now() + Duration::from_secs(2);
        while !client.is_finished() && Instant::now() < deadline {
            broker.poll(&cancelled, deadline).unwrap();
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(client.join().unwrap(), format!("{secret}\n").as_bytes());
        assert!(broker.password.is_none());
    }

    #[test]
    fn broker_waits_for_a_fragmented_capability_before_delivering_password() {
        let secret = "local test password";
        let mut broker = Broker::new(Arc::new(SecretString::new(secret.into()))).unwrap();
        let mut connection = TcpStream::connect(broker.listener.local_addr().unwrap()).unwrap();
        connection
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        let token = broker.token.clone();
        connection
            .write_all(&token.as_bytes()[..TOKEN_LENGTH / 2])
            .unwrap();
        let client = std::thread::spawn(move || -> io::Result<Vec<u8>> {
            std::thread::sleep(Duration::from_millis(5));
            connection.write_all(&token.as_bytes()[TOKEN_LENGTH / 2..])?;
            let mut size = [0; 4];
            connection.read_exact(&mut size)?;
            let mut password = vec![0; u32::from_be_bytes(size) as usize];
            connection.read_exact(&mut password)?;
            Ok(password)
        });
        broker
            .poll(
                &AtomicBool::new(false),
                Instant::now() + Duration::from_secs(1),
            )
            .unwrap();
        let password = client.join().unwrap().expect("valid fragmented capability");
        assert_eq!(password, secret.as_bytes());
        assert!(broker.password.is_none());
    }

    #[test]
    fn cancellation_and_timeout_revoke_password_and_drop_closes_channel() {
        for cancelled in [false, true] {
            let password = Arc::new(SecretString::new("test only".into()));
            let mut broker = Broker::new(password.clone()).unwrap();
            let address = broker.listener.local_addr().unwrap();
            broker
                .poll(&AtomicBool::new(cancelled), Instant::now())
                .unwrap();
            assert!(broker.password.is_none());
            assert_eq!(Arc::strong_count(&password), 1);
            drop(broker);
            assert!(TcpStream::connect_timeout(&address, Duration::from_millis(100)).is_err());
        }
    }

    #[test]
    fn invalid_capability_does_not_release_or_consume_password() {
        let mut broker = Broker::new(Arc::new(SecretString::new("test only".into()))).unwrap();
        let mut connection = TcpStream::connect(broker.listener.local_addr().unwrap()).unwrap();
        connection.write_all(&[b'0'; TOKEN_LENGTH]).unwrap();
        broker
            .poll(
                &AtomicBool::new(false),
                Instant::now() + Duration::from_secs(1),
            )
            .unwrap();
        let mut output = Vec::new();
        connection.read_to_end(&mut output).unwrap();
        assert!(output.is_empty());
        assert!(broker.password.is_some());
        assert!(request_password("192.0.2.1:1", &broker.token, &mut Vec::new()).is_err());
    }
}
