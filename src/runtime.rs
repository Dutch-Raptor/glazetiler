use crate::diagnostics::{EventSink, RuntimeEvent};
use crate::protocol::{parse_glazewm_event, GlazeEvent};
use crate::tiling::direction_for_event;
use crate::GLAZEWM_WS_URL;
use anyhow::{anyhow, Context};
use std::net::TcpStream;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread;
use std::time::Duration;
use tungstenite::http::Uri;
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{connect, Message, WebSocket};

pub const SUBSCRIPTIONS: [&str; 3] = [
    "sub -e focus_changed",
    "sub -e focused_container_moved",
    "sub -e application_exiting",
];

const BACKOFF_SECONDS: [u64; 5] = [1, 2, 5, 10, 30];

pub trait IpcConnection {
    fn send_text(&mut self, message: &str) -> anyhow::Result<()>;
    fn read_text(&mut self) -> anyhow::Result<Option<String>>;
}

pub trait GlazewmConnector {
    type Connection: IpcConnection;

    fn connect(&mut self) -> anyhow::Result<Self::Connection>;
}

pub trait Sleeper {
    fn sleep(&mut self, duration: Duration);
}

#[derive(Debug, Clone, Default)]
pub struct ShutdownToken {
    is_shutdown: Arc<AtomicBool>,
}

impl ShutdownToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn request_shutdown(&self) {
        self.is_shutdown.store(true, Ordering::SeqCst);
    }

    pub fn is_shutdown_requested(&self) -> bool {
        self.is_shutdown.load(Ordering::SeqCst)
    }
}

pub enum StreamOutcome {
    ApplicationExiting,
    ShutdownRequested,
}

pub fn run_glazewm_event_loop() -> anyhow::Result<()> {
    run_glazewm_event_loop_with_status(ShutdownToken::new(), |_| {})
}

pub fn run_glazewm_event_loop_with_status<S>(
    shutdown: ShutdownToken,
    event_sink: S,
) -> anyhow::Result<()>
where
    S: EventSink,
{
    let mut connector = TungsteniteConnector::new(GLAZEWM_WS_URL);
    let mut sleeper = ThreadSleeper;

    run_with_reconnect(&mut connector, &mut sleeper, &shutdown, &event_sink)
}

pub fn run_with_reconnect<C, S, Events>(
    connector: &mut C,
    sleeper: &mut S,
    shutdown: &ShutdownToken,
    event_sink: &Events,
) -> anyhow::Result<()>
where
    C: GlazewmConnector,
    S: Sleeper,
    Events: EventSink,
{
    let mut retry_attempt = 0;
    event_sink.publish(RuntimeEvent::Starting);

    while !shutdown.is_shutdown_requested() {
        event_sink.publish(RuntimeEvent::Connecting);
        match connector.connect() {
            Ok(mut connection) => {
                retry_attempt = 0;
                event_sink.publish(RuntimeEvent::Connected);
                match run_event_stream(&mut connection, shutdown, event_sink) {
                    Ok(StreamOutcome::ApplicationExiting) => return Ok(()),
                    Ok(StreamOutcome::ShutdownRequested) => {
                        event_sink.publish(RuntimeEvent::ShutdownRequested);
                        return Ok(());
                    }
                    Err(error) => {
                        event_sink.publish(RuntimeEvent::ConnectionError {
                            message: format!("{error:#}"),
                        });
                        eprintln!("GlazeWM IPC connection lost: {error:#}");
                    }
                }
            }
            Err(error) => {
                event_sink.publish(RuntimeEvent::ConnectionError {
                    message: format!("{error:#}"),
                });
                eprintln!("Failed to connect to GlazeWM IPC at {GLAZEWM_WS_URL}: {error:#}");
            }
        }

        let delay = reconnect_delay(retry_attempt);
        retry_attempt = retry_attempt.saturating_add(1);
        event_sink.publish(RuntimeEvent::Reconnecting {
            delay_seconds: delay.as_secs(),
        });
        eprintln!(
            "Retrying GlazeWM IPC connection in {} second(s).",
            delay.as_secs()
        );
        sleeper.sleep(delay);
    }

    event_sink.publish(RuntimeEvent::ShutdownRequested);
    Ok(())
}

pub fn run_event_stream<Events>(
    connection: &mut impl IpcConnection,
    shutdown: &ShutdownToken,
    event_sink: &Events,
) -> anyhow::Result<StreamOutcome>
where
    Events: EventSink,
{
    subscribe_to_events(connection)?;

    while !shutdown.is_shutdown_requested() {
        let Some(message) = connection.read_text()? else {
            continue;
        };

        if handle_message(connection, &message, event_sink)? == MessageOutcome::ApplicationExiting {
            return Ok(StreamOutcome::ApplicationExiting);
        }
    }

    Ok(StreamOutcome::ShutdownRequested)
}

