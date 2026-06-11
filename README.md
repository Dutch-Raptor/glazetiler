# GAT - GlazeWM Alternating Tiler, written in Rust.

## What is this?
This is an auto tiler for the [GlazeWM](https://github.com/glzr-io/glazewm) window manager. It is written in Rust
which allows for easy and portable compilation.

Any time a window is focused, the tiling direction of the window will be set that a new window will be placed in the
direction with the highest length. 

```
Initial state:
A new window (C) is about to be placed. The tiling direction depends on the focused window.

A focused window is denoted by an asterisk (*).

    -----------------------
    |                     |
    |                     |
    |      a       B*     |  <- Suppose B is focused
    |                     |
    |                     |
    -----------------------

Since B is focused, the longest direction from B’s perspective is vertical, so C is placed below B:

    -----------------------
    |          |          |  
    |          |    b     |  
    |      a   |----------|  
    |          |    C*    |  <- C is placed below B
    |          |          |
    -----------------------
    
Now with C focused, the longest direction from C’s perspective is horizontal, so D is placed to the right of C:

    -----------------------
    |          |          |  
    |          |    b     |  
    |      a   |----------|  
    |          | c  |  D* |  <- D is placed to the right of C
    |          |    |     |
    -----------------------
```

It is a fork of the archived [GAT-GWM](https://github.com/ParasiteDelta/GAT-GWM) project by [ParasiteDelta](https://github.com/ParasiteDelta), but has mostly been rewritten.

## Installation

### Dependencies

- Rust (Can be installed with [rustup](https://rustup.rs/))
- GlazeWM running with its IPC WebSocket available at `ws://localhost:6123`

### Installing with Cargo

On Windows:

```bash
cargo install --git https://github.com/Dutch-Raptor/GAT-GWM.git --features=no_console
```

On macOS:

```bash
cargo install --git https://github.com/Dutch-Raptor/GAT-GWM.git
```

> If ~/.cargo/bin is in your PATH, you can run `gat-gwm` from anywhere.

### Building from source

On Windows:

```bash
git clone https://github.com/glzr-io/gat-gwm
cd gat-gwm
cargo build --release --features=no_console
```

On macOS:

```bash
git clone https://github.com/glzr-io/gat-gwm
cd gat-gwm
cargo build --release
```

> The executable will be in `target/release/gat-gwm`.

### Running Gat-GWM when GlazeWM starts

1. Open your GlazeWM config file (usually `~/.glzr/glazewm/config.yaml`).
2. Add the following command to your `general::startup_commands` list:

   ```shell-exec gat-gwm```

   **Example**:
    ```yaml
    general:
        # Commands to run when the WM has started (e.g. to run a script or launch
        # another application). Here we are running a batch script to start Zebar.
        startup_commands: ['shell-exec zebar', 'shell-exec gat-gwm']
    ```
   
## Limitations

- Does not support resizing of windows. I am not aware of a way to get notified when a window is resized by GlazeWM.
  As a workaround, you can refocus the window after it has been resized or manually set the tiling direction.
