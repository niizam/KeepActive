use std::{
    cell::RefCell,
    collections::HashSet,
    env,
    ffi::{c_void, OsStr},
    io::{self, Write},
    os::windows::ffi::OsStrExt,
    os::windows::process::CommandExt,
    process::{Child, Command, Stdio},
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
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Parser, Debug)]
#[command(author, version, about = "KeepActive - keep a target window in the foreground")]
struct Args {
    /// Run the application in console/CLI mode
    #[arg(long)]
    cli: bool,

    /// Internal flag: run as a background worker for a single target
    #[arg(long, hide = true)]
    worker: bool,

    /// Window titles to target (repeatable; fallback list if processes are not found)
    #[arg(short = 'w', long = "window", value_name = "TITLE", action = clap::ArgAction::Append)]
    window: Vec<String>,

    /// Executable names to target (repeatable, e.g. notepad.exe)
    #[arg(short = 'e', long = "exe", value_name = "NAME", action = clap::ArgAction::Append)]
    exe: Vec<String>,
}

#[derive(Clone, Debug)]
struct AppConfig {
    window_titles: Vec<String>,
    process_names: Vec<String>,
}

impl AppConfig {
    fn from_args(args: &Args) -> Self {
        let mut window_titles = normalize_list(args.window.clone());
        if window_titles.is_empty() {
            window_titles.push(DEFAULT_WINDOW_TITLE.to_string());
        }
        let process_names = normalize_list(args.exe.clone());
        Self {
            window_titles,
            process_names,
        }
    }

    fn resolved(&self) -> ResolvedConfig {
        ResolvedConfig::from_lists(self.window_titles.clone(), self.process_names.clone())
    }
}

#[derive(Clone, Debug)]
struct ResolvedConfig {
    window_titles: Vec<String>,
    process_names: Vec<String>,
}

impl ResolvedConfig {
    fn from_lists(window_titles: Vec<String>, process_names: Vec<String>) -> Self {
        let mut window_titles = normalize_list(window_titles);
        if window_titles.is_empty() {
            window_titles.push(DEFAULT_WINDOW_TITLE.to_string());
        }
        let process_names = normalize_list(process_names);
        Self { window_titles, process_names }
    }
}

struct KeepAliveController {
    children: Vec<Child>,
}

impl KeepAliveController {
    fn new() -> Self {
        Self { children: Vec::new() }
    }

    fn start(&mut self, config: ResolvedConfig) -> Result<()> {
        self.prune_finished();
        if !self.children.is_empty() {
            return Ok(());
        }

        let ResolvedConfig {
            window_titles,
            process_names,
        } = config;

        let window_titles = normalize_list(window_titles);
        let process_names = normalize_list(process_names);

        if window_titles.is_empty() && process_names.is_empty() {
            return Err(anyhow!("no targets configured"));
        }

        let exe_path = env::current_exe().context("failed to locate KeepActive executable")?;

        let mut children = Vec::new();

        for title in &window_titles {
            let mut cmd = Command::new(&exe_path);
            cmd.arg("--worker").arg("--window").arg(title);
            cmd.stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .creation_flags(CREATE_NO_WINDOW);
            let child = cmd
                .spawn()
                .with_context(|| format!("failed to launch worker for window '{}'", title))?;
            children.push(child);
        }

        for name in &process_names {
            let mut cmd = Command::new(&exe_path);
            cmd.arg("--worker");
            for title in &window_titles {
                cmd.arg("--window").arg(title);
            }
            cmd.arg("--exe").arg(name);
            cmd.stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .creation_flags(CREATE_NO_WINDOW);
            let child = cmd
                .spawn()
                .with_context(|| format!("failed to launch worker for executable '{}'", name))?;
            children.push(child);
        }

        self.children = children;
        Ok(())
    }

    fn stop(&mut self) -> Result<()> {
        for mut child in self.children.drain(..) {
            if let Err(err) = child.kill() {
                if err.kind() != io::ErrorKind::InvalidInput {
                    return Err(err.into());
                }
            }
            let _ = child.wait();
        }
        Ok(())
    }

    fn is_running(&mut self) -> bool {
        self.prune_finished();
        !self.children.is_empty()
    }

