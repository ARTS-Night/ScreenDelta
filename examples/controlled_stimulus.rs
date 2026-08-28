//! Deterministic desktop-damage stimulus for interactive Windows benchmarks.
//!
//! `cargo run --release --example controlled_stimulus -- small 12`
//! runs one named workload for a fixed number of seconds and closes itself.

use std::sync::atomic::{AtomicUsize, Ordering};

use windows::{
    Win32::{
        Foundation::{COLORREF, E_INVALIDARG, HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM},
        Graphics::Gdi::{
            BeginPaint, CreateSolidBrush, DeleteObject, EndPaint, FillRect, HGDIOBJ,
            InvalidateRect, PAINTSTRUCT,
        },
        System::LibraryLoader::GetModuleHandleW,
        UI::WindowsAndMessaging::{
            CS_HREDRAW, CS_VREDRAW, CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW,
            MSG, PostQuitMessage, RegisterClassW, SW_SHOW, SWP_NOSIZE, SWP_NOZORDER, SetCursorPos,
            SetTimer, SetWindowPos, ShowWindow, TranslateMessage, WM_DESTROY, WM_PAINT, WM_TIMER,
            WNDCLASSW, WS_OVERLAPPEDWINDOW,
        },
    },
    core::w,
};

const STATIC: usize = 0;
const CURSOR: usize = 1;
const SMALL: usize = 2;
const TYPING: usize = 3;
const SCROLL: usize = 4;
const WINDOW_MOVE: usize = 5;
const FULL_MOTION: usize = 6;

static SCENARIO: AtomicUsize = AtomicUsize::new(STATIC);
static TICK: AtomicUsize = AtomicUsize::new(0);
static LIMIT: AtomicUsize = AtomicUsize::new(300);

fn main() -> windows::core::Result<()> {
    let (scenario, seconds) = parse_args()?;
    SCENARIO.store(scenario, Ordering::Relaxed);
    LIMIT.store(seconds.saturating_mul(30), Ordering::Relaxed);
    let instance = unsafe { GetModuleHandleW(None)? };
    let class = w!("ScreenDeltaControlledStimulus");
    let window_class = WNDCLASSW {
        hInstance: HINSTANCE(instance.0),
        lpszClassName: class,
        lpfnWndProc: Some(window_proc),
        style: CS_HREDRAW | CS_VREDRAW,
        ..Default::default()
    };
    unsafe { RegisterClassW(&window_class) };
    let hwnd = unsafe {
        CreateWindowExW(
            Default::default(),
            class,
            w!("ScreenDelta controlled workload"),
            WS_OVERLAPPEDWINDOW,
            120,
            80,
            1100,
            650,
            None,
            None,
            Some(HINSTANCE(instance.0)),
            None,
        )?
    };
    let _ = unsafe { ShowWindow(hwnd, SW_SHOW) };
    unsafe { SetTimer(Some(hwnd), 1, 33, None) };
    let mut message = MSG::default();
    while unsafe { GetMessageW(&mut message, None, 0, 0) }.as_bool() {
        let _ = unsafe { TranslateMessage(&message) };
        unsafe { DispatchMessageW(&message) };
    }
    Ok(())
}

fn parse_args() -> windows::core::Result<(usize, usize)> {
    let name = std::env::args().nth(1).unwrap_or_else(|| "small".into());
    let scenario = match name.as_str() {
        "static" => STATIC,
        "cursor" => CURSOR,
        "small" => SMALL,
        "typing" => TYPING,
        "scroll" => SCROLL,
        "window-move" => WINDOW_MOVE,
        "full" => FULL_MOTION,
        _ => {
            return Err(windows::core::Error::new(
                E_INVALIDARG,
                "scenario: static | cursor | small | typing | scroll | window-move | full",
            ));
        }
    };
    let seconds = std::env::args()
        .nth(2)
        .and_then(|value| value.parse().ok())
        .unwrap_or(12);
    Ok((scenario, seconds))
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_TIMER => {
            let tick = TICK.fetch_add(1, Ordering::Relaxed);
            if tick >= LIMIT.load(Ordering::Relaxed) {
                unsafe { PostQuitMessage(0) };
                return LRESULT(0);
            }
            match SCENARIO.load(Ordering::Relaxed) {
                STATIC => {}
                CURSOR => {
                    let x = 200 + (tick % 500) as i32;
                    let y = 200 + ((tick * 3) % 250) as i32;
                    let _ = unsafe { SetCursorPos(x, y) };
                }
                WINDOW_MOVE => {
                    let x = 80 + (tick % 200) as i32;
                    let y = 80 + ((tick * 2) % 120) as i32;
                    let _ =
                        unsafe { SetWindowPos(hwnd, None, x, y, 0, 0, SWP_NOSIZE | SWP_NOZORDER) };
                }
                FULL_MOTION => {
                    let _ = unsafe { InvalidateRect(Some(hwnd), None, false) };
                }
                scenario => {
                    let x = 40 + ((tick * 13) % 900) as i32;
                    let width = if scenario == SCROLL { 750 } else { 40 };
                    let rect = RECT {
                        left: x,
                        top: 180,
                        right: x + width,
                        bottom: if scenario == TYPING { 230 } else { 260 },
                    };
                    let _ = unsafe { InvalidateRect(Some(hwnd), Some(&rect), false) };
                }
            }
            LRESULT(0)
        }
        WM_PAINT => {
            let mut paint = PAINTSTRUCT::default();
            let dc = unsafe { BeginPaint(hwnd, &mut paint) };
            let paint_rect = paint.rcPaint;
            let background = unsafe { CreateSolidBrush(COLORREF(0x00101010)) };
            unsafe { FillRect(dc, &paint_rect, background) };
            let _ = unsafe { DeleteObject(HGDIOBJ(background.0)) };
            let tick = TICK.load(Ordering::Relaxed);
            let (rect, color) = match SCENARIO.load(Ordering::Relaxed) {
                FULL_MOTION => (
                    paint_rect,
                    if tick.is_multiple_of(2) {
                        0x00402010
                    } else {
                        0x00104070
                    },
                ),
                TYPING => (
                    RECT {
                        left: 40 + ((tick * 13) % 900) as i32,
                        top: 180,
                        right: 80 + ((tick * 13) % 900) as i32,
                        bottom: 230,
                    },
                    0x00ffffff,
                ),
                SCROLL => (
                    RECT {
                        left: 40 + ((tick * 13) % 900) as i32,
                        top: 180,
                        right: 790 + ((tick * 13) % 900) as i32,
                        bottom: 260,
                    },
                    0x0000b0ff,
                ),
                _ => (
                    RECT {
                        left: 40 + ((tick * 13) % 900) as i32,
                        top: 180,
                        right: 80 + ((tick * 13) % 900) as i32,
                        bottom: 260,
                    },
                    0x0000d0ff,
                ),
            };
            let brush = unsafe { CreateSolidBrush(COLORREF(color)) };
            unsafe { FillRect(dc, &rect, brush) };
            let _ = unsafe { DeleteObject(HGDIOBJ(brush.0)) };
            let _ = unsafe { EndPaint(hwnd, &paint) };
            LRESULT(0)
        }
        WM_DESTROY => {
            unsafe { PostQuitMessage(0) };
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}
