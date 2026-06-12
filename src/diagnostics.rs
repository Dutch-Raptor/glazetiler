use crate::tiling::TilingDirection;
use crate::GLAZEWM_WS_URL;
use std::env;
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeEvent {
    Starting,
    Connecting,
    Connected,
    Reconnecting { delay_seconds: u64 },
    DirectionChanged { direction: TilingDirection },
    IgnoredMessage { reason: String },
    ConnectionError { message: String },
    UiError { message: String },
    GlazewmExiting,
    ShutdownRequested,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionState {
    Starting,
    Connecting,
    Connected,
    Reconnecting { delay_seconds: u64 },
    Error { message: String },
    GlazewmExiting,
    ShuttingDown,
}

impl ConnectionState {
    pub fn menu_text(&self) -> String {
        match self {
            Self::Starting => "Connection: starting".to_string(),
            Self::Connecting => "Connection: connecting to GlazeWM IPC".to_string(),
            Self::Connected => "Connection: connected".to_string(),
            Self::Reconnecting { delay_seconds } => {
                format!("Connection: reconnecting in {delay_seconds}s")
            }
            Self::Error { message } => format!("Connection: error - {}", one_line(message)),
            Self::GlazewmExiting => "Connection: GlazeWM exiting".to_string(),
            Self::ShuttingDown => "Connection: shutting down".to_string(),
        }
    }

    pub fn tooltip_text(&self) -> String {
        format!(
            "GAT-GWM - {}",
            self.menu_text().replacen("Connection: ", "", 1)
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticsState {
    pub connection: ConnectionState,
    pub last_direction: Option<TilingDirection>,
    pub last_ignored_message: Option<String>,
    pub last_error: Option<String>,
}

impl Default for DiagnosticsState {
    fn default() -> Self {
        Self {
            connection: ConnectionState::Starting,
            last_direction: None,
            last_ignored_message: None,
            last_error: None,
        }
    }
}

impl DiagnosticsState {
    pub fn apply_event(&mut self, event: &RuntimeEvent) {
        match event {
            RuntimeEvent::Starting => self.connection = ConnectionState::Starting,
            RuntimeEvent::Connecting => self.connection = ConnectionState::Connecting,
            RuntimeEvent::Connected => self.connection = ConnectionState::Connected,
            RuntimeEvent::Reconnecting { delay_seconds } => {
                self.connection = ConnectionState::Reconnecting {
                    delay_seconds: *delay_seconds,
                };
            }
            RuntimeEvent::ConnectionError { message } => {
                self.connection = ConnectionState::Error {
                    message: message.clone(),
                };
                self.last_error = Some(message.clone());
            }
            RuntimeEvent::UiError { message } => {
                self.last_error = Some(message.clone());
            }
            RuntimeEvent::GlazewmExiting => self.connection = ConnectionState::GlazewmExiting,
            RuntimeEvent::ShutdownRequested => self.connection = ConnectionState::ShuttingDown,
            RuntimeEvent::DirectionChanged { direction } => {
                self.last_direction = Some(*direction);
            }
            RuntimeEvent::IgnoredMessage { reason } => {
                self.last_ignored_message = Some(reason.clone());
            }
        }
    }
}

impl fmt::Display for RuntimeEvent {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Starting
            | Self::Connecting
            | Self::Connected
            | Self::Reconnecting { .. }
            | Self::ConnectionError { .. }
            | Self::GlazewmExiting
            | Self::ShutdownRequested => write!(
                formatter,
                "{}",
                connection_state_from_event(self)
                    .expect("connection lifecycle events have connection state")
                    .menu_text()
            ),
            Self::DirectionChanged { direction } => {
                write!(formatter, "Tiling: set {} direction", direction.as_str())
            }
            Self::IgnoredMessage { reason } => {
                write!(formatter, "Ignored IPC message: {}", one_line(reason))
            }
            Self::UiError { message } => {
                write!(formatter, "UI error: {}", one_line(message))
            }
        }
    }
}

pub trait EventSink {
    fn publish(&self, event: RuntimeEvent);
}

impl<F> EventSink for F
where
    F: Fn(RuntimeEvent),
{
    fn publish(&self, event: RuntimeEvent) {
        self(event);
    }
}

pub fn log_file_path() -> PathBuf {
    app_data_dir().join("gat-gwm.log")
}

pub fn append_log_line(event: &RuntimeEvent) -> io::Result<()> {
    let path = log_file_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(file, "{} {}", timestamp_seconds(), event)
}

pub fn ipc_url_text() -> String {
    format!("IPC: {GLAZEWM_WS_URL}")
}

fn app_data_dir() -> PathBuf {
    if let Ok(app_data) = env::var("APPDATA") {
        return PathBuf::from(app_data).join("GAT-GWM");
    }

    if let Ok(home) = env::var("HOME") {
        return PathBuf::from(home)
            .join("Library")
            .join("Application Support")
            .join("GAT-GWM");
    }

    env::temp_dir().join("GAT-GWM")
}

fn timestamp_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn one_line(value: &str) -> String {
    let mut normalized = value.split_whitespace().collect::<Vec<_>>().join(" ");
    const MAX_LEN: usize = 96;
    if normalized.len() > MAX_LEN {
        normalized.truncate(MAX_LEN - 3);
        normalized.push_str("...");
    }

    normalized
}

fn connection_state_from_event(event: &RuntimeEvent) -> Option<ConnectionState> {
    match event {
        RuntimeEvent::Starting => Some(ConnectionState::Starting),
        RuntimeEvent::Connecting => Some(ConnectionState::Connecting),
        RuntimeEvent::Connected => Some(ConnectionState::Connected),
        RuntimeEvent::Reconnecting { delay_seconds } => Some(ConnectionState::Reconnecting {
            delay_seconds: *delay_seconds,
        }),
        RuntimeEvent::ConnectionError { message } => Some(ConnectionState::Error {
            message: message.clone(),
        }),
        RuntimeEvent::GlazewmExiting => Some(ConnectionState::GlazewmExiting),
        RuntimeEvent::ShutdownRequested => Some(ConnectionState::ShuttingDown),
        RuntimeEvent::DirectionChanged { .. }
        | RuntimeEvent::IgnoredMessage { .. }
        | RuntimeEvent::UiError { .. } => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_connection_text_for_connected_state() {
        assert_eq!(
            ConnectionState::Connected.menu_text(),
            "Connection: connected".to_string()
        );
        assert_eq!(
            ConnectionState::Connected.tooltip_text(),
            "GAT-GWM - connected".to_string()
        );
    }

    #[test]
    fn direction_changes_update_state_without_changing_connection() {
        let mut state = DiagnosticsState {
            connection: ConnectionState::Connected,
            ..Default::default()
        };

        state.apply_event(&RuntimeEvent::DirectionChanged {
            direction: TilingDirection::Horizontal,
        });

        assert_eq!(state.connection, ConnectionState::Connected);
        assert_eq!(state.last_direction, Some(TilingDirection::Horizontal));
    }

    #[test]
    fn ignored_messages_update_state_without_changing_connection() {
        let mut state = DiagnosticsState {
            connection: ConnectionState::Connected,
            ..Default::default()
        };

        state.apply_event(&RuntimeEvent::IgnoredMessage {
            reason: "first line\nsecond line".to_string(),
        });

        assert_eq!(state.connection, ConnectionState::Connected);
        assert_eq!(
            state.last_ignored_message,
            Some("first line\nsecond line".to_string())
        );
    }

    #[test]
    fn ui_errors_update_last_error_without_changing_connection() {
        let mut state = DiagnosticsState {
            connection: ConnectionState::Connected,
            ..Default::default()
        };

        state.apply_event(&RuntimeEvent::UiError {
            message: "could not open log folder".to_string(),
        });

        assert_eq!(state.connection, ConnectionState::Connected);
        assert_eq!(
            state.last_error,
            Some("could not open log folder".to_string())
        );
    }

    #[test]
    fn formats_connection_errors_on_one_line() {
        assert_eq!(
            ConnectionState::Error {
                message: "first line\nsecond line".to_string(),
            }
            .menu_text(),
            "Connection: error - first line second line".to_string()
        );
    }

    #[test]
    fn connection_errors_update_connection_and_last_error() {
        let mut state = DiagnosticsState::default();

        state.apply_event(&RuntimeEvent::ConnectionError {
            message: "socket closed".to_string(),
        });

        assert_eq!(
            state.connection,
            ConnectionState::Error {
                message: "socket closed".to_string()
            }
        );
        assert_eq!(state.last_error, Some("socket closed".to_string()));
    }

    #[test]
    fn log_format_keeps_direction_events() {
        assert_eq!(
            RuntimeEvent::DirectionChanged {
                direction: TilingDirection::Vertical,
            }
            .to_string(),
            "Tiling: set vertical direction"
        );
    }
}
