use std::{
    cell::RefCell,
    ffi::{c_void, OsStr},
    io::{self, Write},
    os::windows::ffi::OsStrExt,
    rc::Rc,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::Duration,
};

use anyhow::{anyhow, Context, Result};
use clap::Parser;
use native_windows_gui as nwg;
use windows::{
    core::{w, PCWSTR},
    Win32::{
        Foundation::{BOOL, CloseHandle, HANDLE, HWND, LPARAM, WPARAM},
        Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY},
        System::{
            Console::GetConsoleWindow,
            Diagnostics::ToolHelp::{
                CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
                TH32CS_SNAPPROCESS,
            },
            Threading::{GetCurrentProcess, OpenProcessToken},
        },
        UI::{
            Shell::ShellExecuteW,
            WindowsAndMessaging::{
                EnumWindows, FindWindowW, GetWindowTextLengthW, GetWindowThreadProcessId,
                IsWindowVisible, SendMessageW, ShowWindow, SW_HIDE, SW_SHOWNORMAL, WM_ACTIVATE,
            },
        },
    },
};

const DEFAULT_WINDOW_TITLE: &str = "CounterSide";
const REFRESH_INTERVAL_MS: u64 = 100;
const WA_CLICKACTIVE: usize = 2;

#[derive(Parser, Debug)]
#[command(author, version, about = "KeepActive - keep a target window in the foreground")]
struct Args {
    /// Run the application in console/CLI mode
    #[arg(long)]
    cli: bool,

    /// Window title to target (fallback if process is not found)
    #[arg(short = 'w', long = "window")]
    window: Option<String>,

    /// Executable name to target (e.g. notepad.exe)
    #[arg(short = 'e', long = "exe")]
    exe: Option<String>,
}

#[derive(Clone, Debug)]
struct AppConfig {
    window_title: String,
    process_name: Option<String>,
}

impl AppConfig {
    fn from_args(args: &Args) -> Self {
        let window_title = args
            .window
            .clone()
            .unwrap_or_else(|| DEFAULT_WINDOW_TITLE.to_string());
        let process_name = args.exe.clone().map(normalize_filter);
        Self {
            window_title,
            process_name,
        }
    }

    fn resolved(&self) -> ResolvedConfig {
        ResolvedConfig {
            window_title: Some(self.window_title.clone()),
            process_name: self.process_name.clone(),
        }
    }
}

#[derive(Clone, Debug)]
struct ResolvedConfig {
    window_title: Option<String>,
    process_name: Option<String>,
}

impl ResolvedConfig {
    fn with_inputs(window_title: String, process_name: Option<String>) -> Self {
        let process_name = process_name.map(normalize_filter);
        let title = if window_title.trim().is_empty() {
            DEFAULT_WINDOW_TITLE.to_string()
        } else {
            window_title.trim().to_string()
        };
        Self {
            window_title: Some(title),
            process_name,
        }
    }
}

struct KeepAliveController {
    active: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}

impl KeepAliveController {
    fn new() -> Self {
        Self {
            active: Arc::new(AtomicBool::new(false)),
            worker: None,
        }
    }

    fn start(&mut self, config: ResolvedConfig) -> Result<()> {
        if self.is_running() {
            return Ok(());
        }

        self.active.store(true, Ordering::SeqCst);
        let flag = Arc::clone(&self.active);
        self.worker = Some(thread::spawn(move || worker_loop(flag, config)));
        Ok(())
    }

    fn stop(&mut self) -> Result<()> {
        self.active.store(false, Ordering::SeqCst);
        if let Some(handle) = self.worker.take() {
            handle
                .join()
                .map_err(|_| anyhow!("worker thread terminated unexpectedly"))?;
        }
        Ok(())
    }

    fn is_running(&self) -> bool {
        self.active.load(Ordering::SeqCst) && self.worker.is_some()
    }
}

