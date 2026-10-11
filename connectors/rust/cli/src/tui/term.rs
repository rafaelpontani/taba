//! O terminal de verdade, em modo cru e sem biblioteca: `stty` no Linux e no macOS, o
//! `SetConsoleMode` do console no Windows (com sequências virtuais, Windows 10 em diante).

use std::collections::VecDeque;
use std::io::{self, Read, Write};

use super::keys::{self, Key};
use super::{Frame, Terminal};

pub struct RawTerminal {
    pending: VecDeque<Key>,
    color: bool,
    saved: sys::Saved,
}

impl RawTerminal {
    /// Põe o terminal em modo cru e entra na tela alternada. Falha se a entrada ou a saída não são
    /// um terminal.
    pub fn open() -> io::Result<RawTerminal> {
        let saved = sys::enter_raw()?;
        let mut out = io::stdout();
        out.write_all(b"\x1b[?1049h\x1b[?25l")?;
        out.flush()?;
        Ok(RawTerminal {
            pending: VecDeque::new(),
            color: std::env::var_os("NO_COLOR").is_none_or(|v| v.is_empty()),
            saved,
        })
    }
}

impl Drop for RawTerminal {
    fn drop(&mut self) {
        let mut out = io::stdout();
        let _ = out.write_all(b"\x1b[0m\x1b[?25h\x1b[?1049l");
        let _ = out.flush();
        sys::restore(&self.saved);
    }
}

impl Terminal for RawTerminal {
    fn size(&self) -> (usize, usize) {
        sys::size().unwrap_or((80, 24))
    }

    fn read_key(&mut self) -> io::Result<Option<Key>> {
        loop {
            if let Some(key) = self.pending.pop_front() {
                return Ok(Some(key));
            }
            let mut buf = [0u8; 256];
            let n = io::stdin().lock().read(&mut buf)?;
            if n == 0 {
                return Ok(None);
            }
            self.pending.extend(keys::decode(&buf[..n]));
        }
    }

    fn draw(&mut self, frame: &Frame) -> io::Result<()> {
        let mut out = String::new();
        out.push_str("\x1b[?25l");
        for (row, line) in frame.lines.iter().enumerate() {
            out.push_str(&format!("\x1b[{};1H{line}\x1b[0m\x1b[K", row + 1));
        }
        out.push_str(&format!("\x1b[{};1H\x1b[J", frame.lines.len() + 1));
        if let Some((row, col)) = frame.cursor {
            out.push_str(&format!("\x1b[{};{}H\x1b[?25h", row + 1, col + 1));
        }
        let mut stdout = io::stdout();
        stdout.write_all(out.as_bytes())?;
        stdout.flush()
    }

    fn color(&self) -> bool {
        self.color
    }
}

#[cfg(unix)]
mod sys {
    use std::io;
    use std::process::{Command, Stdio};

    pub struct Saved(String);

    fn stty(args: &[&str]) -> io::Result<String> {
        let out = Command::new("stty")
            .args(args)
            .stdin(Stdio::inherit())
            .stderr(Stdio::null())
            .output()?;
        if !out.status.success() {
            return Err(io::Error::other("o stty falhou (a entrada é um terminal?)"));
        }
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
    }

    pub fn enter_raw() -> io::Result<Saved> {
        let saved = stty(&["-g"])?;
        stty(&["raw", "-echo"])?;
        Ok(Saved(saved))
    }

    pub fn restore(saved: &Saved) {
        let _ = stty(&[&saved.0]);
    }

    pub fn size() -> Option<(usize, usize)> {
        let text = stty(&["size"]).ok()?;
        let mut parts = text.split_whitespace();
        let rows: usize = parts.next()?.parse().ok()?;
        let cols: usize = parts.next()?.parse().ok()?;
        Some((cols, rows))
    }
}

#[cfg(windows)]
mod sys {
    use std::io;

    type Handle = isize;

    #[repr(C)]
    #[derive(Default, Clone, Copy)]
    struct Coord {
        x: i16,
        y: i16,
    }

    #[repr(C)]
    #[derive(Default, Clone, Copy)]
    struct SmallRect {
        left: i16,
        top: i16,
        right: i16,
        bottom: i16,
    }

