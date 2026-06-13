# GlazeTiler

GlazeTiler is a lightweight tray utility that keeps the [GlazeWM](https://github.com/glzr-io/glazewm) tiling direction aligned with the focused window.

When the focused window is wider than it is tall, GlazeTiler sets GlazeWM's tiling direction to horizontal. When the focused window is taller than it is wide, it sets the tiling direction to vertical. This makes the next window open along the focused window's longest available axis.

GlazeTiler supports GlazeWM on Windows and macOS.

## How It Works

GlazeTiler connects to the GlazeWM IPC WebSocket at `ws://localhost:6123`, listens for focus and focused-container movement events, and sends `set-tiling-direction` commands back to GlazeWM.

```text
Initial state:
A new window (C) is about to be placed. The tiling direction depends on the focused window.

A focused window is denoted by an asterisk (*).

    -----------------------
    |                     |
    |                     |
    |      A       B*     |  <- Suppose B is focused
    |                     |
    |                     |
    -----------------------

Since B is focused, the longest direction from B's perspective is vertical, so C is placed below B:

    -----------------------
    |          |          |
    |          |    B     |
    |      A   |----------|
    |          |    C*    |  <- C is placed below B
    |          |          |
    -----------------------

Now with C focused, the longest direction from C's perspective is horizontal, so D is placed to the right of C:

    -----------------------
    |          |          |
    |          |    B     |
    |      A   |----------|
    |          | C  |  D* |  <- D is placed to the right of C
    |          |    |     |
    -----------------------
```

## Installation

### Dependencies

- Rust, installed with [rustup](https://rustup.rs/)
- GlazeWM running with its IPC WebSocket available at `ws://localhost:6123`

### Install With Cargo

```bash
cargo install --git https://github.com/Dutch-Raptor/glazetiler.git
```

If `~/.cargo/bin` is in your `PATH`, you can run `glazetiler` from anywhere.

### Build From Source

```bash
git clone https://github.com/Dutch-Raptor/glazetiler.git
cd glazetiler
cargo build --release
```

The executable will be in `target/release/glazetiler`.

## Run With GlazeWM

Open your GlazeWM config file, usually `~/.glzr/glazewm/config.yaml`, and add GlazeTiler to `general.startup_commands`:

```yaml
general:
  startup_commands: ['shell-exec glazetiler']
```

## Diagnostics

GlazeTiler runs as a tray application and shows live diagnostic state in its tray menu:

- The current connection state at the top level
- A Diagnostics submenu with the GlazeWM IPC URL, rolling log file pattern, and an action to open the log folder
- An App submenu with version information

Tiling direction changes and ignored IPC messages are written to daily rolling log files instead of the tray menu. The app keeps the most recent 14 daily log files.

On Windows, logs are written to `%APPDATA%\GlazeTiler\glazetiler.log.YYYY-MM-DD`.
On macOS, logs are written to `~/Library/Application Support/GlazeTiler/glazetiler.log.YYYY-MM-DD`.

## Limitations

- Window resize events are not currently handled directly. As a workaround, refocus the window after it has been resized or manually set the tiling direction.

## Development

```bash
cargo test
cargo fmt
cargo build --release
```

## Credits

GlazeTiler was originally based on the archived [GAT-GWM](https://github.com/ParasiteDelta/GAT-GWM) project by [ParasiteDelta](https://github.com/ParasiteDelta).