    fn prune_finished(&mut self) {
        let mut active_children = Vec::new();
        for mut child in self.children.drain(..) {
            match child.try_wait() {
                Ok(Some(_status)) => {
                    // child finished; drop it
                }
                Ok(None) | Err(_) => active_children.push(child),
            }
        }
        self.children = active_children;
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
    if args.worker || !args.cli {
        hide_console_window();
    }

    let config = AppConfig::from_args(&args);
    if args.worker {
        run_worker(config.resolved())?;
    } else if args.cli {
        run_cli(config)?;
    } else {
        run_gui(config)?;
    }
    Ok(())
}

fn run_worker(config: ResolvedConfig) -> Result<()> {
    let active = Arc::new(AtomicBool::new(true));
    worker_loop(active, config);
    Ok(())
}

fn run_cli(config: AppConfig) -> Result<()> {
    println!("KeepActive - Rust CLI");
    let exe_display = if config.process_names.is_empty() {
        "not set".to_string()
    } else {
        config.process_names.join(", ")
    };
    let window_display = if config.window_titles.is_empty() {
        "not set".to_string()
    } else {
        config.window_titles.join(", ")
    };
    println!("Target executables: {}", exe_display);
    println!("Fallback window titles: {}", window_display);
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
        .size((420, 520))
        .title("KeepActive")
        .build(&mut window)
        .context("failed to build main window")?;

    let mut _window_label = nwg::Label::default();
    nwg::Label::builder()
        .text("Window Titles")
        .position((20, 20))
        .size((180, 24))
        .parent(&window)
        .build(&mut _window_label)
        .context("failed to build window label")?;

    let mut window_list: nwg::ListBox<String> = Default::default();
    nwg::ListBox::builder()
        .collection(config.window_titles.clone())
        .position((20, 48))
        .size((360, 110))
        .parent(&window)
        .build(&mut window_list)
        .context("failed to build window list box")?;
    let window_list = Rc::new(window_list);

    let mut window_remove_btn = nwg::Button::default();
    nwg::Button::builder()
        .text("Remove")
        .position((320, 166))
        .size((60, 28))
        .parent(&window)
        .build(&mut window_remove_btn)
        .context("failed to build window remove button")?;
    let window_remove_btn = Rc::new(window_remove_btn);

    let mut _exe_label = nwg::Label::default();
    nwg::Label::builder()
        .text("Executable Names (optional)")
        .position((20, 204))
        .size((200, 24))
        .parent(&window)
        .build(&mut _exe_label)
        .context("failed to build process label")?;

    let mut exe_list: nwg::ListBox<String> = Default::default();
    nwg::ListBox::builder()
        .collection(config.process_names.clone())
        .position((20, 200))
        .size((360, 110))
        .parent(&window)
        .build(&mut exe_list)
        .context("failed to build process list box")?;
    let exe_list = Rc::new(exe_list);

    let mut exe_remove_btn = nwg::Button::default();
    nwg::Button::builder()
        .text("Remove")
        .position((320, 318))
        .size((60, 28))
        .parent(&window)
        .build(&mut exe_remove_btn)
        .context("failed to build process remove button")?;
    let exe_remove_btn = Rc::new(exe_remove_btn);

    let mut _target_label = nwg::Label::default();
    nwg::Label::builder()
        .text("Add target (.exe -> executable list)")
        .position((20, 332))
        .size((280, 24))
        .parent(&window)
        .build(&mut _target_label)
        .context("failed to build target label")?;

    let mut target_entry = nwg::TextInput::default();
    nwg::TextInput::builder()
        .text("")
        .position((20, 360))
        .size((220, 28))
        .parent(&window)
        .build(&mut target_entry)
        .context("failed to build target entry input")?;
    let target_entry = Rc::new(target_entry);

    let mut add_btn = nwg::Button::default();
    nwg::Button::builder()
        .text("Add Target")
        .position((250, 360))
        .size((130, 28))
        .parent(&window)
        .build(&mut add_btn)
        .context("failed to build add target button")?;
    let add_btn = Rc::new(add_btn);

    let mut status_label = nwg::Label::default();
    nwg::Label::builder()
        .text("Status: Not running")
        .position((20, 404))
        .size((360, 24))
        .parent(&window)
        .build(&mut status_label)
        .context("failed to build status label")?;
    let status_label = Rc::new(status_label);

    let mut start_btn = nwg::Button::default();
    nwg::Button::builder()
        .text("Start")
        .position((20, 440))
        .size((160, 32))
        .parent(&window)
        .build(&mut start_btn)
        .context("failed to build start button")?;
    let start_btn = Rc::new(start_btn);

    let mut stop_btn = nwg::Button::default();
    nwg::Button::builder()
        .text("Stop")
        .enabled(false)
        .position((220, 440))
        .size((160, 32))
        .parent(&window)
        .build(&mut stop_btn)
        .context("failed to build stop button")?;
    let stop_btn = Rc::new(stop_btn);

    struct GuiState {
        window_list: Rc<nwg::ListBox<String>>,
        window_remove_btn: Rc<nwg::Button>,
        exe_list: Rc<nwg::ListBox<String>>,
        exe_remove_btn: Rc<nwg::Button>,
        target_entry: Rc<nwg::TextInput>,
        add_btn: Rc<nwg::Button>,
        status_label: Rc<nwg::Label>,
        start_btn: Rc<nwg::Button>,
        stop_btn: Rc<nwg::Button>,
    }

    let controller = Rc::new(RefCell::new(KeepAliveController::new()));
    let state = Rc::new(GuiState {
        window_list,
        window_remove_btn,
        exe_list,
        exe_remove_btn,
        target_entry,
        add_btn,
        status_label,
        start_btn,
        stop_btn,
    });

    let ui_state = Rc::clone(&state);
    let controller = Rc::clone(&controller);
    let handler = nwg::full_bind_event_handler(&window.handle, move |evt, _, handle| {
        use nwg::Event;
        let mut alert: Option<String> = None;

        match evt {
            Event::OnButtonClick => {
                if handle == ui_state.start_btn.handle {
                    let window_titles = {
                        let col = ui_state.window_list.collection();
                        col.iter().cloned().collect::<Vec<_>>()
                    };
                    let process_names = {
                        let col = ui_state.exe_list.collection();
                        col.iter().cloned().collect::<Vec<_>>()
                    };

                    let config = ResolvedConfig::from_lists(window_titles, process_names);

                    match controller.borrow_mut().start(config) {
                        Ok(()) => {
                            ui_state.status_label.set_text("Status: Running");
                            ui_state.start_btn.set_enabled(false);
                            ui_state.stop_btn.set_enabled(true);
                        }
                        Err(err) => {
                            let message = format!("Error: {}", err);
                            ui_state
                                .status_label
                                .set_text(&format!("Status: {}", message));
                            alert = Some(message);
                        }
                    }
                } else if handle == ui_state.stop_btn.handle {
                    match controller.borrow_mut().stop() {
                        Ok(()) => {
                            ui_state.status_label.set_text("Status: Not running");
                            ui_state.start_btn.set_enabled(true);
                            ui_state.stop_btn.set_enabled(false);
                        }
                        Err(err) => {
                            let message = format!("Error: {}", err);
                            ui_state
                                .status_label
                                .set_text(&format!("Status: {}", message));
                            alert = Some(message);
                        }
                    }
                } else if handle == ui_state.add_btn.handle {
                    let entry_text = ui_state.target_entry.text();
                    let trimmed = entry_text.trim();
                    if trimmed.is_empty() {
                        ui_state.target_entry.set_text("");
                    } else {
                        let entry_owned = trimmed.to_string();
                        let lower = entry_owned.to_ascii_lowercase();
                        if lower.ends_with(".exe") {
                            let mut index = None;
                            {
                                let col = ui_state.exe_list.collection();
                                for (i, value) in col.iter().enumerate() {
                                    if value.eq_ignore_ascii_case(&entry_owned) {
                                        index = Some(i);
                                        break;
                                    }
                                }
                            }
                            if index.is_none() {
                                ui_state.exe_list.push(entry_owned.clone());
                                index = Some(ui_state.exe_list.len().saturating_sub(1));
                            }
                            if let Some(i) = index {
                                ui_state.exe_list.set_selection(Some(i));
                            }
                        } else {
                            let mut index = None;
                            {
                                let col = ui_state.window_list.collection();
                                for (i, value) in col.iter().enumerate() {
                                    if value.eq_ignore_ascii_case(&entry_owned) {
                                        index = Some(i);
                                        break;
                                    }
                                }
                            }
                            if index.is_none() {
                                ui_state.window_list.push(entry_owned.clone());
                                index = Some(ui_state.window_list.len().saturating_sub(1));
                            }
                            if let Some(i) = index {
                                ui_state.window_list.set_selection(Some(i));
                            }
                        }
                        ui_state.target_entry.set_text("");
                    }
                } else if handle == ui_state.window_remove_btn.handle {
                    if let Some(index) = ui_state.window_list.selection() {
                        ui_state.window_list.remove(index);
                    }
                } else if handle == ui_state.exe_remove_btn.handle {
                    if let Some(index) = ui_state.exe_list.selection() {
                        ui_state.exe_list.remove(index);
                    }
                }
            }
            Event::OnWindowClose => {
                controller.borrow_mut().stop().ok();
                nwg::stop_thread_dispatch();
            }
            _ => {}
        }

        if let Some(message) = alert {
            nwg::simple_message("KeepActive error", &message);
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
    for process_name in &config.process_names {
        if let Ok(pid) = find_process_id(process_name) {
            if let Some(hwnd) = find_window_by_pid(pid) {
                return Some(hwnd);
            }
        }
    }
    for window_title in &config.window_titles {
        if let Some(hwnd) = find_window_by_title(window_title) {
            return Some(hwnd);
        }
    }
    None
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

fn normalize_list(values: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut result = Vec::new();
    for value in values {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            continue;
        }
        let owned = trimmed.to_string();
        let key = owned.to_ascii_lowercase();
        if seen.insert(key) {
            result.push(owned);
        }
    }
    result
}
