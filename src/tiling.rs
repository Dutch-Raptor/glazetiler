use crate::protocol::{Container, GlazeEvent};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TilingDirection {
    Horizontal,
    Vertical,
}

impl TilingDirection {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Horizontal => "horizontal",
            Self::Vertical => "vertical",
        }
    }

    pub fn command(self) -> &'static str {
        match self {
            Self::Horizontal => "command set-tiling-direction horizontal",
            Self::Vertical => "command set-tiling-direction vertical",
        }
    }
}

pub fn direction_for_size(width: f64, height: f64) -> Option<TilingDirection> {
    if width > height {
        Some(TilingDirection::Horizontal)
    } else if height > width {
        Some(TilingDirection::Vertical)
    } else {
        None
    }
}

pub fn direction_for_container(container: &Container) -> Option<TilingDirection> {
    let (width, height) = container.size()?;

    direction_for_size(width, height)
}

pub fn direction_for_event(event: &GlazeEvent) -> Option<TilingDirection> {
    match event {
        GlazeEvent::FocusChanged { focused_container } => {
            direction_for_container(focused_container)
        }
        GlazeEvent::FocusedContainerMoved { focused_container } => focused_container
            .find_focused_window()
            .and_then(direction_for_container),
        GlazeEvent::ApplicationExiting => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn container(width: Option<f64>, height: Option<f64>) -> Container {
        Container {
            container_type: Some("window".to_string()),
            has_focus: true,
            width,
            height,
            children: Vec::new(),
        }
    }

    #[test]
    fn wider_container_is_horizontal() {
        assert_eq!(
            direction_for_size(900.0, 600.0),
            Some(TilingDirection::Horizontal)
        );
    }

    #[test]
    fn taller_container_is_vertical() {
        assert_eq!(
            direction_for_size(600.0, 900.0),
            Some(TilingDirection::Vertical)
        );
    }

    #[test]
    fn square_container_does_not_change_direction() {
        assert_eq!(direction_for_size(600.0, 600.0), None);
    }

    #[test]
    fn missing_dimensions_do_not_change_direction() {
        assert_eq!(direction_for_container(&container(None, Some(600.0))), None);
        assert_eq!(direction_for_container(&container(Some(600.0), None)), None);
    }

    #[test]
    fn focused_container_moved_uses_nested_focused_window() {
        let event = GlazeEvent::FocusedContainerMoved {
            focused_container: Container {
                container_type: Some("split".to_string()),
                has_focus: false,
                width: None,
                height: None,
                children: vec![container(Some(400.0), Some(900.0))],
            },
        };

        assert_eq!(direction_for_event(&event), Some(TilingDirection::Vertical));
    }
}
