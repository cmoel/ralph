use std::io::{self, BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::Duration;

use anyhow::Result;
use crossterm::event::{
    DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers, MouseEventKind,
};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui::{DefaultTerminal, Terminal};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AppStatus {
    Stopped,
    Running,
    Error,
}

impl AppStatus {
    fn label(&self) -> &'static str {
        match self {
            AppStatus::Stopped => "STOPPED",
            AppStatus::Running => "RUNNING",
            AppStatus::Error => "ERROR",
        }
    }

    fn color(&self) -> Color {
        match self {
            AppStatus::Stopped => Color::Yellow,
            AppStatus::Running => Color::Green,
            AppStatus::Error => Color::Red,
        }
    }
}

enum OutputMessage {
    Line(String),
}

struct App {
    status: AppStatus,
    output_lines: Vec<String>,
    scroll_offset: u16,
    is_auto_following: bool,
    show_already_running_popup: bool,
    main_pane_height: u16,
    main_pane_width: u16,
    child_process: Option<Child>,
    output_receiver: Option<Receiver<OutputMessage>>,
}

impl App {
    fn new() -> Self {
        Self {
            status: AppStatus::Stopped,
            output_lines: Vec::new(),
            scroll_offset: 0,
            is_auto_following: true,
            show_already_running_popup: false,
            main_pane_height: 0,
            main_pane_width: 0,
            child_process: None,
            output_receiver: None,
        }
    }

    fn visual_line_count(&self) -> u16 {
        if self.main_pane_width == 0 {
            return 0;
        }
        let content: Vec<Line> = self.output_lines.iter().map(Line::raw).collect();
        let paragraph = Paragraph::new(content)
            .block(Block::default().borders(Borders::ALL))
            .wrap(Wrap { trim: false });
        paragraph.line_count(self.main_pane_width) as u16
    }

    fn max_scroll(&self) -> u16 {
        self.visual_line_count()
            .saturating_sub(self.main_pane_height)
    }

    fn scroll_up(&mut self, amount: u16) {
        if self.scroll_offset > 0 {
            self.scroll_offset = self.scroll_offset.saturating_sub(amount);
            self.is_auto_following = false;
        }
    }

    fn scroll_down(&mut self, amount: u16) {
        let max = self.max_scroll();
        self.scroll_offset = (self.scroll_offset + amount).min(max);
        if self.scroll_offset >= max {
            self.is_auto_following = true;
        }
    }

    fn scroll_to_bottom(&mut self) {
        self.scroll_offset = self.max_scroll();
        self.is_auto_following = true;
    }

    fn add_line(&mut self, line: String) {
        self.output_lines.push(line);
        if self.is_auto_following {
            self.scroll_to_bottom();
        }
    }

    fn start_command(&mut self) -> Result<()> {
        if self.status == AppStatus::Running {
            self.show_already_running_popup = true;
            return Ok(());
        }

        // Check if PROMPT.md exists
        if !std::path::Path::new("PROMPT.md").exists() {
            self.status = AppStatus::Error;
            self.add_line("Error: PROMPT.md not found".to_string());
            return Ok(());
        }

        // Add divider if not first run
        if !self.output_lines.is_empty() {
            self.add_line("─".repeat(40));
        }

        // Spawn the command using shell to handle the pipe
        let child = Command::new("sh")
            .arg("-c")
            .arg("cat PROMPT.md | $HOME/.claude/local/claude --output-format=stream-json --verbose --print")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn();

        match child {
            Ok(mut child) => {
                let (tx, rx) = mpsc::channel();

                // Read stdout in a background thread
                if let Some(stdout) = child.stdout.take() {
                    let tx_stdout = tx.clone();
                    thread::spawn(move || {
                        let reader = BufReader::new(stdout);
                        for line in reader.lines().map_while(Result::ok) {
                            if tx_stdout.send(OutputMessage::Line(line)).is_err() {
                                break;
                            }
                        }
                    });
                }

                // Read stderr in a background thread
                if let Some(stderr) = child.stderr.take() {
                    let tx_stderr = tx.clone();
                    thread::spawn(move || {
                        let reader = BufReader::new(stderr);
                        for line in reader.lines().map_while(Result::ok) {
                            if tx_stderr
                                .send(OutputMessage::Line(format!("[stderr] {}", line)))
                                .is_err()
                            {
                                break;
                            }
                        }
                    });
                }

                self.child_process = Some(child);
                self.output_receiver = Some(rx);
                self.status = AppStatus::Running;
            }
            Err(e) => {
                self.status = AppStatus::Error;
                self.add_line(format!("Error starting command: {}", e));
            }
        }

        Ok(())
    }

    fn kill_child(&mut self) {
        if let Some(mut child) = self.child_process.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        self.output_receiver = None;
    }

    fn poll_output(&mut self) {
        // First, collect all pending messages
        let mut messages = Vec::new();
        let mut channel_disconnected = false;

        if let Some(rx) = &self.output_receiver {
            loop {
                match rx.try_recv() {
                    Ok(msg) => messages.push(msg),
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        channel_disconnected = true;
                        break;
                    }
                }
            }
        }

