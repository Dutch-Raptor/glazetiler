#![cfg_attr(
    all(feature = "no_console", target_os = "windows"),
    windows_subsystem = "windows"
)]
use anyhow::Context;
use serde::de::DeserializeOwned;
use serde_json::value::Index;
use serde_json::Value;
use std::net::TcpStream;
#[cfg(target_os = "macos")]
use std::thread;
use tray_item::{IconSource, TrayItem};
use tungstenite::http::Uri;
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{connect, Message, WebSocket};

const APP_TITLE: &str = "GAT - GlazeWM Alternating Tiler";
const GLAZEWM_WS_URL: &str = "ws://localhost:6123";

fn main() -> anyhow::Result<()> {
    run_platform_app()
}

#[cfg(target_os = "macos")]
fn run_platform_app() -> anyhow::Result<()> {
    let mut tray = create_tray(IconSource::Resource(""))?;

    thread::spawn(|| {
        if let Err(err) = run_glazewm_event_loop() {
            eprintln!("{err:#}");
            std::process::exit(1);
        }
    });

    tray.inner_mut().display();

    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn run_platform_app() -> anyhow::Result<()> {
    let _tray = create_tray(IconSource::Resource("main-icon"))?;

    run_glazewm_event_loop()
}

fn create_tray(icon: IconSource) -> anyhow::Result<TrayItem> {
    let mut tray = TrayItem::new(APP_TITLE, icon).context("Failed to initialize tray")?;
    tray.add_label(APP_TITLE)?;
    tray.add_menu_item("Quit GAT", || std::process::exit(0))?;

    Ok(tray)
}

fn run_glazewm_event_loop() -> anyhow::Result<()> {
    let (mut socket, _) = connect(
        GLAZEWM_WS_URL
            .parse::<Uri>()
            .context("Failed to parse GWM WS URL")?,
    )
    .context("Failed to connect to GWM WS at ws://localhost:6123")?;

    socket
        .send(Message::Text(r#"sub -e focus_changed"#.into()))
        .context("Failed to subscribe to focus_changed event")?;

    socket
        .send(Message::Text(r#"sub -e focused_container_moved"#.into()))
        .context("Failed to subscribe to container moved")?;

    socket
        .send(Message::Text(r#"sub -e application_exiting"#.into()))
        .context("Failed to subscribe to application exiting")?;

    loop {
        let event = match read_as::<Value>(&mut socket) {
            Err(e) => {
                return Err(e);
            }
            Ok(Some(value)) => value,
            Ok(None) => continue,
        };

        let event_type = event.get_path(["data", "eventType"]);

        match event_type.and_then(|v| v.as_str()) {
            Some("focused_container_moved") => {
                _ = handle_focused_container_moved(event, &mut socket).inspect_err(|e| {
                    eprintln!("Failed to handle focused container moved event: {e}")
                });
            }
            Some("focus_changed") => {
                _ = handle_focus_changed(event, &mut socket)
                    .inspect_err(|e| eprintln!("Failed to handle focus changed event: {e}"))
            }
            Some("application_exiting") => {
                eprintln!("GlazeWM is exiting, exiting too.");
                std::process::exit(0);
            }
            _ => continue,
        }
    }
}

fn handle_focused_container_moved(
    event: Value,
    web_socket: &mut WebSocket<MaybeTlsStream<TcpStream>>,
) -> anyhow::Result<()> {
    let root_container = event
        .get_path(["data", "focusedContainer"])
        .context("Expected focused container event to contain a focused container field")?;

    fn find_focused_window(container: &Value) -> Option<&Value> {
        let container_type = container.get_path(["type"]).and_then(|v| v.as_str())?;
        let has_focus = container.get_path(["hasFocus"]).and_then(|v| v.as_bool())?;

        // Termination case: focused window found
        if container_type == "window" && has_focus {
            return Some(container);
        }

        let children = container.get("children").and_then(|v| v.as_array())?;

        // Recursive case: search through children
        for child in children {
            if let Some(focused_container) = find_focused_window(child) {
                return Some(focused_container);
            }
        }

        // Termination case: No window with focus
        None
    }

    if let Some(focused_window) = find_focused_window(root_container) {
        let (width, height) = get_container_size(&focused_window)
            .context("focused container did not have a width or height")?;
        change_tiling_direction(web_socket, width, height)?;
    }

    Ok(())
}

fn handle_focus_changed(
    event: Value,
    socket: &mut WebSocket<MaybeTlsStream<TcpStream>>,
) -> anyhow::Result<()> {
    let focused_container = event
        .get_path(["data", "focusedContainer"])
        .context("Expected focus changed event to contain a focused container field")?;
    let (width, height) = get_container_size(focused_container)
        .context("focused container did not have a width or height")?;

    change_tiling_direction(socket, width, height)?;

    Ok(())
}

fn get_container_size(event: &Value) -> Option<(f64, f64)> {
    let width = event.get("width").and_then(|v| v.as_f64())?;
    let height = event.get("height").and_then(|v| v.as_f64())?;

    Some((width, height))
}

fn change_tiling_direction(
    socket: &mut WebSocket<MaybeTlsStream<TcpStream>>,
    window_width: f64,
    window_height: f64,
) -> anyhow::Result<()> {
    if window_width < window_height {
        socket
            .send(Message::Text(
                "command set-tiling-direction vertical".into(),
            ))
            .context("Failed to send message to GWM")?;
    }
    if window_width > window_height {
        socket
            .send(Message::Text(
                "command set-tiling-direction horizontal".into(),
            ))
            .context("Failed to send message to GWM")?;
    };

    Ok(())
}

fn read_as<T: DeserializeOwned>(
    socket: &mut WebSocket<MaybeTlsStream<TcpStream>>,
) -> anyhow::Result<Option<T>> {
    let msg = match socket.read() {
        Ok(msg) => msg,
        Err(err) => {
            return Err(err).context("Failed to read message from GWM socket");
        }
    };

    let text = match msg.to_text() {
        Ok(text) => text,
        Err(err) => {
            eprintln!("Error while converting message to text: {err}");
            return Ok(None);
        }
    };

    let json_msg = match serde_json::from_str(text) {
        Ok(msg) => msg,
        Err(err) => {
            eprintln!("Error while parsing message as json: {err}");
            return Ok(None);
        }
    };

    Ok(Some(json_msg))
}

trait JsonValueExt {
    /// Retrieves a nested value based on the provided path of keys.
    ///
    /// # Arguments
    /// * `path` - An iterable of string keys specifying the nested path.
    ///
    /// # Returns
    /// * `Option<&Value>` - The nested value if found, otherwise `None`.
    fn get_path<T: IntoIterator<Item = I>, I: Index>(&self, path: T) -> Option<&Value>;
}

impl JsonValueExt for Value {
    fn get_path<T: IntoIterator<Item = I>, I: Index>(&self, path: T) -> Option<&Value> {
        path.into_iter()
            .fold(Some(self), |acc, key| acc.and_then(|v| v.get(key)))
    }
}
