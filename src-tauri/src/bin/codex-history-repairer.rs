#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(not(target_os = "windows"))]
fn main() {
    eprintln!("Codex History Repairer GUI is currently available on Windows.");
}

#[cfg(target_os = "windows")]
mod windows_gui {
    use cc_switch_lib::codex_history_migration::{
        repair_codex_history_visibility_standalone, CodexHistoryStandaloneRepairOptions,
        CodexHistoryVisibilityRepairOutcome,
    };
    use std::ffi::{c_void, OsStr};
    use std::iter;
    use std::os::windows::ffi::OsStrExt;
    use std::ptr;
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::Graphics::Gdi::{
        GetStockObject, UpdateWindow, COLOR_WINDOW, DEFAULT_GUI_FONT,
    };
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::Controls::BST_CHECKED;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::EnableWindow;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DispatchMessageW, GetDlgItem, GetMessageW,
        GetWindowLongPtrW, GetWindowTextLengthW, GetWindowTextW, LoadCursorW, PostMessageW,
        PostQuitMessage, RegisterClassW, SendMessageW, SetWindowLongPtrW, SetWindowTextW,
        ShowWindow, TranslateMessage, BS_AUTOCHECKBOX, BS_DEFPUSHBUTTON, BS_PUSHBUTTON,
        CREATESTRUCTW, CW_USEDEFAULT, ES_AUTOHSCROLL, ES_AUTOVSCROLL, ES_LEFT, ES_MULTILINE,
        ES_READONLY, ES_WANTRETURN, GWLP_USERDATA, HMENU, IDC_ARROW, MSG, SW_SHOW, WM_APP,
        WM_COMMAND, WM_CREATE, WM_DESTROY, WM_NCCREATE, WM_NCDESTROY, WM_SETFONT, WNDCLASSW,
        WS_BORDER, WS_CHILD, WS_OVERLAPPEDWINDOW, WS_TABSTOP, WS_VISIBLE, WS_VSCROLL,
    };

    const ID_CODEX_HOME: i32 = 1001;
    const ID_STATE_DB: i32 = 1002;
    const ID_PROJECT_PATH: i32 = 1003;
    const ID_TARGET_PROVIDER: i32 = 1004;
    const ID_SOURCE_PROVIDERS: i32 = 1005;
    const ID_COUNT: i32 = 1006;
    const ID_WINDOW_LIMIT: i32 = 1007;
    const ID_BALANCE_RECENT_WINDOW: i32 = 1014;
    const ID_MAX_PER_PROJECT: i32 = 1015;
    const ID_MAX_TOTAL: i32 = 1016;
    const ID_SOURCE_FILTER: i32 = 1017;
    const ID_INCLUDE_ARCHIVED: i32 = 1008;
    const ID_INCLUDE_SUBAGENTS: i32 = 1009;
    const ID_FORCE: i32 = 1010;
    const ID_PREVIEW: i32 = 1011;
    const ID_APPLY: i32 = 1012;
    const ID_OUTPUT: i32 = 1013;
    const WM_REPAIR_DONE: u32 = WM_APP + 41;

    /// 保存窗口控件句柄，按钮状态和输出区域由主线程统一更新。
    struct GuiState {
        hwnd: HWND,
        preview_button: HWND,
        apply_button: HWND,
        output: HWND,
    }

    impl GuiState {
        /// 构造空状态；真实 HWND 会在 `WM_CREATE` 后填入。
        fn new() -> Self {
            Self {
                hwnd: ptr::null_mut(),
                preview_button: ptr::null_mut(),
                apply_button: ptr::null_mut(),
                output: ptr::null_mut(),
            }
        }

        /// 创建并排布所有原生 Windows 控件。
        unsafe fn create_controls(&mut self, hwnd: HWND) {
            self.hwnd = hwnd;
            let font = GetStockObject(DEFAULT_GUI_FONT) as WPARAM;
            let codex_home = std::env::var("USERPROFILE")
                .map(|home| format!(r"{home}\.codex"))
                .unwrap_or_else(|_| r"%USERPROFILE%\.codex".to_string());
            let project_path = std::env::current_dir()
                .ok()
                .map(|path| path.to_string_lossy().to_string())
                .unwrap_or_default();

            create_label(hwnd, "Codex Home", 16, 18, 110, 22, font);
            create_edit(hwnd, ID_CODEX_HOME, &codex_home, 150, 16, 590, 24, font);
            create_label(hwnd, "State DB (可空)", 16, 52, 120, 22, font);
            create_edit(hwnd, ID_STATE_DB, "", 150, 50, 590, 24, font);
            create_label(hwnd, "项目路径", 16, 86, 110, 22, font);
            create_edit(hwnd, ID_PROJECT_PATH, &project_path, 150, 84, 590, 24, font);
            create_label(hwnd, "目标 provider", 16, 120, 120, 22, font);
            create_edit(hwnd, ID_TARGET_PROVIDER, "", 150, 118, 220, 24, font);
            create_label(hwnd, "空值跟随 config.toml", 386, 120, 170, 22, font);
            create_label(hwnd, "source providers", 16, 154, 120, 22, font);
            create_edit(
                hwnd,
                ID_SOURCE_PROVIDERS,
                "openai, custom, codex_model_router_v2, cc_switch_codex_router, codex_model_router",
                150,
                152,
                590,
                24,
                font,
            );
            create_label(hwnd, "置顶数量", 16, 188, 80, 22, font);
            create_edit(hwnd, ID_COUNT, "30", 150, 186, 80, 24, font);
            create_label(hwnd, "窗口范围", 250, 188, 80, 22, font);
            create_edit(hwnd, ID_WINDOW_LIMIT, "80", 330, 186, 80, 24, font);
            create_label(hwnd, "Source", 16, 222, 80, 22, font);
            create_edit(hwnd, ID_SOURCE_FILTER, "vscode", 150, 220, 90, 24, font);
            let balance_checkbox = create_checkbox(
                hwnd,
                ID_BALANCE_RECENT_WINDOW,
                "Balance recent",
                260,
                220,
                130,
                24,
                font,
            );
            set_checked(balance_checkbox, true);
            create_label(hwnd, "Max/project", 408, 222, 86, 22, font);
            create_edit(hwnd, ID_MAX_PER_PROJECT, "10", 500, 220, 48, 24, font);
            create_label(hwnd, "Max total", 562, 222, 74, 22, font);
            create_edit(hwnd, ID_MAX_TOTAL, "300", 640, 220, 64, 24, font);
            create_checkbox(
                hwnd,
                ID_INCLUDE_ARCHIVED,
                "包含归档",
                440,
                186,
                96,
                24,
                font,
            );
            create_checkbox(
                hwnd,
                ID_INCLUDE_SUBAGENTS,
                "包含 subagent",
                540,
                186,
                118,
                24,
                font,
            );
            create_checkbox(
                hwnd,
                ID_FORCE,
                "Codex 运行时强制写入",
                16,
                256,
                180,
                24,
                font,
            );

            self.preview_button =
                create_button(hwnd, ID_PREVIEW, "预览修复", 210, 254, 120, 30, font);
            self.apply_button = create_button(hwnd, ID_APPLY, "确认写入", 344, 254, 120, 30, font);
            self.output = create_output(hwnd, ID_OUTPUT, 16, 300, 724, 300, font);
            set_text(
                self.output,
                "先执行“预览修复”，确认 active DB、provider、session_index、workspace hints 和 focus 计数后再写入。\r\n写入前会创建本地备份。",
            );
        }

        /// 禁用或恢复按钮，避免同一时间执行多次修复。
        unsafe fn set_busy(&self, busy: bool) {
            EnableWindow(self.preview_button, (!busy) as i32);
            EnableWindow(self.apply_button, (!busy) as i32);
        }
    }

    /// 启动 Windows 消息循环。
    pub fn run() {
        unsafe {
            let instance = GetModuleHandleW(ptr::null());
            let class_name = wide("CodexHistoryRepairerWindow");
            let window_class = WNDCLASSW {
                style: 0,
                lpfnWndProc: Some(window_proc),
                cbClsExtra: 0,
                cbWndExtra: 0,
                hInstance: instance,
                hIcon: ptr::null_mut(),
                hCursor: LoadCursorW(ptr::null_mut(), IDC_ARROW),
                hbrBackground: (COLOR_WINDOW + 1) as usize as _,
                lpszMenuName: ptr::null(),
                lpszClassName: class_name.as_ptr(),
            };
            RegisterClassW(&window_class);

            let state = Box::new(GuiState::new());
            let state_ptr = Box::into_raw(state);
            let hwnd = CreateWindowExW(
                0,
                class_name.as_ptr(),
                wide("Codex 历史修复工具").as_ptr(),
                WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                780,
                660,
                ptr::null_mut(),
                ptr::null_mut(),
                instance,
                state_ptr.cast::<c_void>(),
            );
            if hwnd.is_null() {
                let _ = Box::from_raw(state_ptr);
                return;
            }
            ShowWindow(hwnd, SW_SHOW);
            UpdateWindow(hwnd);

            let mut message = MSG::default();
            while GetMessageW(&mut message, ptr::null_mut(), 0, 0) > 0 {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
    }

    /// Windows 窗口过程，负责创建控件、响应按钮和接收后台修复结果。
    unsafe extern "system" fn window_proc(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match message {
            WM_NCCREATE => {
                let create = lparam as *const CREATESTRUCTW;
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, (*create).lpCreateParams as isize);
                1
            }
            WM_CREATE => {
                if let Some(state) = state_from_hwnd(hwnd) {
                    state.create_controls(hwnd);
                }
                0
            }
            WM_COMMAND => {
                let command_id = (wparam & 0xffff) as i32;
                if command_id == ID_PREVIEW || command_id == ID_APPLY {
                    if let Some(state) = state_from_hwnd(hwnd) {
                        start_repair(state, command_id == ID_PREVIEW);
                    }
                }
                0
            }
            WM_REPAIR_DONE => {
                if let Some(state) = state_from_hwnd(hwnd) {
                    let output = Box::from_raw(lparam as *mut String);
                    set_text(state.output, &output);
                    state.set_busy(false);
                }
                0
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                0
            }
            WM_NCDESTROY => {
                let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut GuiState;
                if !ptr.is_null() {
                    SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                    drop(Box::from_raw(ptr));
                }
                DefWindowProcW(hwnd, message, wparam, lparam)
            }
            _ => DefWindowProcW(hwnd, message, wparam, lparam),
        }
    }

    /// 从窗口用户数据里取回 GUI 状态。
    unsafe fn state_from_hwnd(hwnd: HWND) -> Option<&'static mut GuiState> {
        let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut GuiState;
        (!ptr.is_null()).then(|| &mut *ptr)
    }

    /// 收集表单参数并在后台线程执行修复。
    unsafe fn start_repair(state: &mut GuiState, dry_run: bool) {
        state.set_busy(true);
        set_text(
            state.output,
            if dry_run {
                "正在预览..."
            } else {
                "正在写入..."
            },
        );
        let hwnd = state.hwnd;
        let hwnd_value = hwnd as isize;
        let options = CodexHistoryStandaloneRepairOptions {
            dry_run,
            codex_home: empty_to_none(get_text(GetDlgItem(hwnd, ID_CODEX_HOME))),
            state_db_path: empty_to_none(get_text(GetDlgItem(hwnd, ID_STATE_DB))),
            project_path: empty_to_none(get_text(GetDlgItem(hwnd, ID_PROJECT_PATH))),
            target_provider: empty_to_none(get_text(GetDlgItem(hwnd, ID_TARGET_PROVIDER))),
            source_provider_ids: parse_source_providers(&get_text(GetDlgItem(
                hwnd,
                ID_SOURCE_PROVIDERS,
            ))),
            count: parse_usize(&get_text(GetDlgItem(hwnd, ID_COUNT)), 30),
            window_limit: parse_usize(&get_text(GetDlgItem(hwnd, ID_WINDOW_LIMIT)), 80),
            balance_recent_window: Some(is_checked(GetDlgItem(hwnd, ID_BALANCE_RECENT_WINDOW))),
            max_per_project: parse_usize(&get_text(GetDlgItem(hwnd, ID_MAX_PER_PROJECT)), 10),
            max_total: parse_usize(&get_text(GetDlgItem(hwnd, ID_MAX_TOTAL)), 300),
            source_filter: empty_to_none(get_text(GetDlgItem(hwnd, ID_SOURCE_FILTER))),
            include_archived: Some(is_checked(GetDlgItem(hwnd, ID_INCLUDE_ARCHIVED))),
            include_subagents: Some(is_checked(GetDlgItem(hwnd, ID_INCLUDE_SUBAGENTS))),
            skip_provider_bucket_sync: Some(false),
            force_while_codex_running: Some(is_checked(GetDlgItem(hwnd, ID_FORCE))),
        };

        std::thread::spawn(move || {
            let text = match repair_codex_history_visibility_standalone(options) {
                Ok(outcome) => format_outcome(&outcome),
                Err(error) => format!("修复失败：{error}"),
            };
            let boxed = Box::new(text);
            let ptr = Box::into_raw(boxed);
            let hwnd = hwnd_value as HWND;
            PostMessageW(hwnd, WM_REPAIR_DONE, 0, ptr as LPARAM);
        });
    }

    /// 把修复结果格式化为便于复制的多行文本。
    fn format_outcome(result: &CodexHistoryVisibilityRepairOutcome) -> String {
        [
            format!(
                "模式: {}",
                if result.dry_run {
                    "预览"
                } else {
                    "已写入"
                }
            ),
            format!("Codex Home: {}", result.codex_home),
            format!(
                "Active DB: {} ({})",
                result.state_db_path.as_deref().unwrap_or("未找到"),
                result.active_db_kind.as_deref().unwrap_or("-")
            ),
            format!(
                "Live provider: {}",
                result
                    .live_config_model_provider
                    .as_deref()
                    .unwrap_or("未读取到")
            ),
            format!("Target provider: {}", result.target_provider),
            format!(
                "Source providers: {}",
                result.source_provider_ids.join(", ")
            ),
            format!("SQLite threads: {}", result.sqlite_threads),
            format!(
                "Provider rows: {} / {}",
                result.provider_rows_updated, result.provider_rows_to_update
            ),
            format!(
                "Rollout provider lines: {} / {}",
                result.rollout_first_lines_updated, result.rollout_first_lines_to_update
            ),
            format!(
                "has_user_event rows: {} / {}",
                result.user_event_rows_updated, result.user_event_rows_to_update
            ),
            format!(
                "session_index append: {} / {}",
                result.session_index_appended, result.session_index_missing_to_append
            ),
            format!(
                "prefix thread cleanup: {} / {}",
                result.prefix_duplicate_rows_removed, result.prefix_duplicate_rows_to_remove
            ),
            format!(
                "prefix index cleanup: {} / {}",
                result.session_index_duplicate_rows_removed,
                result.session_index_duplicate_rows_to_remove
            ),
            format!(
                "focus rows: {} / {}",
                result.sqlite_focus_rows_updated, result.sqlite_focus_rows_to_update
            ),
            format!(
                "balanced recent: {} rows / {} projects / max {} per project / max total {}",
                result.balanced_recent_window_rows,
                result.balanced_recent_window_projects,
                result.max_per_project,
                result.max_total
            ),
            format!(
                "session_index move: {} / {}",
                result.session_index_rows_moved, result.session_index_rows_to_move
            ),
            format!(
                "session_index titles: {} / {}",
                result.session_index_titles_updated, result.session_index_titles_to_update
            ),
            format!(
                "workspace hints: {} / {}",
                result.workspace_hints_fixed, result.workspace_hints_to_fix
            ),
            format!(
                "projectless remove: {} / {}",
                result.projectless_ids_removed, result.projectless_ids_to_remove
            ),
            format!(
                "saved roots: {} / {}",
                result.saved_workspace_roots_added, result.saved_workspace_roots_to_add
            ),
            format!(
                "rollout mtimes: {} / {}",
                result.rollout_mtimes_touched, result.rollout_mtimes_to_touch
            ),
            format!(
                "Visible candidates / project window: {} / {}",
                result.visible_candidate_rows, result.visible_project_rows_in_window_before
            ),
            format!(
                "Source filter: {}",
                result
                    .source_filter
                    .as_deref()
                    .unwrap_or("(default cli/vscode)")
            ),
            format!(
                "Backup: {}",
                result.backup_dir.as_deref().unwrap_or("预览模式未创建")
            ),
            format!(
                "Skipped: {}",
                result.skipped_reason.as_deref().unwrap_or("-")
            ),
        ]
        .join("\r\n")
    }

    /// 创建普通标签控件。
    unsafe fn create_label(hwnd: HWND, text: &str, x: i32, y: i32, w: i32, h: i32, font: WPARAM) {
        let control = CreateWindowExW(
            0,
            wide("STATIC").as_ptr(),
            wide(text).as_ptr(),
            WS_CHILD | WS_VISIBLE,
            x,
            y,
            w,
            h,
            hwnd,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null(),
        );
        SendMessageW(control, WM_SETFONT, font, 1);
    }

    /// 创建单行输入框。
    unsafe fn create_edit(
        hwnd: HWND,
        id: i32,
        text: &str,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        font: WPARAM,
    ) -> HWND {
        let control = CreateWindowExW(
            0,
            wide("EDIT").as_ptr(),
            wide(text).as_ptr(),
            WS_CHILD | WS_VISIBLE | WS_BORDER | ES_LEFT as u32 | ES_AUTOHSCROLL as u32 | WS_TABSTOP,
            x,
            y,
            w,
            h,
            hwnd,
            id as usize as HMENU,
            ptr::null_mut(),
            ptr::null(),
        );
        SendMessageW(control, WM_SETFONT, font, 1);
        control
    }

    /// 创建复选框。
    unsafe fn create_checkbox(
        hwnd: HWND,
        id: i32,
        text: &str,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        font: WPARAM,
    ) -> HWND {
        let control = CreateWindowExW(
            0,
            wide("BUTTON").as_ptr(),
            wide(text).as_ptr(),
            WS_CHILD | WS_VISIBLE | BS_AUTOCHECKBOX as u32 | WS_TABSTOP,
            x,
            y,
            w,
            h,
            hwnd,
            id as usize as HMENU,
            ptr::null_mut(),
            ptr::null(),
        );
        SendMessageW(control, WM_SETFONT, font, 1);
        control
    }

    /// 设置复选框初始状态，用于默认启用最新的 recent-window 平衡修复。
    unsafe fn set_checked(hwnd: HWND, checked: bool) {
        SendMessageW(
            hwnd,
            windows_sys::Win32::UI::WindowsAndMessaging::BM_SETCHECK,
            if checked { BST_CHECKED as usize } else { 0 },
            0,
        );
    }

    /// 创建命令按钮。
    unsafe fn create_button(
        hwnd: HWND,
        id: i32,
        text: &str,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        font: WPARAM,
    ) -> HWND {
        let control = CreateWindowExW(
            0,
            wide("BUTTON").as_ptr(),
            wide(text).as_ptr(),
            WS_CHILD | WS_VISIBLE | BS_PUSHBUTTON as u32 | BS_DEFPUSHBUTTON as u32 | WS_TABSTOP,
            x,
            y,
            w,
            h,
            hwnd,
            id as usize as HMENU,
            ptr::null_mut(),
            ptr::null(),
        );
        SendMessageW(control, WM_SETFONT, font, 1);
        control
    }

    /// 创建多行只读输出框。
    unsafe fn create_output(
        hwnd: HWND,
        id: i32,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        font: WPARAM,
    ) -> HWND {
        let control = CreateWindowExW(
            0,
            wide("EDIT").as_ptr(),
            wide("").as_ptr(),
            WS_CHILD
                | WS_VISIBLE
                | WS_BORDER
                | WS_VSCROLL
                | ES_LEFT as u32
                | ES_MULTILINE as u32
                | ES_AUTOVSCROLL as u32
                | ES_WANTRETURN as u32
                | ES_READONLY as u32,
            x,
            y,
            w,
            h,
            hwnd,
            id as usize as HMENU,
            ptr::null_mut(),
            ptr::null(),
        );
        SendMessageW(control, WM_SETFONT, font, 1);
        control
    }

    /// 读取控件文本。
    unsafe fn get_text(hwnd: HWND) -> String {
        let len = GetWindowTextLengthW(hwnd);
        let mut buffer = vec![0u16; len as usize + 1];
        let read = GetWindowTextW(hwnd, buffer.as_mut_ptr(), buffer.len() as i32);
        String::from_utf16_lossy(&buffer[..read as usize])
    }

    /// 写入控件文本。
    unsafe fn set_text(hwnd: HWND, text: &str) {
        SetWindowTextW(hwnd, wide(text).as_ptr());
    }

    /// 判断复选框是否勾选。
    unsafe fn is_checked(hwnd: HWND) -> bool {
        SendMessageW(
            hwnd,
            windows_sys::Win32::UI::WindowsAndMessaging::BM_GETCHECK,
            0,
            0,
        ) == BST_CHECKED as isize
    }

    /// 空字符串转为 None。
    fn empty_to_none(value: String) -> Option<String> {
        let trimmed = value.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_string())
    }

    /// 解析用逗号分隔的 source provider 列表。
    fn parse_source_providers(value: &str) -> Option<Vec<String>> {
        let values = value
            .split(',')
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(str::to_string)
            .collect::<Vec<_>>();
        (!values.is_empty()).then_some(values)
    }

    /// 解析正整数，失败时回退到默认值。
    fn parse_usize(value: &str, fallback: usize) -> Option<usize> {
        Some(value.trim().parse::<usize>().unwrap_or(fallback))
    }

    /// 转换 UTF-16 Windows 字符串。
    fn wide(value: &str) -> Vec<u16> {
        OsStr::new(value)
            .encode_wide()
            .chain(iter::once(0))
            .collect()
    }
}

#[cfg(target_os = "windows")]
fn main() {
    windows_gui::run();
}
