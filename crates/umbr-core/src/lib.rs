use crossterm::{
    event::{self, Event, KeyCode},
    terminal,
};
use std::io::{self};
use tokio::signal::unix::{SignalKind, signal};

use std::env;
use std::error::Error;
use std::io::Write;

use thiserror::Error;
use zbus::Result;
use zbus::{Connection, proxy, zvariant::OwnedObjectPath};

#[proxy(
    interface = "org.freedesktop.login1.Manager",
    default_service = "org.freedesktop.login1",
    default_path = "/org/freedesktop/login1"
)]
trait LoginManager {
    fn get_user(&self, uid: u32) -> Result<OwnedObjectPath>;

    #[zbus(name = "GetSessionByPID")]
    fn get_session_by_pid(&self, pid: u32) -> Result<OwnedObjectPath>;
}

#[proxy(
    interface = "org.freedesktop.login1.Session",
    default_service = "org.freedesktop.login1"
)]
trait LoginSession {
    fn set_locked_hint(&self, value: bool) -> Result<()>;
}

#[tokio::main]
pub async fn lock() -> anyhow::Result<(), Box<dyn Error>> {
    logger("Start server", "debug");

    let pid = std::process::id();
    logger(format!("PID of umbr-bin, {}", pid).as_str(), "debug");

    let connection = Connection::system().await?;

    logger("Connected to System DBUS", "debug");

    let proxy = LoginManagerProxy::new(&connection).await?;
    let path = proxy.get_session_by_pid(pid).await?;

    let session = LoginSessionProxy::new(&connection, path.as_str()).await?;
    session.set_locked_hint(true).await?;

    terminal::enable_raw_mode()?;

    print!("Press Enter to continue...");
    io::stdout().flush()?;
    let mut stream = signal(SignalKind::terminate())?;

    loop {
        stream.recv().await;

        if let Event::Key(key_event) = event::read()? {
            if key_event.code == KeyCode::Enter {
                session.set_locked_hint(false).await?;
                break;
            }
        }
    }

    terminal::disable_raw_mode()?;
    println!("\nContinuing program...");

    Ok(())
}

fn logger(message: &str, level: &str) {
    let log_level = env::var("LOG_LEVEL").unwrap_or("off".to_string());

    match log_level.as_str() {
        "debug" => {
            println!("{}: {}", level.to_uppercase(), message);
        }
        _ => {}
    }
}

#[derive(Error, Debug)]
pub enum UmbrError {
    #[error("{0}")]
    Generic(String),
    #[error("")]
    WindowingThreadQuit,
}

pub type Anyresult<T> = std::result::Result<T, UmbrError>;