impl Drop for KeepAliveController {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

fn main() -> Result<()> {
    let args = Args::parse();
    ensure_admin()?;
    if !args.cli {
        hide_console_window();
    }

    let config = AppConfig::from_args(&args);
    if args.cli {
        run_cli(config)?;
    } else {
        run_gui(config)?;
    }
    Ok(())
}

fn run_cli(config: AppConfig) -> Result<()> {
    println!("KeepActive - Rust CLI");
    println!("Target executable: {}", config.process_name.as_deref().unwrap_or("not set"));
    println!("Fallback window title: {}", config.window_title);
    println!("----------------------------------------");
    println!("Commands: 1 = start, 0 = stop, q = quit");

    let mut controller = KeepAliveController::new();
    let stdin = io::stdin();
    let mut buffer = String::new();

    loop {
        print!("> ");
        io::stdout().flush().ok();

        buffer.clear();
        if stdin.read_line(&mut buffer)? == 0 {
            continue;
        }
        let trimmed = buffer.trim();
        match trimmed {
            "1" => {
                if controller.is_running() {
                    println!("Already running.");
                    continue;
                }
                controller.start(config.resolved())?;
                println!("Activation loop started.");
            }
            "0" => {
                if controller.is_running() {
                    controller.stop()?;
                    println!("Activation loop stopped.");
                } else {
                    println!("Not running.");
                }
            }
            "q" | "Q" => {
                controller.stop().ok();
                println!("Exiting.");
                break;
            }
            _ => println!("Unknown command: {}", trimmed),
        }
    }

    Ok(())
}

fn run_gui(config: AppConfig) -> Result<()> {
    nwg::init().context("failed to initialise GUI runtime")?;
    let _ = nwg::Font::set_global_family("Segoe UI");

    let mut window = nwg::Window::default();
    nwg::Window::builder()
        .flags(nwg::WindowFlags::WINDOW | nwg::WindowFlags::VISIBLE)
        .size((420, 260))
        .title("KeepActive")
        .build(&mut window)
        .context("failed to build main window")?;

    let mut _window_label = nwg::Label::default();
    nwg::Label::builder()
        .text("Window Title (fallback)")
        .position((20, 20))
        .size((180, 24))
        .parent(&window)
        .build(&mut _window_label)
        .context("failed to build window label")?;

    let mut window_input = nwg::TextInput::default();
    nwg::TextInput::builder()
        .text(&config.window_title)
        .position((20, 48))
        .size((360, 28))
        .parent(&window)
        .build(&mut window_input)
        .context("failed to build window text input")?;

    let mut _exe_label = nwg::Label::default();
    nwg::Label::builder()
        .text("Executable Name (optional)")
        .position((20, 90))
        .size((200, 24))
        .parent(&window)
        .build(&mut _exe_label)
        .context("failed to build process label")?;

    let mut exe_input = nwg::TextInput::default();
    nwg::TextInput::builder()
        .text(config.process_name.as_deref().unwrap_or(""))
        .position((20, 118))
        .size((360, 28))
        .parent(&window)
        .build(&mut exe_input)
        .context("failed to build process text input")?;

    let mut status_label = nwg::Label::default();
    nwg::Label::builder()
        .text("Status: Not running")
        .position((20, 162))
        .size((360, 24))
        .parent(&window)
        .build(&mut status_label)
        .context("failed to build status label")?;

    let mut start_btn = nwg::Button::default();
    nwg::Button::builder()
        .text("Start")
        .position((20, 200))
        .size((160, 32))
        .parent(&window)
        .build(&mut start_btn)
        .context("failed to build start button")?;

    let mut stop_btn = nwg::Button::default();
    nwg::Button::builder()
        .text("Stop")
        .enabled(false)
        .position((220, 200))
        .size((160, 32))
        .parent(&window)
        .build(&mut stop_btn)
        .context("failed to build stop button")?;

    struct GuiState {
        controller: KeepAliveController,
        window_input: nwg::TextInput,
        exe_input: nwg::TextInput,
        status_label: nwg::Label,
        start_btn: nwg::Button,
        stop_btn: nwg::Button,
    }

    let state = Rc::new(RefCell::new(GuiState {
        controller: KeepAliveController::new(),
        window_input,
        exe_input,
        status_label,
        start_btn,
        stop_btn,
    }));

    let ui_state = Rc::clone(&state);
    let handler = nwg::full_bind_event_handler(&window.handle, move |evt, _, handle| {
        use nwg::Event;
        let mut state = ui_state.borrow_mut();
        match evt {
            Event::OnButtonClick => {
                if handle == state.start_btn.handle {
                    let window_value = state.window_input.text();
                    let exe_value = state.exe_input.text();
                    let exe_value = exe_value.trim().to_string();
                    let process_name = if exe_value.is_empty() {
                        None
                    } else {
                        Some(exe_value)
                    };
                    let config = ResolvedConfig::with_inputs(window_value, process_name);

                    if let Err(err) = state.controller.start(config) {
                        let message = format!("Error: {}", err);
                        state.status_label.set_text(&format!("Status: {}", message));
                        nwg::simple_message("KeepActive error", &message);
                    } else {
                        state.status_label.set_text("Status: Running");
                        state.start_btn.set_enabled(false);
                        state.stop_btn.set_enabled(true);
                    }
                } else if handle == state.stop_btn.handle {
                    if let Err(err) = state.controller.stop() {
                        let message = format!("Error: {}", err);
                        state.status_label.set_text(&format!("Status: {}", message));
                        nwg::simple_message("KeepActive error", &message);
                    } else {
                        state.status_label.set_text("Status: Not running");
                        state.start_btn.set_enabled(true);
                        state.stop_btn.set_enabled(false);
                    }
                }
            }
            Event::OnWindowClose => {
                state.controller.stop().ok();
                nwg::stop_thread_dispatch();
            }
            _ => {}
        }
    });

    let _guard = EventHandlerGuard { handler: Some(handler) };

    nwg::dispatch_thread_events();
    Ok(())
}

struct EventHandlerGuard {
    handler: Option<nwg::EventHandler>,
}

impl Drop for EventHandlerGuard {
    fn drop(&mut self) {
        if let Some(handler) = self.handler.take() {
            nwg::unbind_event_handler(&handler);
        }
    }
}

fn worker_loop(active: Arc<AtomicBool>, config: ResolvedConfig) {
    while active.load(Ordering::SeqCst) {
        if let Some(hwnd) = find_target_window(&config) {
            unsafe {
                SendMessageW(
                    hwnd,
                    WM_ACTIVATE,
                    WPARAM(WA_CLICKACTIVE),
                    LPARAM::default(),
                );
            }
        }
        thread::sleep(Duration::from_millis(REFRESH_INTERVAL_MS));
    }
}

fn find_target_window(config: &ResolvedConfig) -> Option<HWND> {
    if let Some(process_name) = &config.process_name {
        if let Ok(pid) = find_process_id(process_name) {
            if let Some(hwnd) = find_window_by_pid(pid) {
                return Some(hwnd);
            }
        }
    }
    if let Some(window_title) = &config.window_title {
        find_window_by_title(window_title)
    } else {
        None
    }
}

fn find_process_id(process_name: &str) -> Result<u32> {
    let snapshot =
        unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }.context("snapshot failed")?;

