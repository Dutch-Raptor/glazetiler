use serde::Deserialize;
use serde_json::Value;
use std::error::Error;
use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum GlazeEvent {
    FocusChanged { focused_container: Container },
    FocusedContainerMoved { focused_container: Container },
    ApplicationExiting,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Container {
    #[serde(rename = "type")]
    pub container_type: Option<String>,
    #[serde(rename = "hasFocus", default)]
    pub has_focus: bool,
    pub width: Option<f64>,
    pub height: Option<f64>,
    #[serde(default)]
    pub children: Vec<Container>,
}

impl Container {
    pub fn size(&self) -> Option<(f64, f64)> {
        Some((self.width?, self.height?))
    }

    pub fn find_focused_window(&self) -> Option<&Container> {
        if self.container_type.as_deref() == Some("window") && self.has_focus {
            return Some(self);
        }

        self.children
            .iter()
            .find_map(Container::find_focused_window)
    }
}

#[derive(Debug)]
pub enum ParseEventError {
    InvalidJson(serde_json::Error),
    MissingEventType,
    InvalidPayload {
        event_type: String,
        source: serde_json::Error,
    },
}

impl fmt::Display for ParseEventError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidJson(source) => write!(formatter, "invalid GlazeWM IPC JSON: {source}"),
            Self::MissingEventType => write!(
                formatter,
                "GlazeWM IPC message did not include data.eventType"
            ),
            Self::InvalidPayload { event_type, source } => {
                write!(
                    formatter,
                    "invalid GlazeWM IPC payload for {event_type}: {source}"
                )
            }
        }
    }
}

impl Error for ParseEventError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidJson(source) | Self::InvalidPayload { source, .. } => Some(source),
            Self::MissingEventType => None,
        }
    }
}

#[derive(Debug, Deserialize)]
struct EventEnvelope {
    data: EventData,
}

#[derive(Debug, Deserialize)]
struct EventData {
    #[serde(rename = "eventType")]
    event_type: String,
}

#[derive(Debug, Deserialize)]
struct FocusedContainerEnvelope {
    data: FocusedContainerData,
}

#[derive(Debug, Deserialize)]
struct FocusedContainerData {
    #[serde(rename = "focusedContainer")]
    focused_container: Container,
}

pub fn parse_glazewm_event(message: &str) -> Result<Option<GlazeEvent>, ParseEventError> {
    let value: Value = serde_json::from_str(message).map_err(ParseEventError::InvalidJson)?;
    let envelope: EventEnvelope = serde_json::from_value(value.clone()).map_err(|error| {
        match missing_event_type(&value) {
            true => ParseEventError::MissingEventType,
            false => ParseEventError::InvalidPayload {
                event_type: "unknown".to_string(),
                source: error,
            },
        }
    })?;

    let event_type = envelope.data.event_type;

    match event_type.as_str() {
        "focus_changed" => focused_container_event(value, event_type)
            .map(|focused_container| Some(GlazeEvent::FocusChanged { focused_container })),
        "focused_container_moved" => focused_container_event(value, event_type)
            .map(|focused_container| Some(GlazeEvent::FocusedContainerMoved { focused_container })),
        "application_exiting" => Ok(Some(GlazeEvent::ApplicationExiting)),
        _ => Ok(None),
    }
}

fn focused_container_event(value: Value, event_type: String) -> Result<Container, ParseEventError> {
    let envelope: FocusedContainerEnvelope = serde_json::from_value(value)
        .map_err(|source| ParseEventError::InvalidPayload { event_type, source })?;

    Ok(envelope.data.focused_container)
}

fn missing_event_type(value: &Value) -> bool {
    value
        .get("data")
        .and_then(|data| data.get("eventType"))
        .is_none()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_focus_changed_event() {
        let event = parse_glazewm_event(
            r#"{
                "data": {
                    "eventType": "focus_changed",
                    "focusedContainer": {
                        "type": "window",
                        "hasFocus": true,
                        "width": 800,
                        "height": 600
                    }
                }
            }"#,
        )
        .expect("event should parse");

        assert_eq!(
            event,
            Some(GlazeEvent::FocusChanged {
                focused_container: Container {
                    container_type: Some("window".to_string()),
                    has_focus: true,
                    width: Some(800.0),
                    height: Some(600.0),
                    children: Vec::new(),
                },
            })
        );
    }

    #[test]
    fn parses_application_exiting_event() {
        let event = parse_glazewm_event(r#"{"data":{"eventType":"application_exiting"}}"#)
            .expect("event should parse");

        assert_eq!(event, Some(GlazeEvent::ApplicationExiting));
    }

    #[test]
    fn ignores_unsupported_events() {
        let event = parse_glazewm_event(r#"{"data":{"eventType":"workspace_changed"}}"#)
            .expect("event should parse");

        assert_eq!(event, None);
    }

    #[test]
    fn reports_missing_event_type() {
        let error = parse_glazewm_event(r#"{"data":{}}"#).expect_err("event should fail");

        assert!(matches!(error, ParseEventError::MissingEventType));
    }

    #[test]
    fn finds_focused_window_in_nested_container() {
        let root = Container {
            container_type: Some("split".to_string()),
            has_focus: false,
            width: None,
            height: None,
            children: vec![
                Container {
                    container_type: Some("window".to_string()),
                    has_focus: false,
                    width: Some(400.0),
                    height: Some(400.0),
                    children: Vec::new(),
                },
                Container {
                    container_type: Some("split".to_string()),
                    has_focus: false,
                    width: None,
                    height: None,
                    children: vec![Container {
                        container_type: Some("window".to_string()),
                        has_focus: true,
                        width: Some(300.0),
                        height: Some(700.0),
                        children: Vec::new(),
                    }],
                },
            ],
        };

        let focused = root
            .find_focused_window()
            .expect("focused window should be found");

        assert_eq!(focused.size(), Some((300.0, 700.0)));
    }
}
