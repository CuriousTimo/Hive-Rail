use crate::{
    locos::LocoInfo,
    power::{PowerRequest, PowerState},
};
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
    pub locos: Vec<LocoInfo>,
}

pub struct Dashboard {
    pub session: String,
    pub node_id: String,
    pub rocnet_id: u16,
    pub local_power: PowerState,
    pub local_sequence: u64,
    pub local_locos: Vec<LocoInfo>,
    pub last_request: String,
    pub clock: String,
    pub loconet: String,
    pub scouting: String,
    pub heartbeat_interval: Duration,
    pub last_event: String,
    pub peers: Vec<PeerRow>,
    pub cli_output: Vec<String>,
}

pub enum ClockChange {
    ToggleRunning,
    Rate(i8),
    Hours(i16),
}

pub enum UiCommand {
    None,
    Quit,
    Power(PowerRequest),
    /// Emergency stop in every district; each district resumes on its own.
    GlobalStop,
    Clock(ClockChange),
    SimulateShort,
    /// Forget a stale peer; the node refuses this for a peer that is still online.
    RemovePeer(String),
}

pub struct TerminalUi {
    terminal: Terminal<CrosstermBackend<Stdout>>,
    _guard: TerminalGuard,
    scroll_back: usize,
    /// Selected row in the node table; row 0 is this node.
    selected: usize,
    /// Peer IDs in the order last drawn, after this node's row.
    peer_ids: Vec<String>,
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
            selected: 0,
            peer_ids: Vec::new(),
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
                KeyCode::Char('x') | KeyCode::Delete | KeyCode::Backspace => {
                    if let Some(peer_id) = self
                        .selected
                        .checked_sub(1)
                        .and_then(|index| self.peer_ids.get(index))
                    {
                        return Ok(UiCommand::RemovePeer(peer_id.clone()));
                    }
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    self.selected = self.selected.saturating_sub(1);
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    self.selected = (self.selected + 1).min(self.peer_ids.len());
                }
                KeyCode::PageUp => self.scroll_back = self.scroll_back.saturating_add(5),
                KeyCode::PageDown => self.scroll_back = self.scroll_back.saturating_sub(5),
                KeyCode::End => self.scroll_back = 0,
                KeyCode::Home => self.scroll_back = usize::MAX,
                KeyCode::Char(key) => {
                    if let Some(command) = command_for_key(key) {
                        return Ok(command);
                    }
                }
                _ => {}
            }
        }
        Ok(UiCommand::None)
    }

    pub fn draw(&mut self, dashboard: &Dashboard) -> io::Result<()> {
        // Keep the cursor on the same peer when peers are added or removed.
        let selected_id = self
            .selected
            .checked_sub(1)
            .and_then(|index| self.peer_ids.get(index))
            .cloned();
        self.peer_ids = dashboard.peers.iter().map(|peer| peer.id.clone()).collect();
        self.selected = match selected_id {
            Some(id) => match self.peer_ids.iter().position(|peer_id| *peer_id == id) {
                Some(index) => index + 1,
                None => self.selected.min(self.peer_ids.len()),
            },
            None => 0,
        };
        let scroll_back = self.scroll_back;
        let selected = self.selected;
        self.terminal
            .draw(|frame| render(frame, dashboard, scroll_back, selected))?;
        Ok(())
    }
}

/// The node commands shared by the dashboard keys and the lines `--plain` reads on stdin.
pub fn command_for_key(key: char) -> Option<UiCommand> {
    Some(match key {
        '1' => UiCommand::Power(PowerRequest::On),
        '0' => UiCommand::Power(PowerRequest::Off),
        'h' => UiCommand::Power(PowerRequest::Halt),
        'E' => UiCommand::GlobalStop,
        's' => UiCommand::SimulateShort,
        'c' => UiCommand::Clock(ClockChange::ToggleRunning),
        '+' | '=' => UiCommand::Clock(ClockChange::Rate(1)),
        '-' => UiCommand::Clock(ClockChange::Rate(-1)),
        ']' => UiCommand::Clock(ClockChange::Hours(1)),
        '[' => UiCommand::Clock(ClockChange::Hours(-1)),
        _ => return None,
    })
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
    }
}

