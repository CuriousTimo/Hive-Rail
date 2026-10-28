use crate::power::{PowerRequest, PowerState};
use chrono::Local;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table, Wrap},
    Frame, Terminal,
};
use std::{
    io::{self, Stdout},
    time::Duration,
};

pub struct PeerRow {
    pub id: String,
    pub online: bool,
    pub last_seen: Duration,
    pub sequence: u64,
    pub power: String,
    pub rocnet_id: Option<u16>,
}

pub struct Dashboard {
    pub session: String,
    pub node_id: String,
    pub rocnet_id: u16,
    pub local_power: PowerState,
    pub layout_power: String,
    pub loconet: String,
    pub scouting: String,
    pub heartbeat_interval: Duration,
    pub last_event: String,
    pub peers: Vec<PeerRow>,
    pub cli_output: Vec<String>,
}

pub enum UiCommand {
    None,
    Quit,
    Power(PowerRequest),
    SimulateShort,
}

pub struct TerminalUi {
    terminal: Terminal<CrosstermBackend<Stdout>>,
    _guard: TerminalGuard,
    scroll_back: usize,
}

struct TerminalGuard;

impl TerminalUi {
    pub fn new() -> io::Result<Self> {
        enable_raw_mode()?;
        let guard = TerminalGuard;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen)?;
        let terminal = Terminal::new(CrosstermBackend::new(stdout))?;
        Ok(Self {
            terminal,
            _guard: guard,
            scroll_back: 0,
        })
    }

    pub fn handle_input(&mut self) -> io::Result<UiCommand> {
        while event::poll(Duration::ZERO)? {
            let Event::Key(key) = event::read()? else {
                continue;
            };
            if key.kind != KeyEventKind::Press {
                continue;
            }

            match key.code {
                KeyCode::Char('q') | KeyCode::Esc => return Ok(UiCommand::Quit),
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    return Ok(UiCommand::Quit);
                }
                KeyCode::Char('1') => return Ok(UiCommand::Power(PowerRequest::On)),
                KeyCode::Char('0') => return Ok(UiCommand::Power(PowerRequest::Off)),
                KeyCode::Char('h') => return Ok(UiCommand::Power(PowerRequest::Halt)),
                KeyCode::Char('s') => return Ok(UiCommand::SimulateShort),
                KeyCode::PageUp => self.scroll_back = self.scroll_back.saturating_add(5),
                KeyCode::Up => self.scroll_back = self.scroll_back.saturating_add(1),
                KeyCode::PageDown => self.scroll_back = self.scroll_back.saturating_sub(5),
                KeyCode::Down => self.scroll_back = self.scroll_back.saturating_sub(1),
                KeyCode::End => self.scroll_back = 0,
                KeyCode::Home => self.scroll_back = usize::MAX,
                _ => {}
            }
        }
        Ok(UiCommand::None)
    }

    pub fn draw(&mut self, dashboard: &Dashboard) -> io::Result<()> {
        let scroll_back = self.scroll_back;
        self.terminal
            .draw(|frame| render(frame, dashboard, scroll_back))?;
        Ok(())
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
    }
}

fn render(frame: &mut Frame<'_>, dashboard: &Dashboard, scroll_back: usize) {
    let areas = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4),
            Constraint::Min(5),
            Constraint::Length(8),
            Constraint::Length(5),
        ])
        .split(frame.area());

    let title = Paragraph::new(vec![
        Line::from(vec![
            Span::styled(
                " HIVE-RAIL ",
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(format!(
                "  Node: {} (RocNet {})  Session: {}",
                dashboard.node_id, dashboard.rocnet_id, dashboard.session
            )),
            Span::styled(
                format!("  {}", Local::now().format("%Y-%m-%d %H:%M:%S")),
                Style::default().fg(Color::Yellow),
            ),
        ]),
        Line::from(vec![
            Span::styled("DISTRICT ", Style::default().add_modifier(Modifier::BOLD)),
            Span::styled(
                dashboard.local_power.label(),
                power_style(dashboard.local_power.label()),
            ),
            Span::styled("   LAYOUT ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(&dashboard.layout_power),
        ]),
    ])
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title("Hive-Rail node"),
    );
    frame.render_widget(title, areas[0]);

    let rows = dashboard.peers.iter().map(|peer| {
        let (label, color) = if peer.online {
            ("ONLINE", Color::Green)
        } else {
            ("STALE / STOP", Color::Red)
        };
        Row::new(vec![
            Cell::from(peer.id.clone()),
            Cell::from(
                peer.rocnet_id
                    .map_or_else(|| "-".to_owned(), |id| id.to_string()),
            ),
            Cell::from(Span::styled(
                label,
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            )),
            Cell::from(format!("{} ms ago", peer.last_seen.as_millis())),
            Cell::from(peer.sequence.to_string()),
            Cell::from(Span::styled(peer.power.clone(), power_style(&peer.power))),
        ])
    });
    let peers = Table::new(
        rows,
        [
            Constraint::Percentage(26),
            Constraint::Percentage(10),
            Constraint::Percentage(18),
            Constraint::Percentage(18),
            Constraint::Percentage(12),
            Constraint::Percentage(16),
        ],
    )
    .header(
        Row::new([
            "Neighbor node",
            "RocNet",
            "Heartbeat",
            "Last seen",
            "Sequence",
            "Power",
        ])
        .style(Style::default().add_modifier(Modifier::BOLD)),
    )
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title("LAN neighbors"),
    );
    frame.render_widget(peers, areas[1]);

    let visible_log_lines = usize::from(areas[2].height.saturating_sub(2)).max(1);
    let max_log_scroll = dashboard.cli_output.len().saturating_sub(visible_log_lines);
    let log_scroll = max_log_scroll.saturating_sub(scroll_back.min(max_log_scroll));
    let log_title = format!(
        "Raw CLI output ({}/{}; Up/Down, PgUp/PgDn, Home/End)",
        log_scroll
            .saturating_add(1)
            .min(dashboard.cli_output.len().max(1)),
        dashboard.cli_output.len()
    );
    let cli_output = Paragraph::new(dashboard.cli_output.join("\n"))
        .scroll((log_scroll.min(usize::from(u16::MAX)) as u16, 0))
        .block(Block::default().borders(Borders::ALL).title(log_title));
    frame.render_widget(cli_output, areas[2]);

    let footer = Paragraph::new(vec![
        Line::from(vec![
            Span::styled(
                "ZENOH SCOUTING  ",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(&dashboard.scouting),
            Span::raw(format!(
                "  |  Heartbeat: {} ms  |  LocoNet: {}",
                dashboard.heartbeat_interval.as_millis(),
                dashboard.loconet
            )),
        ]),
        Line::from(vec![
            Span::styled(
                "EVENT  ",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(&dashboard.last_event),
        ]),
        Line::from(Span::styled(
            "1  Layout power on  |  0  Layout power off  |  h  Emergency stop  |  s  Simulate short here",
            Style::default().fg(Color::Gray),
        )),
        Line::from(Span::styled(
            "Up/Down, PgUp/PgDn  Scroll log  |  Home/End  Oldest/latest  |  q  Quit",
            Style::default().fg(Color::Gray),
        )),
    ])
    .wrap(Wrap { trim: true })
    .block(Block::default().borders(Borders::ALL).title("Status"));
    frame.render_widget(footer, areas[3]);
}

fn power_style(label: &str) -> Style {
    let color = match label {
        "ON" => Color::Green,
        "OFF" => Color::Gray,
        "STOP" => Color::Yellow,
        _ => Color::Red,
    };
    Style::default().fg(color).add_modifier(Modifier::BOLD)
}