pub fn subscribe_to_events(connection: &mut impl IpcConnection) -> anyhow::Result<()> {
    for subscription in SUBSCRIPTIONS {
        connection.send_text(subscription).with_context(|| {
            format!("Failed to subscribe to GlazeWM IPC event with `{subscription}`")
        })?;
    }

    Ok(())
}

pub fn reconnect_delay(attempt: usize) -> Duration {
    Duration::from_secs(
        BACKOFF_SECONDS
            .get(attempt)
            .copied()
            .unwrap_or(*BACKOFF_SECONDS.last().expect("backoff is non-empty")),
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MessageOutcome {
    Continue,
    ApplicationExiting,
}

fn handle_message(
    connection: &mut impl IpcConnection,
    message: &str,
    event_sink: &impl EventSink,
) -> anyhow::Result<MessageOutcome> {
    let event = match parse_glazewm_event(message) {
        Ok(Some(event)) => event,
        Ok(None) => return Ok(MessageOutcome::Continue),
        Err(error) => {
            event_sink.publish(RuntimeEvent::IgnoredMessage {
                reason: error.to_string(),
            });
            eprintln!("Ignoring malformed GlazeWM IPC message: {error}");
            return Ok(MessageOutcome::Continue);
        }
    };

    if event == GlazeEvent::ApplicationExiting {
        event_sink.publish(RuntimeEvent::GlazewmExiting);
        eprintln!("GlazeWM is exiting; GAT-GWM is exiting too.");
        return Ok(MessageOutcome::ApplicationExiting);
    }

    if let Some(direction) = direction_for_event(&event) {
        connection.send_text(direction.command()).with_context(|| {
            format!(
                "Failed to send GlazeWM IPC command `{}`",
                direction.command()
            )
        })?;
        event_sink.publish(RuntimeEvent::DirectionChanged { direction });
    }

    Ok(MessageOutcome::Continue)
}

struct ThreadSleeper;

impl Sleeper for ThreadSleeper {
    fn sleep(&mut self, duration: Duration) {
        thread::sleep(duration);
    }
}

struct TungsteniteConnector {
    url: &'static str,
}

impl TungsteniteConnector {
    fn new(url: &'static str) -> Self {
        Self { url }
    }
}

impl GlazewmConnector for TungsteniteConnector {
    type Connection = TungsteniteConnection;

    fn connect(&mut self) -> anyhow::Result<Self::Connection> {
        let uri = self
            .url
            .parse::<Uri>()
            .context("Failed to parse GlazeWM IPC WebSocket URL")?;
        let (socket, _) = connect(uri)
            .with_context(|| format!("Failed to connect to GlazeWM IPC at {}", self.url))?;

        Ok(TungsteniteConnection { socket })
    }
}

struct TungsteniteConnection {
    socket: WebSocket<MaybeTlsStream<TcpStream>>,
}

impl IpcConnection for TungsteniteConnection {
    fn send_text(&mut self, message: &str) -> anyhow::Result<()> {
        self.socket
            .send(Message::Text(message.into()))
            .with_context(|| format!("Failed to write to GlazeWM IPC at {GLAZEWM_WS_URL}"))
    }

    fn read_text(&mut self) -> anyhow::Result<Option<String>> {
        match self
            .socket
            .read()
            .with_context(|| format!("Failed to read from GlazeWM IPC at {GLAZEWM_WS_URL}"))?
        {
            Message::Text(text) => Ok(Some(text.to_string())),
            Message::Close(_) => Err(anyhow!("GlazeWM IPC socket closed")),
            _ => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::rc::Rc;

    enum MockRead {
        Text(String),
        Error(&'static str),
        NonText,
    }

    struct MockConnection {
        id: usize,
        sent: Rc<RefCell<Vec<Vec<String>>>>,
        reads: VecDeque<MockRead>,
    }

    impl MockConnection {
        fn new(sent: Rc<RefCell<Vec<Vec<String>>>>, reads: Vec<MockRead>) -> Self {
            let id = {
                let mut sent = sent.borrow_mut();
                sent.push(Vec::new());
                sent.len() - 1
            };

            Self {
                id,
                sent,
                reads: reads.into(),
            }
        }
    }

    impl IpcConnection for MockConnection {
        fn send_text(&mut self, message: &str) -> anyhow::Result<()> {
            self.sent.borrow_mut()[self.id].push(message.to_string());
            Ok(())
        }

        fn read_text(&mut self) -> anyhow::Result<Option<String>> {
            match self.reads.pop_front() {
                Some(MockRead::Text(text)) => Ok(Some(text)),
                Some(MockRead::Error(message)) => Err(anyhow!(message)),
                Some(MockRead::NonText) => Ok(None),
                None => Err(anyhow!("no more mock reads")),
            }
        }
    }

    struct MockConnector {
        attempts: usize,
        sent: Rc<RefCell<Vec<Vec<String>>>>,
    }

    impl GlazewmConnector for MockConnector {
        type Connection = MockConnection;

        fn connect(&mut self) -> anyhow::Result<Self::Connection> {
            self.attempts += 1;

            match self.attempts {
                1 => Err(anyhow!("GlazeWM offline")),
                2 => Ok(MockConnection::new(
                    Rc::clone(&self.sent),
                    vec![MockRead::Error("socket dropped")],
                )),
                _ => Ok(MockConnection::new(
                    Rc::clone(&self.sent),
                    vec![MockRead::Text(
                        r#"{"data":{"eventType":"application_exiting"}}"#.to_string(),
                    )],
                )),
            }
        }
    }

    #[derive(Default)]
    struct MockSleeper {
        sleeps: Vec<Duration>,
    }

    impl Sleeper for MockSleeper {
        fn sleep(&mut self, duration: Duration) {
            self.sleeps.push(duration);
        }
    }

    #[test]
    fn subscribes_and_sends_direction_commands() {
        let sent = Rc::new(RefCell::new(Vec::new()));
        let mut connection = MockConnection::new(
            Rc::clone(&sent),
            vec![
                MockRead::NonText,
                MockRead::Text("not json".to_string()),
                MockRead::Text(
                    r#"{
                        "data": {
                            "eventType": "focus_changed",
                            "focusedContainer": {
                                "type": "window",
                                "hasFocus": true,
                                "width": 1000,
                                "height": 500
                            }
                        }
                    }"#
                    .to_string(),
                ),
                MockRead::Text(r#"{"data":{"eventType":"application_exiting"}}"#.to_string()),
            ],
        );

        let events = Rc::new(RefCell::new(Vec::new()));
        let event_sink = {
            let events = Rc::clone(&events);
            move |event| events.borrow_mut().push(event)
        };
        let outcome = run_event_stream(&mut connection, &ShutdownToken::new(), &event_sink)
            .expect("stream should finish cleanly");

        assert!(matches!(outcome, StreamOutcome::ApplicationExiting));
        assert_eq!(
            sent.borrow()[0],
            vec![
                "sub -e focus_changed",
                "sub -e focused_container_moved",
                "sub -e application_exiting",
                "command set-tiling-direction horizontal",
            ]
        );
        assert!(events.borrow().contains(&RuntimeEvent::DirectionChanged {
            direction: crate::tiling::TilingDirection::Horizontal,
        }));
        assert!(events
            .borrow()
            .iter()
            .any(|event| matches!(event, RuntimeEvent::IgnoredMessage { .. })));
    }

    #[test]
    fn reconnects_and_resubscribes_after_connection_loss() {
        let sent = Rc::new(RefCell::new(Vec::new()));
        let mut connector = MockConnector {
            attempts: 0,
            sent: Rc::clone(&sent),
        };
        let mut sleeper = MockSleeper::default();

        let events = Rc::new(RefCell::new(Vec::new()));
        let event_sink = {
            let events = Rc::clone(&events);
            move |event| events.borrow_mut().push(event)
        };

        run_with_reconnect(
            &mut connector,
            &mut sleeper,
            &ShutdownToken::new(),
            &event_sink,
        )
        .expect("runtime should exit cleanly");

        assert_eq!(connector.attempts, 3);
        assert_eq!(
            sleeper.sleeps,
            vec![Duration::from_secs(1), Duration::from_secs(1)]
        );
        assert_eq!(sent.borrow().len(), 2);
        for connection_sent in sent.borrow().iter() {
            assert_eq!(
                connection_sent,
                &vec![
                    "sub -e focus_changed".to_string(),
                    "sub -e focused_container_moved".to_string(),
                    "sub -e application_exiting".to_string(),
                ]
            );
        }
        assert!(events
            .borrow()
            .iter()
            .any(|event| matches!(event, RuntimeEvent::ConnectionError { .. })));
        assert!(events
            .borrow()
            .iter()
            .any(|event| matches!(event, RuntimeEvent::Reconnecting { .. })));
    }

    #[test]
    fn reconnect_delay_caps_at_thirty_seconds() {
        assert_eq!(reconnect_delay(0), Duration::from_secs(1));
        assert_eq!(reconnect_delay(1), Duration::from_secs(2));
        assert_eq!(reconnect_delay(4), Duration::from_secs(30));
        assert_eq!(reconnect_delay(99), Duration::from_secs(30));
    }
}
