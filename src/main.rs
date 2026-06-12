#![cfg_attr(
    all(feature = "no_console", target_os = "windows"),
    windows_subsystem = "windows"
)]

use anyhow::Context;
use gat_gwm::diagnostics::{
    append_log_line, ipc_url_text, log_file_path, DiagnosticsState, RuntimeEvent,
};
use gat_gwm::runtime::{run_glazewm_event_loop_with_status, ShutdownToken};
use std::thread;
use tao::event::{Event, StartCause};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tray_icon::menu::{Menu, MenuEvent, MenuId, MenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

#[derive(Debug, Clone)]
enum AppEvent {
    Runtime(RuntimeEvent),
    Menu(MenuEvent),
}

fn main() {
    if let Err(error) = run_app() {
        eprintln!("{error:#}");
        std::process::exit(1);
    }
}

fn run_app() -> anyhow::Result<()> {
    let event_loop = EventLoopBuilder::<AppEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();
    let menu_proxy = proxy.clone();
    MenuEvent::set_event_handler(Some(move |event| {
        _ = menu_proxy.send_event(AppEvent::Menu(event));
    }));

    let shutdown = ShutdownToken::new();
    let mut diagnostics = DiagnosticsState::default();
    let mut tray = None;
    let mut runtime_started = false;

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;

        match event {
            Event::NewEvents(StartCause::Init) if tray.is_none() => {
                match TrayUi::new() {
                    Ok(ui) => {
                        tray = Some(ui);
                        if let Some(tray) = tray.as_ref() {
                            tray.render(&diagnostics);
                        }
                    }
                    Err(error) => {
                        eprintln!("Failed to initialize GAT-GWM tray: {error:#}");
                        *control_flow = ControlFlow::Exit;
                        return;
                    }
                }

                if !runtime_started {
                    runtime_started = true;
                    start_runtime_thread(proxy.clone(), shutdown.clone());
                }
            }
            Event::UserEvent(AppEvent::Runtime(event)) => {
                if let Err(error) = append_log_line(&event) {
                    eprintln!("Failed to write GAT-GWM diagnostic log: {error}");
                }

                diagnostics.apply_event(&event);

                if let Some(tray) = tray.as_ref() {
                    tray.render(&diagnostics);
                }

                if matches!(
                    event,
                    RuntimeEvent::GlazewmExiting | RuntimeEvent::ShutdownRequested
                ) {
                    *control_flow = ControlFlow::Exit;
                }
            }
            Event::UserEvent(AppEvent::Menu(event)) => {
                if let Some(tray) = tray.as_ref() {
                    if tray.is_log_event(event.id()) {
                        if let Err(error) = open_log_folder() {
                            eprintln!("Failed to open GAT-GWM log folder: {error:#}");
                            let event = RuntimeEvent::UiError {
                                message: format!("Failed to open log folder: {error:#}"),
                            };
                            _ = append_log_line(&event);
                            diagnostics.apply_event(&event);
                            tray.render(&diagnostics);
                        }
                    } else if tray.is_quit_event(event.id()) {
                        shutdown.request_shutdown();
                        let event = RuntimeEvent::ShutdownRequested;
                        _ = append_log_line(&event);
                        diagnostics.apply_event(&event);
                        tray.render(&diagnostics);
                        *control_flow = ControlFlow::Exit;
                    }
                }
            }
            _ => {}
        }
    });
}

fn start_runtime_thread(proxy: tao::event_loop::EventLoopProxy<AppEvent>, shutdown: ShutdownToken) {
    thread::spawn(move || {
        let event_proxy = proxy.clone();
        let result = run_glazewm_event_loop_with_status(shutdown, move |status| {
            _ = event_proxy.send_event(AppEvent::Runtime(status));
        });

        if let Err(error) = result {
            _ = proxy.send_event(AppEvent::Runtime(RuntimeEvent::ConnectionError {
                message: format!("{error:#}"),
            }));
        }
    });
}

struct TrayUi {
    _tray_icon: TrayIcon,
    connection_item: MenuItem,
    log_id: MenuId,
    quit_id: MenuId,
}