        // Process collected messages
        for msg in messages {
            let OutputMessage::Line(line) = msg;
            self.add_line(line);
        }

        // Check if the channel disconnected (all senders dropped = readers finished)
        if channel_disconnected {
            // Try to get exit status from child process
            if let Some(mut child) = self.child_process.take() {
                match child.try_wait() {
                    Ok(Some(status)) => {
                        if let Some(c) = status.code()
                            && c != 0
                        {
                            self.add_line(format!("[Process exited with code {}]", c));
                        }
                    }
                    Ok(None) => {
                        // Still running, put it back (shouldn't happen if channel disconnected)
                        self.child_process = Some(child);
                        return;
                    }
                    Err(_) => {}
                }
            }
            self.status = AppStatus::Stopped;
            self.output_receiver = None;
        }
    }
}

fn main() -> Result<()> {
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let terminal = Terminal::new(ratatui::backend::CrosstermBackend::new(stdout))?;

    let result = run_app(terminal);

    // Restore terminal
    disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture)?;

    result
}

fn run_app(mut terminal: DefaultTerminal) -> Result<()> {
    let mut app = App::new();

    loop {
        // Poll for output from child process
        app.poll_output();

        // Draw UI
        terminal.draw(|f| draw_ui(f, &mut app))?;

        // Poll for events with a short timeout to allow process output polling
        if crossterm::event::poll(Duration::from_millis(50))? {
            let event = crossterm::event::read()?;

            // Handle popup dismissal first
            if app.show_already_running_popup {
                if let Event::Key(key) = event
                    && (key.code == KeyCode::Enter || key.code == KeyCode::Esc)
                {
                    app.show_already_running_popup = false;
                }
                continue;
            }

            match event {
                Event::Key(key) => match key.code {
                    KeyCode::Char('q') => {
                        app.kill_child();
                        return Ok(());
                    }
                    KeyCode::Char('s') => {
                        if app.status != AppStatus::Error {
                            app.start_command()?;
                        }
                    }
                    KeyCode::Char('k') | KeyCode::Up => {
                        app.scroll_up(1);
                    }
                    KeyCode::Char('j') | KeyCode::Down => {
                        app.scroll_down(1);
                    }
                    KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        let half_page = app.main_pane_height / 2;
                        app.scroll_up(half_page);
                    }
                    KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        let half_page = app.main_pane_height / 2;
                        app.scroll_down(half_page);
                    }
                    KeyCode::Char('b') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        app.scroll_up(app.main_pane_height);
                    }
                    KeyCode::Char('f') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        app.scroll_down(app.main_pane_height);
                    }
                    _ => {}
                },
                Event::Mouse(mouse) => match mouse.kind {
                    MouseEventKind::ScrollUp => {
                        app.scroll_up(3);
                    }
                    MouseEventKind::ScrollDown => {
                        app.scroll_down(3);
                    }
                    _ => {}
                },
                Event::Resize(_, _) => {
                    // Terminal resized, will be handled in next draw
                }
                _ => {}
            }
        }
    }
}

fn draw_ui(f: &mut Frame, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Title bar
            Constraint::Min(1),    // Main pane
            Constraint::Length(1), // Footer
        ])
        .split(f.area());

    // Update main pane dimensions for scroll calculations
    app.main_pane_height = chunks[1].height.saturating_sub(2); // Account for borders
    app.main_pane_width = chunks[1].width;

    // Title bar
    let title_bar = Paragraph::new(Line::from(vec![
        Span::styled("RALPH", Style::default().fg(Color::Cyan)),
        Span::raw("  "),
        Span::styled(app.status.label(), Style::default().fg(app.status.color())),
    ]));
    f.render_widget(title_bar, chunks[0]);

    // Main pane with scrolling
    let content: Vec<Line> = app.output_lines.iter().map(Line::raw).collect();

    let main_pane = Paragraph::new(content)
        .block(Block::default().borders(Borders::ALL))
        .wrap(Wrap { trim: false })
        .scroll((app.scroll_offset, 0));

    f.render_widget(main_pane, chunks[1]);

    // Footer
    let footer_text = if app.status == AppStatus::Error {
        "[q] Quit"
    } else {
        "[s] Start  [q] Quit"
    };
    let footer = Paragraph::new(Line::from(vec![Span::styled(
        footer_text,
        Style::default().fg(Color::DarkGray),
    )]));
    f.render_widget(footer, chunks[2]);

    // Popup dialog if needed
    if app.show_already_running_popup {
        let popup_area = centered_rect(40, 5, f.area());
        f.render_widget(Clear, popup_area);
        let popup = Paragraph::new("Command already running")
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Notice")
                    .style(Style::default().fg(Color::Yellow)),
            )
            .style(Style::default());
        f.render_widget(popup, popup_area);
    }
}

fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    Rect::new(x, y, width.min(area.width), height.min(area.height))
}
