//! 终端模拟器
//!
//! 基于 alacritty_terminal 实现真正的终端体验。

pub mod mappings;

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::Arc;

use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::event_loop::{EventLoopSender, Msg};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::Term;
use alacritty_terminal::tty::{self, Options as PtyOptions, Shell};
use anyhow::{Context, Result};
use log::info;

use crate::runtime::{
    global_terminal_event_bus, log_runtime_event, RuntimeEvent, TerminalEventBus,
};

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
    fn columns(&self) -> usize {
        self.cols
    }
    fn screen_lines(&self) -> usize {
        self.rows
    }
    fn total_lines(&self) -> usize {
        self.rows + 5000
    }
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
    terminal_id: String,
    output_seq: Arc<AtomicU64>,
    event_bus: Arc<TerminalEventBus>,
}

enum TerminalEvent {
    Exited,
    Title(String),
}

impl EventListener for TerminalListener {
    fn send_event(&self, event: Event) {
        let ev = match event {
            Event::Wakeup => {
                let seq = self.output_seq.fetch_add(1, Ordering::Relaxed) + 1;
                log_runtime_event(
                    "terminal.raw_output_changed",
                    format!("terminal_id={} seq={}", self.terminal_id, seq),
                );
                self.event_bus.publish(RuntimeEvent::TerminalOutputChanged {
                    terminal_id: self.terminal_id.clone(),
                    seq,
                });
                return;
            }
            Event::Exit | Event::ChildExit(_) => {
                log_runtime_event(
                    "terminal.exited",
                    format!("terminal_id={}", self.terminal_id),
                );
                self.event_bus.publish(RuntimeEvent::TerminalExited {
                    terminal_id: self.terminal_id.clone(),
                });
                TerminalEvent::Exited
            }
            Event::Title(t) => {
                log_runtime_event(
                    "terminal.title_changed",
                    format!("terminal_id={} title={}", self.terminal_id, t),
                );
                self.event_bus.publish(RuntimeEvent::TerminalTitleChanged {
                    terminal_id: self.terminal_id.clone(),
                    title: t.clone(),
                });
                TerminalEvent::Title(t)
            }
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
    exited: bool,
    title: Option<String>,
    cols: usize,
    rows: usize,
}

impl TerminalEmulator {
    pub fn new_with_terminal_id(
        terminal_id: impl Into<String>,
        shell: Option<&str>,
        working_dir: Option<&Path>,
        cols: usize,
        rows: usize,
    ) -> Result<Self> {
        Self::new_with_args_and_terminal_id(terminal_id, shell, &[], working_dir, cols, rows)
    }

    pub fn new_with_args_and_terminal_id(
        terminal_id: impl Into<String>,
        shell: Option<&str>,
        args: &[String],
        working_dir: Option<&Path>,
        cols: usize,
        rows: usize,
    ) -> Result<Self> {
        let terminal_id = terminal_id.into();
        let size = TermSize { cols, rows };
        let (event_tx, event_rx) = mpsc::channel();
        let output_seq = Arc::new(AtomicU64::new(0));
        let event_bus = global_terminal_event_bus();

        let term = Term::new(
            alacritty_terminal::term::Config::default(),
            &size,
            TerminalListener {
                tx: event_tx.clone(),
                terminal_id: terminal_id.clone(),
                output_seq: output_seq.clone(),
                event_bus: event_bus.clone(),
            },
        );
        let term = Arc::new(FairMutex::new(term));

        let pty_config = PtyOptions {
            shell: shell.map(|s| Shell::new(s.to_string(), args.to_vec())),
            working_directory: working_dir.map(|p| p.to_path_buf()),
            drain_on_exit: false,
            env: std::collections::HashMap::from([(
                "PWD".to_string(),
                working_dir
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_default(),
            )]),
        };

        let pty = tty::new(&pty_config, (&size).into(), 0).context("Failed to create PTY")?;

        let event_loop = alacritty_terminal::event_loop::EventLoop::new(
            term.clone(),
            TerminalListener {
                tx: event_tx,
                terminal_id: terminal_id.clone(),
                output_seq,
                event_bus,
            },
            pty,
            false,
            false,
        )
        .context("Failed to create event loop")?;

        let channel = event_loop.channel();

        // EventLoop::spawn() 在新线程中运行
        let _handle = event_loop.spawn();

        Ok(Self {
            term,
            pty_sender: Some(channel),
            event_rx,
            exited: false,
            title: None,
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
                let is_cursor =
                    cursor_point.line == point.line && cursor_point.column == point.column;
                let has_bg = cell.bg != Color::Named(NamedColor::Background);
                chars.push(RenderChar {
                    c: cell.c,
                    is_cursor,
                    has_bg,
                });
            }
            lines.push(RenderLine { chars });
        }
        lines
    }

    /// 获取包含 scrollback 历史的可渲染行。
    pub fn renderable_history_lines(&self) -> Vec<RenderLine> {
        use alacritty_terminal::index::{Column, Line, Point};

        let term = self.term.lock();
        let grid = term.grid();
        let total_lines = grid.total_lines();
        let visible_lines = grid.screen_lines();
        let history_lines = total_lines.saturating_sub(visible_lines);
        let cursor_point = grid.cursor.point;
        let mut lines = Vec::with_capacity(total_lines);

        for line in (0..history_lines).rev() {
            let line_index = Line(-(line as i32) - 1);
            lines.push(render_line_from_string(
                term.bounds_to_string(
                    Point::new(line_index, Column(0)),
                    Point::new(line_index, Column(self.cols.saturating_sub(1))),
                ),
                false,
                self.cols,
            ));
        }

        for line_idx in 0..visible_lines as i32 {
            let line_index = Line(line_idx);
            let has_cursor = cursor_point.line == line_index;
            let mut render_line = render_line_from_string(
                term.bounds_to_string(
                    Point::new(line_index, Column(0)),
                    Point::new(line_index, Column(self.cols.saturating_sub(1))),
                ),
                false,
                self.cols,
            );
            if has_cursor {
                let col = cursor_point
                    .column
                    .0
                    .min(render_line.chars.len().saturating_sub(1));
                if let Some(ch) = render_line.chars.get_mut(col) {
                    ch.is_cursor = true;
                }
            }
            lines.push(render_line);
        }

        trim_leading_blank_lines(lines)
    }

    /// 写入数据到 PTY
    pub fn write(&self, data: &[u8]) {
        if let Some(ref tx) = self.pty_sender {
            if let Err(e) = tx.send(Msg::Input(std::borrow::Cow::Owned(data.to_vec()))) {
                log::error!("[Terminal] Failed to send data to PTY: {}", e);
            }
        }
    }

    /// 输入一行 shell 命令并按 Enter。
    pub fn write_command_line(&self, text: &str) {
        self.write(text.as_bytes());
        self.write(b"\r");
    }

    /// 向交互式 TUI 粘贴一段 prompt，然后按 Enter 提交。
    pub fn write_interactive_prompt(&self, text: &str) {
        let text = text.trim_end_matches(['\r', '\n']);
        self.write(b"\x1b[200~");
        self.write(text.as_bytes());
        self.write(b"\x1b[201~");
        std::thread::sleep(std::time::Duration::from_millis(250));
        self.write(b"\r");
        std::thread::sleep(std::time::Duration::from_millis(250));
        self.write(b"\r");
    }

    /// 向交互式 TUI 发送短选择，例如 Claude Code 的 1/2/3 确认项。
    pub fn write_interactive_choice(&self, text: &str) {
        self.write(text.trim().as_bytes());
        self.write(b"\r");
    }

    pub fn shutdown(&mut self) {
        if let Some(tx) = self.pty_sender.take() {
            let _ = tx.send(Msg::Shutdown);
        }
        self.exited = true;
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
    pub fn process_events(&mut self) {
        while let Ok(event) = self.event_rx.try_recv() {
            match event {
                TerminalEvent::Exited => {
                    self.exited = true;
                    info!("[Terminal] Process exited");
                }
                TerminalEvent::Title(t) => {
                    self.title = Some(t.clone());
                    info!("[Terminal] Title: {}", t);
                }
            }
        }
    }

    pub fn is_exited(&self) -> bool {
        self.exited
    }

    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    pub fn screen_text_lines(&self) -> Vec<String> {
        self.renderable_lines()
            .into_iter()
            .map(|line| {
                line.chars
                    .into_iter()
                    .map(|c| c.c)
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect()
    }

    pub fn columns(&self) -> usize {
        self.cols
    }
    pub fn rows(&self) -> usize {
        self.rows
    }
}

fn render_line_from_string(mut text: String, has_bg: bool, cols: usize) -> RenderLine {
    let char_count = text.chars().count();
    if char_count < cols {
        text.push_str(&" ".repeat(cols - char_count));
    }
    RenderLine {
        chars: text
            .chars()
            .take(cols)
            .map(|c| RenderChar {
                c,
                is_cursor: false,
                has_bg,
            })
            .collect(),
    }
}

fn trim_leading_blank_lines(lines: Vec<RenderLine>) -> Vec<RenderLine> {
    let first_content = lines.iter().position(|line| {
        line.chars
            .iter()
            .any(|ch| ch.is_cursor || !ch.c.is_whitespace())
    });
    match first_content {
        Some(index) => lines.into_iter().skip(index).collect(),
        None => lines,
    }
}
