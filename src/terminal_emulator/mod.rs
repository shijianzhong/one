//! 终端模拟器
//!
//! 基于 alacritty_terminal 实现真正的终端体验。

pub mod mappings;

use std::path::Path;
use std::sync::Arc;
use std::sync::mpsc;

use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::event_loop::{EventLoop, EventLoopSender, Msg};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::Term;
use alacritty_terminal::tty::{self, Options as PtyOptions, Shell};
use anyhow::{Context, Result};
use log::info;

/// 可渲染的终端中的一行
pub struct RenderLine {
    pub chars: Vec<RenderChar>,
}

/// 终端中的一个字符
pub struct RenderChar {
    pub c: char,
    pub is_cursor: bool,
    pub has_bg: bool,
}

/// 终端尺寸
struct TermSize {
    cols: usize,
    rows: usize,
}

impl Dimensions for TermSize {
    fn columns(&self) -> usize { self.cols }
    fn screen_lines(&self) -> usize { self.rows }
    fn total_lines(&self) -> usize { self.rows * 2 }
}

impl From<&TermSize> for alacritty_terminal::event::WindowSize {
    fn from(s: &TermSize) -> Self {
        alacritty_terminal::event::WindowSize {
            num_lines: s.rows as u16,
            num_cols: s.cols as u16,
            cell_width: 1,
            cell_height: 1,
        }
    }
}

/// 事件监听器
struct TerminalListener {
    tx: mpsc::Sender<TerminalEvent>,
}

enum TerminalEvent {
    Exited,
    Title(String),
}

impl EventListener for TerminalListener {
    fn send_event(&self, event: Event) {
        let ev = match event {
            Event::Exit => TerminalEvent::Exited,
            Event::Title(t) => TerminalEvent::Title(t),
            _ => return,
        };
        let _ = self.tx.send(ev);
    }
}

/// 终端模拟器
pub struct TerminalEmulator {
    term: Arc<FairMutex<Term<TerminalListener>>>,
    pty_sender: Option<EventLoopSender>,
    event_rx: mpsc::Receiver<TerminalEvent>,
    cols: usize,
    rows: usize,
}

impl TerminalEmulator {
    pub fn new(
        shell: Option<&str>,
        working_dir: Option<&Path>,
        cols: usize,
        rows: usize,
    ) -> Result<Self> {
        let size = TermSize { cols, rows };
        let (event_tx, event_rx) = mpsc::channel();

        let term = Term::new(
            alacritty_terminal::term::Config::default(),
            &size,
            TerminalListener { tx: event_tx.clone() },
        );
        let term = Arc::new(FairMutex::new(term));

        let pty_config = PtyOptions {
            shell: shell.map(|s| Shell::new(s.to_string(), Vec::new())),
            working_directory: working_dir.map(|p| p.to_path_buf()),
            drain_on_exit: false,
            env: std::collections::HashMap::new(),
        };

        let pty = tty::new(&pty_config, (&size).into(), 0)
            .context("Failed to create PTY")?;

        let event_loop = alacritty_terminal::event_loop::EventLoop::new(
            term.clone(),
            TerminalListener { tx: event_tx },
            pty,
            false,
            false,
        ).context("Failed to create event loop")?;

        let channel = event_loop.channel();

        // EventLoop::spawn() 在新线程中运行
        let _handle = event_loop.spawn();

        Ok(Self {
            term,
            pty_sender: Some(channel),
            event_rx,
            cols,
            rows,
        })
    }

    /// 获取可渲染的网格内容
    pub fn renderable_lines(&self) -> Vec<RenderLine> {
        use alacritty_terminal::index::{Column, Line, Point};
        use alacritty_terminal::vte::ansi::{Color, NamedColor};

        let term = self.term.lock();
        let grid = term.grid();
        let cursor_point = grid.cursor.point;
        let mut lines = Vec::with_capacity(self.rows);

        for line_idx in 0..self.rows as i32 {
            let mut chars = Vec::with_capacity(self.cols);
            for col_idx in 0..self.cols {
                let point = Point::new(Line(line_idx), Column(col_idx));
                let cell = &grid[point];
                let is_cursor = cursor_point.line == point.line && cursor_point.column == point.column;
                let has_bg = cell.bg != Color::Named(NamedColor::Background);
                chars.push(RenderChar { c: cell.c, is_cursor, has_bg });
            }
            lines.push(RenderLine { chars });
        }
        lines
    }

    /// 写入数据到 PTY
    pub fn write(&self, data: &[u8]) {
        if let Some(ref tx) = self.pty_sender {
            let _ = tx.send(Msg::Input(std::borrow::Cow::Owned(data.to_vec())));
        }
    }

    /// 写入文本
    pub fn write_text(&self, text: &str) {
        let mut data = text.as_bytes().to_vec();
        data.push(b'\n');
        self.write(&data);
    }

    /// 调整大小
    pub fn resize(&mut self, cols: usize, rows: usize) {
        if cols != self.cols || rows != self.rows {
            self.cols = cols;
            self.rows = rows;
            let size = TermSize { cols, rows };
            self.term.lock().resize(TermSize { cols, rows });
            if let Some(ref tx) = self.pty_sender {
                let _ = tx.send(Msg::Resize((&size).into()));
            }
        }
    }

    /// 处理事件
    pub fn process_events(&self) {
        while let Ok(event) = self.event_rx.try_recv() {
            match event {
                TerminalEvent::Exited => info!("[Terminal] Process exited"),
                TerminalEvent::Title(t) => info!("[Terminal] Title: {}", t),
            }
        }
    }

    pub fn columns(&self) -> usize { self.cols }
    pub fn rows(&self) -> usize { self.rows }
}