impl TrayUi {
    fn new() -> anyhow::Result<Self> {
        let menu = Menu::new();
        let connection_item = MenuItem::new("Connection: starting", false, None);
        let ipc_item = MenuItem::new(ipc_url_text(), false, None);
        let log_item = MenuItem::new(format!("Log: {}", log_file_path().display()), true, None);
        let about_item = MenuItem::new(
            concat!("About GAT-GWM ", env!("CARGO_PKG_VERSION")),
            false,
            None,
        );
        let log_id = log_item.id().clone();
        let quit_item = MenuItem::new("Quit GAT-GWM", true, None);
        let quit_id = quit_item.id().clone();

        menu.append_items(&[
            &connection_item,
            &ipc_item,
            &log_item,
            &about_item,
            &quit_item,
        ])
        .context("Failed to initialize GAT-GWM tray menu")?;

        let tray_icon = TrayIconBuilder::new()
            .with_tooltip("GAT-GWM - starting")
            .with_title("GAT-GWM")
            .with_icon(app_icon()?)
            .with_menu(Box::new(menu))
            .with_menu_on_left_click(true)
            .with_menu_on_right_click(true)
            .build()
            .context("Failed to create GAT-GWM tray icon")?;

        Ok(Self {
            _tray_icon: tray_icon,
            connection_item,
            log_id,
            quit_id,
        })
    }

    fn render(&self, diagnostics: &DiagnosticsState) {
        let connection_text = diagnostics.connection.menu_text();
        self.connection_item.set_text(&connection_text);
        _ = self
            ._tray_icon
            .set_tooltip(Some(diagnostics.connection.tooltip_text()));

        #[cfg(target_os = "macos")]
        {
            _ = self._tray_icon.set_title(Some(connection_text));
        }
    }

    fn is_quit_event(&self, id: &MenuId) -> bool {
        id == &self.quit_id
    }

    fn is_log_event(&self, id: &MenuId) -> bool {
        id == &self.log_id
    }
}

fn open_log_folder() -> anyhow::Result<()> {
    let log_path = log_file_path();
    let log_folder = log_path
        .parent()
        .context("GAT-GWM log path did not include a containing folder")?;

    std::fs::create_dir_all(log_folder).with_context(|| {
        format!(
            "Failed to create GAT-GWM log folder at {}",
            log_folder.display()
        )
    })?;

    open_folder(log_folder)
}

#[cfg(target_os = "windows")]
fn open_folder(path: &std::path::Path) -> anyhow::Result<()> {
    std::process::Command::new("explorer")
        .arg(path)
        .spawn()
        .context("Failed to launch Windows Explorer for GAT-GWM log folder")?;

    Ok(())
}

#[cfg(target_os = "macos")]
fn open_folder(path: &std::path::Path) -> anyhow::Result<()> {
    std::process::Command::new("open")
        .arg(path)
        .spawn()
        .context("Failed to launch Finder for GAT-GWM log folder")?;

    Ok(())
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn open_folder(path: &std::path::Path) -> anyhow::Result<()> {
    std::process::Command::new("xdg-open")
        .arg(path)
        .spawn()
        .context("Failed to launch a file manager for GAT-GWM log folder")?;

    Ok(())
}

fn app_icon() -> anyhow::Result<Icon> {
    const SIZE: u32 = 32;
    let mut rgba = Vec::with_capacity((SIZE * SIZE * 4) as usize);

    for y in 0..SIZE {
        for x in 0..SIZE {
            let in_frame = !(4..=27).contains(&x) || !(4..=27).contains(&y);
            let in_split = (14..=17).contains(&x) || (14..=17).contains(&y);
            let accent = x > 17 && y > 17;
            let (r, g, b, a) = if in_frame {
                (22, 25, 32, 255)
            } else if in_split {
                (245, 247, 250, 255)
            } else if accent {
                (43, 132, 210, 255)
            } else {
                (72, 80, 94, 255)
            };

            rgba.extend_from_slice(&[r, g, b, a]);
        }
    }

    Icon::from_rgba(rgba, SIZE, SIZE).context("Failed to create GAT-GWM tray icon image")
}