    let mut entry = PROCESSENTRY32W::default();
    entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;

    let mut pid = None;
    unsafe {
        if Process32FirstW(snapshot, &mut entry).is_ok() {
            loop {
                let exe_name = wide_ptr_to_string(&entry.szExeFile);
                if exe_name.eq_ignore_ascii_case(process_name) {
                    pid = Some(entry.th32ProcessID);
                    break;
                }

                if Process32NextW(snapshot, &mut entry).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snapshot);
    }

    pid.context(format!("process {} not found", process_name))
}

fn find_window_by_pid(pid: u32) -> Option<HWND> {
    struct SearchContext {
        target_pid: u32,
        found: Option<HWND>,
    }

    unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let ctx = unsafe { &mut *(lparam.0 as *mut SearchContext) };
        let mut window_pid = 0;
        unsafe {
            GetWindowThreadProcessId(hwnd, Some(&mut window_pid));
            if window_pid == ctx.target_pid && IsWindowVisible(hwnd).as_bool() {
                if GetWindowTextLengthW(hwnd) > 0 {
                    ctx.found = Some(hwnd);
                    return BOOL(0);
                }
            }
        }
        BOOL(1)
    }

    let mut context = SearchContext {
        target_pid: pid,
        found: None,
    };
    let ctx_ptr: *mut SearchContext = &mut context;
    let param = LPARAM(ctx_ptr as isize);
    unsafe {
        let _ = EnumWindows(Some(enum_proc), param);
    }
    context.found
}