    #[repr(C)]
    #[derive(Default, Clone, Copy)]
    struct ScreenBufferInfo {
        size: Coord,
        cursor: Coord,
        attributes: u16,
        window: SmallRect,
        maximum: Coord,
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn GetStdHandle(which: u32) -> Handle;
        fn GetConsoleMode(handle: Handle, mode: *mut u32) -> i32;
        fn SetConsoleMode(handle: Handle, mode: u32) -> i32;
        fn GetConsoleScreenBufferInfo(handle: Handle, info: *mut ScreenBufferInfo) -> i32;
        fn GetConsoleOutputCP() -> u32;
        fn SetConsoleOutputCP(code_page: u32) -> i32;
    }

    const STD_INPUT: u32 = -10i32 as u32;
    const STD_OUTPUT: u32 = -11i32 as u32;
    const ENABLE_PROCESSED_INPUT: u32 = 0x0001;
    const ENABLE_LINE_INPUT: u32 = 0x0002;
    const ENABLE_ECHO_INPUT: u32 = 0x0004;
    const ENABLE_VIRTUAL_TERMINAL_INPUT: u32 = 0x0200;
    const ENABLE_PROCESSED_OUTPUT: u32 = 0x0001;
    const ENABLE_VIRTUAL_TERMINAL_PROCESSING: u32 = 0x0004;
    const UTF8: u32 = 65001;

    pub struct Saved {
        input: u32,
        output: u32,
        code_page: u32,
    }

    fn mode(handle: Handle) -> io::Result<u32> {
        let mut mode = 0;
        // SAFETY: `handle` vem do `GetStdHandle` e `mode` é um `u32` válido para escrita.
        if unsafe { GetConsoleMode(handle, &mut mode) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(mode)
    }

    fn set_mode(handle: Handle, mode: u32) -> io::Result<()> {
        // SAFETY: só repassa o valor ao console.
        if unsafe { SetConsoleMode(handle, mode) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    pub fn enter_raw() -> io::Result<Saved> {
        // SAFETY: `GetStdHandle` e `GetConsoleOutputCP` só leem estado do processo.
        let (input, output, code_page) = unsafe {
            (
                GetStdHandle(STD_INPUT),
                GetStdHandle(STD_OUTPUT),
                GetConsoleOutputCP(),
            )
        };
        let saved = Saved {
            input: mode(input)?,
            output: mode(output)?,
            code_page,
        };
        set_mode(
            input,
            (saved.input & !(ENABLE_ECHO_INPUT | ENABLE_LINE_INPUT | ENABLE_PROCESSED_INPUT))
                | ENABLE_VIRTUAL_TERMINAL_INPUT,
        )?;
        if let Err(e) = set_mode(
            output,
            saved.output | ENABLE_PROCESSED_OUTPUT | ENABLE_VIRTUAL_TERMINAL_PROCESSING,
        ) {
            let _ = set_mode(input, saved.input);
            return Err(e);
        }
        // SAFETY: troca só a página de código de saída, restaurada em `restore`.
        unsafe { SetConsoleOutputCP(UTF8) };
        Ok(saved)
    }

    pub fn restore(saved: &Saved) {
        // SAFETY: devolve ao console os valores que ele tinha.
        unsafe {
            let _ = SetConsoleMode(GetStdHandle(STD_INPUT), saved.input);
            let _ = SetConsoleMode(GetStdHandle(STD_OUTPUT), saved.output);
            SetConsoleOutputCP(saved.code_page);
        }
    }

    pub fn size() -> Option<(usize, usize)> {
        let mut info = ScreenBufferInfo::default();
        // SAFETY: `info` é uma estrutura válida para escrita, no formato que o console espera.
        let ok = unsafe { GetConsoleScreenBufferInfo(GetStdHandle(STD_OUTPUT), &mut info) };
        if ok == 0 {
            return None;
        }
        let w = (info.window.right - info.window.left + 1) as usize;
        let h = (info.window.bottom - info.window.top + 1) as usize;
        Some((w, h))
    }
}

#[cfg(not(any(unix, windows)))]
mod sys {
    use std::io;

    pub struct Saved;

    pub fn enter_raw() -> io::Result<Saved> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "este sistema não tem a tela interativa",
        ))
    }

    pub fn restore(_: &Saved) {}

    pub fn size() -> Option<(usize, usize)> {
        None
    }
}