fn render(frame: &mut Frame<'_>, dashboard: &Dashboard, scroll_back: usize, selected: usize) {
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
                format!("  FAST CLOCK {}", dashboard.clock),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("   wall {}", Local::now().format("%H:%M:%S")),
                Style::default().fg(Color::Gray),
            ),
        ]),
        Line::from(vec![
            Span::styled("DISTRICT ", Style::default().add_modifier(Modifier::BOLD)),
            Span::styled(
                dashboard.local_power.label(),
                power_style(dashboard.local_power.label()),
            ),
            Span::styled(
                "   LAST REQUEST ",
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::raw(&dashboard.last_request),
        ]),
    ])
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title("Hive-Rail node"),
    );
    frame.render_widget(title, areas[0]);

    let mut rows = vec![node_row(
        format!("{} (this node)", dashboard.node_id),
        Some(dashboard.rocnet_id),
        Span::styled(
            "LOCAL",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        "-".to_owned(),
        dashboard.local_sequence,
        dashboard.local_power.label(),
        selected == 0,
    )];
    rows.extend(loco_rows(&dashboard.local_locos));
    for (index, peer) in dashboard.peers.iter().enumerate() {
        let (label, color) = if peer.online {
            ("ONLINE", Color::Green)
        } else {
            ("STALE / STOP", Color::Red)
        };
        rows.push(node_row(
            peer.id.clone(),
            peer.rocnet_id,
            Span::styled(
                label,
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ),
            format_age(peer.last_seen),
            peer.sequence,
            &peer.power,
            selected == index + 1,
        ));
        rows.extend(loco_rows(&peer.locos));
    }
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
            "Node / loco",
            "RocNet",
            "Heartbeat",
            "Last seen",
            "Sequence",
            "Power / F0-F4",
        ])
        .style(Style::default().add_modifier(Modifier::BOLD)),
    )
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title("Nodes (Up/Down select, x remove stale peer)"),
    );
    frame.render_widget(peers, areas[1]);

    let visible_log_lines = usize::from(areas[2].height.saturating_sub(2)).max(1);
    let max_log_scroll = dashboard.cli_output.len().saturating_sub(visible_log_lines);
    let log_scroll = max_log_scroll.saturating_sub(scroll_back.min(max_log_scroll));
    let log_title = format!(
        "Raw CLI output ({}/{}; PgUp/PgDn, Home/End)",
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
            "1  District power on  |  0  District power off  |  h  District emergency stop  |  E  GLOBAL emergency stop  |  s  Simulate short",
            Style::default().fg(Color::Gray),
        )),
        Line::from(Span::styled(
            "c  Fast clock run/pause  |  + -  Clock rate  |  [ ]  Clock hour back/forward",
            Style::default().fg(Color::Gray),
        )),
        Line::from(Span::styled(
            "Up/Down (k/j)  Select node  |  x  Remove stale peer  |  PgUp/PgDn  Scroll log  |  Home/End  Oldest/latest  |  q  Quit",
            Style::default().fg(Color::Gray),
        )),
    ])
    .wrap(Wrap { trim: true })
    .block(Block::default().borders(Borders::ALL).title("Status"));
    frame.render_widget(footer, areas[3]);
}

fn node_row(
    id: String,
    rocnet_id: Option<u16>,
    heartbeat: Span<'static>,
    last_seen: String,
    sequence: u64,
    power: &str,
    selected: bool,
) -> Row<'static> {
    let marker = if selected { "▶ " } else { "  " };
    let row = Row::new(vec![
        Cell::from(format!("{marker}{id}")),
        Cell::from(rocnet_id.map_or_else(|| "-".to_owned(), |id| id.to_string())),
        Cell::from(heartbeat),
        Cell::from(last_seen),
        Cell::from(sequence.to_string()),
        Cell::from(Span::styled(power.to_owned(), power_style(power))),
    ]);
    if selected {
        row.style(Style::default().add_modifier(Modifier::REVERSED))
    } else {
        row
    }
}

/// One indented row per loco under its node: direction and speed.
fn loco_rows(locos: &[LocoInfo]) -> impl Iterator<Item = Row<'static>> + '_ {
    locos.iter().map(|loco| {
        let speed_color = match loco.speed {
            0 => Color::Gray,
            1 => Color::Red,
            _ => Color::Green,
        };
        Row::new(vec![
            Cell::from(format!("    └ {}", loco.label())),
            Cell::from(""),
            Cell::from(Span::styled(
                format!(
                    "{} {}",
                    if loco.forward { "▶" } else { "◀" },
                    loco.direction()
                ),
                Style::default().fg(Color::Cyan),
            )),
            Cell::from(Span::styled(
                loco.speed_label(),
                Style::default()
                    .fg(speed_color)
                    .add_modifier(Modifier::BOLD),
            )),
            Cell::from(""),
            Cell::from(Line::from(
                (0..=4)
                    .map(|number| {
                        let style = if loco.function_on(number) {
                            Style::default()
                                .fg(Color::Yellow)
                                .add_modifier(Modifier::BOLD)
                        } else {
                            Style::default().fg(Color::DarkGray)
                        };
                        Span::styled(format!("F{number} "), style)
                    })
                    .collect::<Vec<_>>(),
            )),
        ])
    })
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

/// Human-readable age: milliseconds while a peer is fresh, then seconds, minutes and hours.
fn format_age(age: Duration) -> String {
    let seconds = age.as_secs();
    if age < Duration::from_secs(1) {
        format!("{} ms ago", age.as_millis())
    } else if seconds < 60 {
        format!("{:.1} s ago", age.as_secs_f64())
    } else if seconds < 3600 {
        format!("{}m {:02}s ago", seconds / 60, seconds % 60)
    } else {
        format!("{}h {:02}m ago", seconds / 3600, seconds % 3600 / 60)
    }
}

#[cfg(test)]
mod tests {
    use super::format_age;
    use std::time::Duration;

    #[test]
    fn age_switches_units_as_it_grows() {
        assert_eq!(format_age(Duration::from_millis(80)), "80 ms ago");
        assert_eq!(format_age(Duration::from_millis(1_500)), "1.5 s ago");
        assert_eq!(format_age(Duration::from_secs(59)), "59.0 s ago");
        assert_eq!(format_age(Duration::from_secs(125)), "2m 05s ago");
        assert_eq!(format_age(Duration::from_secs(3_725)), "1h 02m ago");
    }
}
