# Keep Active

KeepActive is a Windows utility written in Rust that keeps a target application responsive by nudging its main window. It can watch for either a specific executable or a window title, and it now ships with a graphical interface by default while still offering an interactive CLI for power users.

## Features

- GUI front-end with start/stop controls and editable targets
- CLI mode (`--cli`) that mirrors the legacy behaviour with `1/0/q` commands
- Targets windows by process name first, falling back to a window title
- Automatically prompts for elevation and relaunches with administrator rights when required
- Refresh cadence of 100 ms using Windows APIs (EnumWindows, SendMessage, etc.)

## Requirements

- Windows 10 or newer
- [Rust](https://www.rust-lang.org/tools/install) toolchain (edition 2024, Rust 1.82+ recommended)

## Building

```powershell
cargo build --release
```

The optimised binary is emitted at `target\release\KeepActive.exe`.

## Running

### GUI (default)

```powershell
cargo run --release
```

Launching the binary directly (`KeepActive.exe`) opens the GUI. Provide a fallback window title and an optional executable name, then press **Start**. The executable name (e.g. `notepad.exe`) is prioritised; the window title is used if no process window is located.

### CLI mode

```powershell
cargo run -- --cli [-w "Window Title"] [-e "process.exe"]
```

- `-w / --window` – fallback window title (defaults to `CounterSide`)
- `-e / --exe` – executable name to prioritise
- Commands once running:
  - `1` – start the activation loop
  - `0` – stop the activation loop
  - `q` – quit the application

When launched in CLI mode from the compiled binary, invoke it the same way:

```powershell
KeepActive.exe --cli -e notepad.exe
```

## Notes

- The application relaunches itself with "Run as administrator" if it is not already elevated. Accept the UAC prompt to allow it to control other windows.
- A 100 ms polling interval is used; adjust the source (`src\main.rs`) if you need a different cadence.

## License

This project is open source and available under the [MIT License](LICENSE).