fn find_window_by_title(title: &str) -> Option<HWND> {
    let wide = to_wide(title);
    match unsafe { FindWindowW(None, PCWSTR(wide.as_ptr())) } {
        Ok(hwnd) if !hwnd.0.is_null() => Some(hwnd),
        _ => None,
    }
}

fn to_wide(value: &str) -> Vec<u16> {
    OsStr::new(value)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

fn wide_ptr_to_string(buffer: &[u16]) -> String {
    let len = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
    String::from_utf16_lossy(&buffer[..len])
}

fn hide_console_window() {
    unsafe {
        let hwnd = GetConsoleWindow();
        if !hwnd.0.is_null() {
            let _ = ShowWindow(hwnd, SW_HIDE);
        }
    }
}

fn ensure_admin() -> Result<()> {
    if is_elevated()? {
        return Ok(());
    }
    relaunch_as_admin()
}

fn is_elevated() -> Result<bool> {
    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token)
            .context("OpenProcessToken failed")?;

        let mut elevation = TOKEN_ELEVATION::default();
        let mut size = std::mem::size_of::<TOKEN_ELEVATION>() as u32;
        let info_result = GetTokenInformation(
            token,
            TokenElevation,
            Some((&mut elevation as *mut TOKEN_ELEVATION).cast::<c_void>()),
            size,
            &mut size,
        );
        let _ = CloseHandle(token);

        info_result.context("GetTokenInformation failed")?;
        Ok(elevation.TokenIsElevated != 0)
    }
}

fn relaunch_as_admin() -> Result<()> {
    let exe = std::env::current_exe().context("failed to determine executable path")?;
    let args: Vec<String> = std::env::args().skip(1).collect();
    let quoted_args: Vec<String> = args.iter().map(|a| quote_argument(a)).collect();
    let params = quoted_args.join(" ");

    let exe_w = exe.as_os_str().encode_wide().chain(std::iter::once(0)).collect::<Vec<_>>();
    let params_w = params.encode_utf16().chain(std::iter::once(0)).collect::<Vec<_>>();

    let result = unsafe {
        ShellExecuteW(
            None,
            w!("runas"),
            PCWSTR(exe_w.as_ptr()),
            if params.is_empty() {
                PCWSTR::null()
            } else {
                PCWSTR(params_w.as_ptr())
            },
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };

    if (result.0 as isize) <= 32 {
        return Err(anyhow!("failed to request elevation (ShellExecuteW error code {})", result.0 as isize));
    }

    std::process::exit(0);
}

fn quote_argument(arg: &str) -> String {
    if arg.is_empty() || arg.chars().any(|c| c == ' ' || c == '\t' || c == '"') {
        let mut escaped = String::from("\"");
        let mut backslashes = 0;
        for ch in arg.chars() {
            match ch {
                '\\' => {
                    backslashes += 1;
                }
                '"' => {
                    escaped.push_str(&"\\".repeat(backslashes * 2 + 1));
                    escaped.push('"');
                    backslashes = 0;
                }
                _ => {
                    if backslashes > 0 {
                        escaped.push_str(&"\\".repeat(backslashes));
                        backslashes = 0;
                    }
                    escaped.push(ch);
                }
            }
        }
        if backslashes > 0 {
            escaped.push_str(&"\\".repeat(backslashes * 2));
        }
        escaped.push('"');
        escaped
    } else {
        arg.to_string()
    }
}

fn normalize_filter(value: String) -> String {
    value.trim().to_string()
}
