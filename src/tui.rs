use std::io::{self, Stdout};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::prelude::{Color, Frame, Line, Modifier, Span, Style};
use ratatui::widgets::{
    Block, BorderType, Borders, Cell, Clear, List, ListItem, ListState, Paragraph, Row, Table,
    TableState, Wrap,
};
use ratatui::Terminal;

use super::*;

const BRAND: Color = Color::Rgb(230, 230, 230);
const BRAND_DIM: Color = Color::Rgb(180, 180, 180);
const PANEL_BG: Color = Color::Rgb(10, 10, 10);
const PANEL_ALT_BG: Color = Color::Rgb(14, 14, 14);
const BORDER: Color = Color::Rgb(70, 70, 70);
const MUTED: Color = Color::Rgb(150, 150, 150);
const SUCCESS: Color = Color::Rgb(220, 220, 220);
const WARNING: Color = Color::Rgb(200, 200, 200);
const ERROR: Color = Color::Rgb(235, 235, 235);

pub fn run(global: &GlobalOptions) -> AppResult<()> {
    let mut session = TerminalSession::enter()?;
    let mut app = DnsPanel::new(global.clone());
    app.refresh_all();

    let result = app.run(session.terminal_mut());
    drop(session);
    result
}

struct TerminalSession {
    terminal: Terminal<CrosstermBackend<Stdout>>,
}

impl TerminalSession {
    fn enter() -> AppResult<Self> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen)?;
        let backend = CrosstermBackend::new(stdout);
        let terminal = Terminal::new(backend)?;
        Ok(Self { terminal })
    }

    fn terminal_mut(&mut self) -> &mut Terminal<CrosstermBackend<Stdout>> {
        &mut self.terminal
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(self.terminal.backend_mut(), LeaveAlternateScreen);
        let _ = self.terminal.show_cursor();
    }
}

struct DnsPanel {
    global: GlobalOptions,
    runner: Option<PdnsUtil>,
    backend_error: Option<String>,
    background_tx: Sender<BackgroundEvent>,
    background_rx: Receiver<BackgroundEvent>,
    next_request_id: u64,
    active_records_request: Option<u64>,
    active_mutation_request: Option<u64>,
    zones: Vec<String>,
    zone_state: ListState,
    records: Vec<ZoneRecord>,
    filtered_records: Vec<usize>,
    record_state: TableState,
    records_loading: bool,
    pending_zone_reload: Option<PendingZoneReload>,
    filter: String,
    focus: Focus,
    mode: Mode,
    message: Option<FlashMessage>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Focus {
    Zones,
    Records,
}

enum Mode {
    Browse,
    Filter(FilterState),
    CreateZone(CreateZoneForm),
    Add(AddForm),
    Edit(EditForm),
    Soa(SoaDialog),
    SoaEdit(SoaEditForm),
    DeleteConfirm(DeleteDialog),
}

struct FilterState {
    value: String,
    cursor: usize,
}

struct AddForm {
    record_type: String,
    name: String,
    content: String,
    ttl: String,
    field: AddField,
    cursor: usize,
}

struct EditForm {
    spec: DeleteRecordSpec,
    content: String,
    ttl: String,
    field: EditField,
    cursor: usize,
}

struct CreateZoneForm {
    zone: String,
    nameserver: String,
    field: CreateZoneField,
    cursor: usize,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CreateZoneField {
    Zone,
    Nameserver,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum EditField {
    Content,
    Ttl,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SoaEditField {
    PrimaryNameserver,
    Mailbox,
    Serial,
    Refresh,
    Retry,
    Expire,
    Minimum,
    Ttl,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum AddField {
    Type,
    Name,
    Content,
    Ttl,
}

struct DeleteDialog {
    spec: DeleteRecordSpec,
    warning: bool,
}

struct SoaDialog {
    zone: String,
    inspection: SoaInspection,
}

struct SoaEditForm {
    zone: String,
    input: SoaEditInput,
    field: SoaEditField,
    note: Option<String>,
    cursor: usize,
}

struct PendingZoneReload {
    ready_at: Instant,
    clear_records: bool,
}

struct FlashMessage {
    kind: FlashKind,
    text: String,
    expires_at: Instant,
}

enum BackgroundEvent {
    RecordsLoaded {
        request_id: u64,
        zone: String,
        result: AppResult<Vec<ZoneRecord>>,
    },
    MutationFinished {
        request_id: u64,
        zone: String,
        result: AppResult<MutationResult>,
    },
}

enum MutationResult {
    CreateZone {
        zone: String,
        zones: Vec<String>,
        records: Vec<ZoneRecord>,
        output: Option<String>,
        zone_warning: Option<String>,
    },
    Add {
        spec: AddRecordSpec,
        records: Vec<ZoneRecord>,
        output: Option<String>,
        serial_warning: Option<String>,
        zone_warning: Option<String>,
    },
    Edit {
        spec: DeleteRecordSpec,
        replace_spec: ReplaceRrsetSpec,
        records: Vec<ZoneRecord>,
        output: Option<String>,
        serial_warning: Option<String>,
        zone_warning: Option<String>,
    },
    EditSoa {
        zone: String,
        records: Vec<ZoneRecord>,
        output: Option<String>,
        serial_warning: Option<String>,
        zone_warning: Option<String>,
    },
    RepairSoa {
        zone: String,
        records: Vec<ZoneRecord>,
        output: Option<String>,
        serial_warning: Option<String>,
        zone_warning: Option<String>,
    },
    Delete {
        spec: DeleteRecordSpec,
        records: Vec<ZoneRecord>,
        output: Option<String>,
        serial_warning: Option<String>,
        zone_warning: Option<String>,
    },
}

#[derive(Clone, Copy)]
enum FlashKind {
    Info,
    Success,
    Warning,
    Error,
}

impl DnsPanel {
    fn new(global: GlobalOptions) -> Self {
        let (background_tx, background_rx) = mpsc::channel();
        let mut zone_state = ListState::default();
        zone_state.select(Some(0));

        let mut record_state = TableState::default();
        record_state.select(Some(0));

        Self {
            global,
            runner: None,
            backend_error: None,
            background_tx,
            background_rx,
            next_request_id: 1,
            active_records_request: None,
            active_mutation_request: None,
            zones: Vec::new(),
            zone_state,
            records: Vec::new(),
            filtered_records: Vec::new(),
            record_state,
            records_loading: false,
            pending_zone_reload: None,
            filter: String::new(),
            focus: Focus::Records,
            mode: Mode::Browse,
            message: None,
        }
    }

    fn run(&mut self, terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> AppResult<()> {
        let mut needs_draw = true;

        loop {
            if self.drain_background_events() {
                needs_draw = true;
            }

            if self.clear_expired_message() {
                needs_draw = true;
            }

            if needs_draw {
                terminal.draw(|frame| self.draw(frame))?;
                needs_draw = false;
            }

            let timeout = self
                .pending_zone_reload_timeout()
                .map(|timeout| timeout.min(Duration::from_millis(50)))
                .unwrap_or_else(|| Duration::from_millis(50));

            if !event::poll(timeout)? {
                if self
                    .pending_zone_reload_timeout()
                    .is_some_and(|timeout| timeout.is_zero())
                {
                    self.flush_pending_zone_reload();
                    needs_draw = true;
                }
                continue;
            }

            let next_event = event::read()?;

            match next_event {
                Event::Key(key) if key.kind != KeyEventKind::Release => {
                    if self.handle_key(key)? {
                        return Ok(());
                    }
                    needs_draw = true;
                }
                Event::Paste(text) => {
                    self.handle_paste(&text);
                    needs_draw = true;
                }
                Event::Resize(_, _) => {
                    needs_draw = true;
                }
                _ => {}
            }
        }
    }

    fn clear_expired_message(&mut self) -> bool {
        let expired = self
            .message
            .as_ref()
            .is_some_and(|message| Instant::now() >= message.expires_at);

        if expired {
            self.message = None;
            true
        } else {
            false
        }
    }

    fn drain_background_events(&mut self) -> bool {
        let mut changed = false;

        loop {
            match self.background_rx.try_recv() {
                Ok(event) => {
                    self.handle_background_event(event);
                    changed = true;
                }
                Err(TryRecvError::Empty) => return changed,
                Err(TryRecvError::Disconnected) => return changed,
            }
        }
    }

    fn next_request_id(&mut self) -> u64 {
        let request_id = self.next_request_id;
        self.next_request_id += 1;
        request_id
    }

    fn pending_zone_reload_timeout(&self) -> Option<Duration> {
        self.pending_zone_reload
            .as_ref()
            .map(|pending| pending.ready_at.saturating_duration_since(Instant::now()))
    }

    fn schedule_record_reload(&mut self) {
        self.pending_zone_reload = self.selected_zone().map(|_| PendingZoneReload {
            ready_at: Instant::now() + Duration::from_millis(120),
            clear_records: true,
        });

        if let Some(pending) = &self.pending_zone_reload {
            self.records_loading = pending.clear_records;
            if pending.clear_records {
                self.records.clear();
                self.rebuild_filtered_records();
                self.record_state.select(None);
            }
        }
    }

    fn flush_pending_zone_reload(&mut self) {
        if let Some(pending) = self.pending_zone_reload.take() {
            self.reload_records(pending.clear_records);
        }
    }

    fn handle_background_event(&mut self, event: BackgroundEvent) {
        match event {
            BackgroundEvent::RecordsLoaded {
                request_id,
                zone,
                result,
            } => {
                if self.active_records_request != Some(request_id) {
                    return;
                }

                self.active_records_request = None;
                self.records_loading = false;

                if self.selected_zone() != Some(zone.as_str()) {
                    return;
                }

                match result {
                    Ok(records) => {
                        self.records = records;
                        self.rebuild_filtered_records();
                        self.ensure_record_selection();
                    }
                    Err(err) => {
                        self.records.clear();
                        self.rebuild_filtered_records();
                        self.record_state.select(None);
                        self.message = Some(FlashMessage::error(err.to_string()));
                    }
                }
            }
            BackgroundEvent::MutationFinished {
                request_id,
                zone,
                result,
            } => {
                if self.active_mutation_request != Some(request_id) {
                    return;
                }

                self.active_mutation_request = None;

                match result {
                    Ok(MutationResult::CreateZone {
                        zone,
                        zones,
                        records,
                        output,
                        zone_warning,
                    }) => {
                        self.zones = zones;
                        self.zone_state
                            .select(self.zones.iter().position(|candidate| candidate == &zone));
                        self.records = records;
                        self.rebuild_filtered_records();
                        self.ensure_record_selection();
                        self.focus = Focus::Records;
                        self.pending_zone_reload = None;
                        self.records_loading = false;

                        self.message = Some(self.build_mutation_message(
                            format!("zone created: {zone}"),
                            output,
                            zone_warning,
                        ));
                    }
                    Ok(MutationResult::Add {
                        spec,
                        records,
                        output,
                        serial_warning,
                        zone_warning,
                    }) => {
                        if self.selected_zone() == Some(zone.as_str()) {
                            self.records = records;
                            self.rebuild_filtered_records();
                            self.ensure_record_selection();
                        }

                        self.message = Some(self.build_mutation_message(
                            format!(
                                "record added: {} {} {}",
                                spec.name, spec.record_type, spec.content
                            ),
                            output,
                            combine_optional_warnings([serial_warning, zone_warning]),
                        ));
                    }
                    Ok(MutationResult::Edit {
                        spec,
                        replace_spec,
                        records,
                        output,
                        serial_warning,
                        zone_warning,
                    }) => {
                        if self.selected_zone() == Some(zone.as_str()) {
                            self.records = records;
                            self.rebuild_filtered_records();
                            self.ensure_record_selection();
                        }

                        self.message = Some(self.build_mutation_message(
                            format!(
                                "record updated: {} {} {}",
                                spec.name,
                                spec.record_type,
                                replace_spec.contents.join(", ")
                            ),
                            output,
                            combine_optional_warnings([serial_warning, zone_warning]),
                        ));
                    }
                    Ok(MutationResult::EditSoa {
                        zone,
                        records,
                        output,
                        serial_warning,
                        zone_warning,
                    }) => {
                        if self.selected_zone() == Some(zone.as_str()) {
                            self.records = records;
                            self.rebuild_filtered_records();
                            self.ensure_record_selection();
                        }

                        self.message = Some(self.build_mutation_message(
                            format!("SOA updated: {zone}"),
                            output,
                            combine_optional_warnings([serial_warning, zone_warning]),
                        ));
                    }
                    Ok(MutationResult::RepairSoa {
                        zone,
                        records,
                        output,
                        serial_warning,
                        zone_warning,
                    }) => {
                        if self.selected_zone() == Some(zone.as_str()) {
                            self.records = records;
                            self.rebuild_filtered_records();
                            self.ensure_record_selection();
                        }

                        self.message = Some(self.build_mutation_message(
                            format!("SOA repaired: {zone}"),
                            output,
                            combine_optional_warnings([serial_warning, zone_warning]),
                        ));
                    }
                    Ok(MutationResult::Delete {
                        spec,
                        records,
                        output,
                        serial_warning,
                        zone_warning,
                    }) => {
                        if self.selected_zone() == Some(zone.as_str()) {
                            self.records = records;
                            self.rebuild_filtered_records();
                            self.ensure_record_selection();
                        }

                        self.message = Some(self.build_mutation_message(
                            format!(
                                "record deleted: {} {} {}",
                                spec.name, spec.record_type, spec.content
                            ),
                            output,
                            combine_optional_warnings([serial_warning, zone_warning]),
                        ));
                    }
                    Err(err) => {
                        self.message = Some(FlashMessage::error(err.to_string()));
                    }
                }
            }
        }
    }

    fn build_mutation_message(
        &self,
        mut message: String,
        output: Option<String>,
        warning: Option<String>,
    ) -> FlashMessage {
        if let Some(output) = output {
            if !output.is_empty() {
                message.push_str(" | ");
                message.push_str(&output);
            }
        }

        if let Some(warning) = warning {
            FlashMessage::warning(format!("{message} | {warning}"))
        } else {
            FlashMessage::success(message)
        }
    }

    fn draw(&mut self, frame: &mut Frame) {
        let outer = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(12),
                Constraint::Length(3),
            ])
            .split(frame.area());

        self.render_header(frame, outer[0]);
        self.render_body(frame, outer[1]);
        self.render_footer(frame, outer[2]);

        match &self.mode {
            Mode::Browse => {}
            Mode::Filter(state) => self.render_filter_modal(frame, state),
            Mode::CreateZone(form) => self.render_create_zone_modal(frame, form),
            Mode::Add(form) => self.render_add_modal(frame, form),
            Mode::Edit(form) => self.render_edit_modal(frame, form),
            Mode::Soa(dialog) => self.render_soa_modal(frame, dialog),
            Mode::SoaEdit(form) => self.render_soa_edit_modal(frame, form),
            Mode::DeleteConfirm(dialog) => self.render_delete_modal(frame, dialog),
        }
    }

    fn render_header(&self, frame: &mut Frame, area: Rect) {
        let zone = self.selected_zone().unwrap_or("No zone selected");
        let header = Paragraph::new(Line::from(vec![
            Span::styled(
                "ppdns",
                Style::default().fg(BRAND).add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::styled("zone:", Style::default().fg(MUTED)),
            Span::raw(" "),
            Span::styled(zone, Style::default().fg(Color::White)),
            Span::raw("  "),
            Span::styled("filter:", Style::default().fg(MUTED)),
            Span::raw(" "),
            Span::styled(
                if self.filter.is_empty() {
                    "-"
                } else {
                    self.filter.as_str()
                },
                Style::default().fg(Color::White),
            ),
            Span::raw(if self.records_loading {
                "  loading"
            } else {
                ""
            }),
            Span::styled(
                if self.global.dry_run { "  dry-run" } else { "" },
                Style::default().fg(WARNING),
            ),
        ]))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Plain)
                .border_style(Style::default().fg(BRAND))
                .style(Style::default().bg(PANEL_BG)),
        );

        frame.render_widget(header, area);
    }

    fn render_body(&mut self, frame: &mut Frame, area: Rect) {
        let columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(24), Constraint::Min(40)])
            .split(area);

        self.render_zone_list(frame, columns[0]);
        self.render_records_table(frame, columns[1]);
    }

    fn render_zone_list(&mut self, frame: &mut Frame, area: Rect) {
        let title = format!(" Zones ({}) ", self.zones.len());
        let mut items = Vec::new();

        if self.zones.is_empty() {
            items.push(ListItem::new(Line::from(Span::styled(
                "No zones loaded",
                Style::default().fg(MUTED),
            ))));
        } else {
            items.extend(
                self.zones
                    .iter()
                    .map(|zone| ListItem::new(Line::from(Span::raw(zone.clone())))),
            );
        }

        let list = List::new(items)
            .block(self.panel_block(&title, self.focus == Focus::Zones))
            .highlight_style(
                Style::default()
                    .fg(Color::Black)
                    .bg(BRAND)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("> ");

        frame.render_stateful_widget(list, area, &mut self.zone_state);
    }

    fn render_records_table(&mut self, frame: &mut Frame, area: Rect) {
        let title = if self.filter.is_empty() {
            format!(" DNS Records ({}) ", self.filtered_records.len())
        } else {
            format!(" DNS Records ({}) / filtered ", self.filtered_records.len())
        };

        let rows: Vec<Row> = if self.records_loading {
            vec![Row::new(vec![
                Cell::from("Loading records..."),
                Cell::from(""),
                Cell::from(""),
                Cell::from(""),
            ])]
        } else if self.filtered_records.is_empty() {
            vec![Row::new(vec![
                Cell::from("No matching records"),
                Cell::from(""),
                Cell::from(""),
                Cell::from(""),
            ])]
        } else {
            self.filtered_records
                .iter()
                .filter_map(|index| self.records.get(*index))
                .map(|record| {
                    Row::new(vec![
                        Cell::from(record.name.clone()),
                        Cell::from(record.record_type.clone()),
                        Cell::from(
                            record
                                .ttl
                                .map(|ttl| ttl.to_string())
                                .unwrap_or_else(|| "-".to_string()),
                        ),
                        Cell::from(record.content.clone()),
                    ])
                })
                .collect()
        };

        let widths = [
            Constraint::Percentage(34),
            Constraint::Length(8),
            Constraint::Length(6),
            Constraint::Percentage(52),
        ];

        let table = Table::new(rows, widths)
            .header(
                Row::new(vec!["Name", "Type", "TTL", "Content"]).style(
                    Style::default()
                        .fg(BRAND)
                        .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
                ),
            )
            .block(self.panel_block(&title, self.focus == Focus::Records))
            .row_highlight_style(
                Style::default()
                    .bg(Color::Rgb(55, 55, 55))
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("> ");

        frame.render_stateful_widget(table, area, &mut self.record_state);
    }

    fn render_footer(&self, frame: &mut Frame, area: Rect) {
        let (kind, text) = match &self.message {
            Some(message) => (message.kind, message.text.as_str()),
            None => (
                FlashKind::Info,
                "j/k move  tab switch  / filter  z zone  a add  e edit  s soa  d delete  r refresh  q quit",
            ),
        };

        let color = match kind {
            FlashKind::Info => MUTED,
            FlashKind::Success => SUCCESS,
            FlashKind::Warning => WARNING,
            FlashKind::Error => ERROR,
        };

        let footer = Paragraph::new(text)
            .alignment(Alignment::Left)
            .wrap(Wrap { trim: true })
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Plain)
                    .border_style(Style::default().fg(BORDER))
                    .style(Style::default().bg(PANEL_ALT_BG)),
            )
            .style(Style::default().fg(color));

        frame.render_widget(footer, area);
    }

    fn render_filter_modal(&self, frame: &mut Frame, state: &FilterState) {
        let area = centered_rect(60, 18, frame.area());
        let content = Paragraph::new(vec![
            Line::from(Span::styled(
                "Search across name, type, TTL and content.",
                Style::default().fg(MUTED),
            )),
            Line::from(""),
            input_line(
                "Filter: ".to_string(),
                Style::default().fg(BRAND),
                state.value.as_str(),
                Some(state.cursor),
            ),
            Line::from(""),
            Line::from(Span::styled(
                "Enter apply   Esc cancel",
                Style::default().fg(MUTED),
            )),
        ])
        .block(self.modal_block(" Filter Records "));

        frame.render_widget(Clear, area);
        frame.render_widget(content, area);
    }

    fn render_add_modal(&self, frame: &mut Frame, form: &AddForm) {
        let area = centered_rect(72, 54, frame.area());
        let zone = self.selected_zone().unwrap_or("No zone selected");
        let fields = [
            (
                "Type",
                form.record_type.as_str(),
                form.field == AddField::Type,
            ),
            ("Name", form.name.as_str(), form.field == AddField::Name),
            (
                "Content",
                form.content.as_str(),
                form.field == AddField::Content,
            ),
            ("TTL", form.ttl.as_str(), form.field == AddField::Ttl),
        ];

        let mut lines = vec![
            Line::from(vec![
                Span::styled("Zone ", Style::default().fg(MUTED)),
                Span::styled(zone, Style::default().fg(Color::White)),
            ]),
            Line::from(Span::styled(
                "Leave name empty for @.",
                Style::default().fg(MUTED),
            )),
            Line::from(""),
        ];

        for (label, value, selected) in fields {
            lines.push(input_line(
                format!("{label:<8}"),
                if selected {
                    Style::default().fg(BRAND).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(MUTED)
                },
                value,
                selected.then_some(form.cursor),
            ));
        }

        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "Tab or arrows move between fields. Enter saves. Esc cancels.",
            Style::default().fg(MUTED),
        )));

        let content = Paragraph::new(lines)
            .wrap(Wrap { trim: true })
            .block(self.modal_block(" Add DNS Record "));

        frame.render_widget(Clear, area);
        frame.render_widget(content, area);
    }

    fn render_edit_modal(&self, frame: &mut Frame, form: &EditForm) {
        let area = centered_rect(72, 48, frame.area());
        let fields = [
            (
                "Content",
                form.content.as_str(),
                form.field == EditField::Content,
            ),
            ("TTL", form.ttl.as_str(), form.field == EditField::Ttl),
        ];

        let mut lines = vec![
            Line::from(vec![
                Span::styled("Zone ", Style::default().fg(MUTED)),
                Span::styled(form.spec.zone.as_str(), Style::default().fg(Color::White)),
            ]),
            Line::from(vec![
                Span::styled("Name ", Style::default().fg(MUTED)),
                Span::styled(form.spec.name.as_str(), Style::default().fg(Color::White)),
            ]),
            Line::from(vec![
                Span::styled("Type ", Style::default().fg(MUTED)),
                Span::styled(
                    form.spec.record_type.as_str(),
                    Style::default().fg(Color::White),
                ),
            ]),
            Line::from(""),
        ];

        for (label, value, selected) in fields {
            lines.push(input_line(
                format!("{label:<8}"),
                if selected {
                    Style::default().fg(BRAND).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(MUTED)
                },
                value,
                selected.then_some(form.cursor),
            ));
        }

        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "Edit updates the selected value inside the current rrset. Enter saves. Esc cancels.",
            Style::default().fg(MUTED),
        )));

        let content = Paragraph::new(lines)
            .wrap(Wrap { trim: true })
            .block(self.modal_block(" Edit DNS Record "));

        frame.render_widget(Clear, area);
        frame.render_widget(content, area);
    }

    fn render_create_zone_modal(&self, frame: &mut Frame, form: &CreateZoneForm) {
        let area = centered_rect(72, 42, frame.area());
        let fields = [
            (
                "Zone",
                form.zone.as_str(),
                form.field == CreateZoneField::Zone,
            ),
            (
                "Primary NS",
                form.nameserver.as_str(),
                form.field == CreateZoneField::Nameserver,
            ),
        ];

        let mut lines = vec![
            Line::from(Span::styled(
                "ppdns will create the zone and replace the default SOA.",
                Style::default().fg(MUTED),
            )),
            Line::from(Span::styled(
                "Use a nameserver host like ns1 or ns1.example.com.",
                Style::default().fg(MUTED),
            )),
            Line::from(Span::styled(
                "The initial mailbox is hostmaster@<zone>; you can refine it in the SOA editor.",
                Style::default().fg(MUTED),
            )),
            Line::from(""),
        ];

        for (label, value, selected) in fields {
            lines.push(input_line(
                format!("{label:<11}"),
                if selected {
                    Style::default().fg(BRAND).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(MUTED)
                },
                value,
                selected.then_some(form.cursor),
            ));
        }

        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "Tab or arrows move between fields. Enter saves. Esc cancels.",
            Style::default().fg(MUTED),
        )));

        let content = Paragraph::new(lines)
            .wrap(Wrap { trim: true })
            .block(self.modal_block(" Create Zone "));

        frame.render_widget(Clear, area);
        frame.render_widget(content, area);
    }

    fn render_soa_modal(&self, frame: &mut Frame, dialog: &SoaDialog) {
        let area = centered_rect(78, 56, frame.area());
        let mut lines = vec![
            Line::from(vec![
                Span::styled("Zone ", Style::default().fg(MUTED)),
                Span::styled(dialog.zone.as_str(), Style::default().fg(Color::White)),
            ]),
            Line::from(""),
        ];

        if dialog.inspection.apex_soa.is_empty() {
            lines.push(Line::from(Span::styled(
                "No apex SOA record found.",
                Style::default().fg(WARNING),
            )));
        } else {
            for record in &dialog.inspection.apex_soa {
                lines.push(Line::from(vec![
                    Span::styled("TTL ", Style::default().fg(MUTED)),
                    Span::styled(
                        record
                            .ttl
                            .map(|ttl| ttl.to_string())
                            .unwrap_or_else(|| "-".to_string()),
                        Style::default().fg(Color::White),
                    ),
                ]));
                lines.push(Line::from(vec![
                    Span::styled("SOA ", Style::default().fg(MUTED)),
                    Span::styled(record.content.as_str(), Style::default().fg(Color::White)),
                ]));
            }
        }

        if dialog.inspection.non_apex_soa_count > 0 {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                format!(
                    "{} non-apex SOA record(s) detected.",
                    dialog.inspection.non_apex_soa_count
                ),
                Style::default().fg(WARNING),
            )));
        }

        if let Some(warning) = &dialog.inspection.warning {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                warning.as_str(),
                Style::default().fg(WARNING),
            )));
        }

        if let Some(summary) = &dialog.inspection.repair_summary {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                summary.as_str(),
                Style::default().fg(MUTED),
            )));
        }

        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            if dialog.inspection.repair_summary.is_some() {
                "e edits SOA fields. Enter or r repairs mailbox. Esc closes."
            } else {
                "e edits SOA fields. Esc closes."
            },
            Style::default().fg(MUTED),
        )));

        let content = Paragraph::new(lines)
            .wrap(Wrap { trim: true })
            .block(self.modal_block(" SOA Health "));

        frame.render_widget(Clear, area);
        frame.render_widget(content, area);
    }

    fn render_soa_edit_modal(&self, frame: &mut Frame, form: &SoaEditForm) {
        let area = centered_rect(82, 70, frame.area());
        let fields = [
            (
                "Primary NS",
                form.input.primary_nameserver.as_str(),
                form.field == SoaEditField::PrimaryNameserver,
            ),
            (
                "Mailbox",
                form.input.mailbox.as_str(),
                form.field == SoaEditField::Mailbox,
            ),
            (
                "Serial",
                form.input.serial.as_str(),
                form.field == SoaEditField::Serial,
            ),
            (
                "Refresh",
                form.input.refresh.as_str(),
                form.field == SoaEditField::Refresh,
            ),
            (
                "Retry",
                form.input.retry.as_str(),
                form.field == SoaEditField::Retry,
            ),
            (
                "Expire",
                form.input.expire.as_str(),
                form.field == SoaEditField::Expire,
            ),
            (
                "Minimum",
                form.input.minimum.as_str(),
                form.field == SoaEditField::Minimum,
            ),
            (
                "TTL",
                form.input.ttl.as_str(),
                form.field == SoaEditField::Ttl,
            ),
        ];

        let mut lines = vec![
            Line::from(vec![
                Span::styled("Zone ", Style::default().fg(MUTED)),
                Span::styled(form.zone.as_str(), Style::default().fg(Color::White)),
            ]),
            Line::from(Span::styled(
                "Mailbox accepts hostmaster@example.com or hostmaster.example.com.",
                Style::default().fg(MUTED),
            )),
            Line::from(""),
        ];

        if let Some(note) = &form.note {
            lines.push(Line::from(Span::styled(
                note.as_str(),
                Style::default().fg(WARNING),
            )));
            lines.push(Line::from(""));
        }

        for (label, value, selected) in fields {
            lines.push(input_line(
                format!("{label:<11}"),
                if selected {
                    Style::default().fg(BRAND).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(MUTED)
                },
                value,
                selected.then_some(form.cursor),
            ));
        }

        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "Tab or arrows move between fields. Enter saves. Esc returns.",
            Style::default().fg(MUTED),
        )));

        let content = Paragraph::new(lines)
            .wrap(Wrap { trim: true })
            .block(self.modal_block(" Edit SOA "));

        frame.render_widget(Clear, area);
        frame.render_widget(content, area);
    }

    fn render_delete_modal(&self, frame: &mut Frame, dialog: &DeleteDialog) {
        let area = centered_rect(68, 34, frame.area());
        let mut lines = vec![
            Line::from(Span::styled(
                "Delete the selected DNS record?",
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(vec![
                Span::styled("Zone ", Style::default().fg(MUTED)),
                Span::styled(dialog.spec.zone.clone(), Style::default().fg(Color::White)),
            ]),
            Line::from(vec![
                Span::styled("Name ", Style::default().fg(MUTED)),
                Span::styled(dialog.spec.name.clone(), Style::default().fg(Color::White)),
            ]),
            Line::from(vec![
                Span::styled("Type ", Style::default().fg(MUTED)),
                Span::styled(dialog.spec.record_type.clone(), Style::default().fg(BRAND)),
            ]),
            Line::from(vec![
                Span::styled("Value ", Style::default().fg(MUTED)),
                Span::styled(
                    dialog.spec.content.clone(),
                    Style::default().fg(Color::White),
                ),
            ]),
        ];

        if dialog.warning {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "Sensitive record type. Confirm carefully.",
                Style::default().fg(WARNING),
            )));
        }

        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "Enter or y confirm   Esc or n cancel",
            Style::default().fg(MUTED),
        )));

        let content = Paragraph::new(lines)
            .wrap(Wrap { trim: true })
            .block(self.modal_block(" Delete Record "));

        frame.render_widget(Clear, area);
        frame.render_widget(content, area);
    }

    fn handle_key(&mut self, key: KeyEvent) -> AppResult<bool> {
        let mode = std::mem::replace(&mut self.mode, Mode::Browse);

        match mode {
            Mode::Browse => self.handle_browse_key(key),
            Mode::Filter(mut state) => {
                if self.handle_filter_key(key, &mut state) {
                    self.mode = Mode::Filter(state);
                }
                Ok(false)
            }
            Mode::CreateZone(mut form) => {
                if self.handle_create_zone_key(key, &mut form)? {
                    self.mode = Mode::CreateZone(form);
                }
                Ok(false)
            }
            Mode::Add(mut form) => {
                if self.handle_add_key(key, &mut form)? {
                    self.mode = Mode::Add(form);
                }
                Ok(false)
            }
            Mode::Edit(mut form) => {
                if self.handle_edit_key(key, &mut form)? {
                    self.mode = Mode::Edit(form);
                }
                Ok(false)
            }
            Mode::Soa(dialog) => {
                if self.handle_soa_key(key, &dialog)? {
                    self.mode = Mode::Soa(dialog);
                }
                Ok(false)
            }
            Mode::SoaEdit(mut form) => {
                if self.handle_soa_edit_key(key, &mut form)? {
                    self.mode = Mode::SoaEdit(form);
                }
                Ok(false)
            }
            Mode::DeleteConfirm(dialog) => {
                if self.handle_delete_key(key, &dialog)? {
                    self.mode = Mode::DeleteConfirm(dialog);
                }
                Ok(false)
            }
        }
    }

    fn handle_browse_key(&mut self, key: KeyEvent) -> AppResult<bool> {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            return Ok(true);
        }

        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => Ok(true),
            KeyCode::Enter => {
                if self.pending_zone_reload.is_some() {
                    self.flush_pending_zone_reload();
                }
                Ok(false)
            }
            KeyCode::Tab | KeyCode::Right | KeyCode::Left => {
                self.toggle_focus();
                Ok(false)
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.move_selection(1);
                Ok(false)
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.move_selection(-1);
                Ok(false)
            }
            KeyCode::Char('/') => {
                self.mode = Mode::Filter(FilterState {
                    value: self.filter.clone(),
                    cursor: self.filter.chars().count(),
                });
                Ok(false)
            }
            KeyCode::Char('c') => {
                self.filter.clear();
                self.rebuild_filtered_records();
                self.ensure_record_selection();
                self.message = Some(FlashMessage::info("record filter cleared"));
                Ok(false)
            }
            KeyCode::Char('r') => {
                self.refresh_all();
                Ok(false)
            }
            KeyCode::Char('a') => {
                if self.selected_zone().is_some() {
                    self.mode = Mode::Add(AddForm::default());
                } else {
                    self.message =
                        Some(FlashMessage::error("select a zone before adding a record"));
                }
                Ok(false)
            }
            KeyCode::Char('e') => {
                if self.records_loading {
                    self.message = Some(FlashMessage::warning(
                        "wait for zone records to finish loading before editing",
                    ));
                } else if let Some(record) = self.selected_record().cloned() {
                    let zone = self.selected_zone().unwrap_or_default().to_string();
                    if record.record_type.eq_ignore_ascii_case("SOA") {
                        let dialog = SoaDialog {
                            zone: zone.clone(),
                            inspection: inspect_zone_soa(&zone, &self.records),
                        };

                        if record.name == zone {
                            self.mode = Mode::SoaEdit(SoaEditForm::from_dialog(&dialog));
                        } else {
                            self.mode = Mode::Soa(dialog);
                        }
                    } else {
                        self.mode = Mode::Edit(EditForm::from_record(zone, &record));
                    }
                } else {
                    self.message = Some(FlashMessage::warning("select a record before editing"));
                }
                Ok(false)
            }
            KeyCode::Char('s') => {
                if self.records_loading {
                    self.message = Some(FlashMessage::warning(
                        "wait for zone records to finish loading before opening SOA health",
                    ));
                } else if let Some(zone) = self.selected_zone() {
                    let inspection = inspect_zone_soa(zone, &self.records);
                    self.mode = Mode::Soa(SoaDialog {
                        zone: zone.to_string(),
                        inspection,
                    });
                } else {
                    self.message = Some(FlashMessage::warning("select a zone first"));
                }
                Ok(false)
            }
            KeyCode::Char('z') => {
                self.mode = Mode::CreateZone(CreateZoneForm::default());
                Ok(false)
            }
            KeyCode::Char('d') => {
                if self.records_loading {
                    self.message = Some(FlashMessage::warning(
                        "wait for zone records to finish loading before deleting",
                    ));
                } else if let Some(record) = self.selected_record() {
                    let spec = DeleteRecordSpec {
                        zone: self.selected_zone().unwrap_or_default().to_string(),
                        name: record.name.clone(),
                        record_type: record.record_type.clone(),
                        content: record.content.clone(),
                    };
                    let warning = is_sensitive_delete(&spec);
                    self.mode = Mode::DeleteConfirm(DeleteDialog { spec, warning });
                } else {
                    self.message = Some(FlashMessage::warning("select a record before deleting"));
                }
                Ok(false)
            }
            _ => Ok(false),
        }
    }

    fn handle_filter_key(&mut self, key: KeyEvent, state: &mut FilterState) -> bool {
        match key.code {
            KeyCode::Esc => false,
            KeyCode::Enter => {
                self.filter = state.value.trim().to_string();
                self.rebuild_filtered_records();
                self.ensure_record_selection();
                self.message = Some(FlashMessage::info(if self.filter.is_empty() {
                    "showing all records"
                } else {
                    "filter applied"
                }));
                false
            }
            KeyCode::Left => {
                state.move_cursor_left();
                true
            }
            KeyCode::Right => {
                state.move_cursor_right();
                true
            }
            KeyCode::Home => {
                state.cursor = 0;
                true
            }
            KeyCode::End => {
                state.cursor = value_char_len(&state.value);
                true
            }
            KeyCode::Backspace => {
                state.backspace();
                true
            }
            KeyCode::Delete => {
                state.delete();
                true
            }
            KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                state.insert_char(ch);
                true
            }
            _ => true,
        }
    }

    fn handle_create_zone_key(
        &mut self,
        key: KeyEvent,
        form: &mut CreateZoneForm,
    ) -> AppResult<bool> {
        match key.code {
            KeyCode::Esc => Ok(false),
            KeyCode::Enter => {
                self.submit_create_zone_form(form)?;
                Ok(false)
            }
            KeyCode::Tab | KeyCode::Down => {
                form.next_field();
                Ok(true)
            }
            KeyCode::BackTab | KeyCode::Up => {
                form.previous_field();
                Ok(true)
            }
            KeyCode::Left => {
                form.move_cursor_left();
                Ok(true)
            }
            KeyCode::Right => {
                form.move_cursor_right();
                Ok(true)
            }
            KeyCode::Home => {
                form.cursor = 0;
                Ok(true)
            }
            KeyCode::End => {
                form.cursor = value_char_len(form.active_value());
                Ok(true)
            }
            KeyCode::Backspace => {
                form.backspace();
                Ok(true)
            }
            KeyCode::Delete => {
                form.delete();
                Ok(true)
            }
            KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                form.insert_char(ch);
                Ok(true)
            }
            _ => Ok(true),
        }
    }

    fn handle_edit_key(&mut self, key: KeyEvent, form: &mut EditForm) -> AppResult<bool> {
        match key.code {
            KeyCode::Esc => Ok(false),
            KeyCode::Enter => {
                self.submit_edit_form(form)?;
                Ok(false)
            }
            KeyCode::Tab | KeyCode::Down => {
                form.next_field();
                Ok(true)
            }
            KeyCode::BackTab | KeyCode::Up => {
                form.previous_field();
                Ok(true)
            }
            KeyCode::Left => {
                form.move_cursor_left();
                Ok(true)
            }
            KeyCode::Right => {
                form.move_cursor_right();
                Ok(true)
            }
            KeyCode::Home => {
                form.cursor = 0;
                Ok(true)
            }
            KeyCode::End => {
                form.cursor = value_char_len(form.active_value());
                Ok(true)
            }
            KeyCode::Backspace => {
                form.backspace();
                Ok(true)
            }
            KeyCode::Delete => {
                form.delete();
                Ok(true)
            }
            KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                form.insert_char(ch);
                Ok(true)
            }
            _ => Ok(true),
        }
    }

    fn handle_soa_key(&mut self, key: KeyEvent, dialog: &SoaDialog) -> AppResult<bool> {
        match key.code {
            KeyCode::Esc => Ok(false),
            KeyCode::Char('e') => {
                self.mode = Mode::SoaEdit(SoaEditForm::from_dialog(dialog));
                Ok(false)
            }
            KeyCode::Enter | KeyCode::Char('r') => {
                self.submit_soa_repair(dialog)?;
                Ok(false)
            }
            _ => Ok(true),
        }
    }

    fn handle_soa_edit_key(&mut self, key: KeyEvent, form: &mut SoaEditForm) -> AppResult<bool> {
        match key.code {
            KeyCode::Esc => {
                let inspection = inspect_zone_soa(&form.zone, &self.records);
                self.mode = Mode::Soa(SoaDialog {
                    zone: form.zone.clone(),
                    inspection,
                });
                Ok(false)
            }
            KeyCode::Enter => {
                self.submit_soa_edit_form(form)?;
                Ok(false)
            }
            KeyCode::Tab | KeyCode::Down => {
                form.next_field();
                Ok(true)
            }
            KeyCode::BackTab | KeyCode::Up => {
                form.previous_field();
                Ok(true)
            }
            KeyCode::Left => {
                form.move_cursor_left();
                Ok(true)
            }
            KeyCode::Right => {
                form.move_cursor_right();
                Ok(true)
            }
            KeyCode::Home => {
                form.cursor = 0;
                Ok(true)
            }
            KeyCode::End => {
                form.cursor = value_char_len(form.active_value());
                Ok(true)
            }
            KeyCode::Backspace => {
                form.backspace();
                Ok(true)
            }
            KeyCode::Delete => {
                form.delete();
                Ok(true)
            }
            KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                form.insert_char(ch);
                Ok(true)
            }
            _ => Ok(true),
        }
    }

    fn handle_add_key(&mut self, key: KeyEvent, form: &mut AddForm) -> AppResult<bool> {
        match key.code {
            KeyCode::Esc => Ok(false),
            KeyCode::Enter => {
                self.submit_add_form(form)?;
                Ok(false)
            }
            KeyCode::Tab | KeyCode::Down => {
                form.next_field();
                Ok(true)
            }
            KeyCode::BackTab | KeyCode::Up => {
                form.previous_field();
                Ok(true)
            }
            KeyCode::Left => {
                form.move_cursor_left();
                Ok(true)
            }
            KeyCode::Right => {
                form.move_cursor_right();
                Ok(true)
            }
            KeyCode::Home => {
                form.cursor = 0;
                Ok(true)
            }
            KeyCode::End => {
                form.cursor = value_char_len(form.active_value());
                Ok(true)
            }
            KeyCode::Backspace => {
                form.backspace();
                Ok(true)
            }
            KeyCode::Delete => {
                form.delete();
                Ok(true)
            }
            KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                form.insert_char(ch);
                Ok(true)
            }
            _ => Ok(true),
        }
    }

    fn handle_delete_key(&mut self, key: KeyEvent, dialog: &DeleteDialog) -> AppResult<bool> {
        match key.code {
            KeyCode::Esc | KeyCode::Char('n') => Ok(false),
            KeyCode::Enter | KeyCode::Char('y') => {
                self.delete_record(&dialog.spec)?;
                Ok(false)
            }
            _ => Ok(true),
        }
    }

    fn handle_paste(&mut self, text: &str) {
        match &mut self.mode {
            Mode::Filter(state) => state.insert_str(text),
            Mode::CreateZone(form) => form.insert_str(text),
            Mode::Add(form) => form.insert_str(text),
            Mode::Edit(form) => form.insert_str(text),
            Mode::SoaEdit(form) => form.insert_str(text),
            Mode::Browse | Mode::Soa(_) | Mode::DeleteConfirm(_) => {}
        }
    }

    fn refresh_all(&mut self) {
        self.pending_zone_reload = None;
        self.records_loading = false;
        self.active_records_request = None;
        self.active_mutation_request = None;
        self.runner = match PdnsUtil::new(self.global.clone()) {
            Ok(runner) => {
                self.backend_error = None;
                Some(runner)
            }
            Err(err) => {
                self.backend_error = Some(err.to_string());
                self.zones.clear();
                self.records.clear();
                self.rebuild_filtered_records();
                self.zone_state.select(None);
                self.record_state.select(None);
                self.message = Some(FlashMessage::error(
                    "pdnsutil is unavailable; install PowerDNS or pass --pdnsutil",
                ));
                None
            }
        };

        self.reload_zones();
    }

    fn reload_zones(&mut self) {
        let previous_zone = self.selected_zone().map(ToOwned::to_owned);
        let Some(runner) = self.runner.as_ref() else {
            self.records.clear();
            self.rebuild_filtered_records();
            return;
        };

        match runner.list_zones() {
            Ok(zones) => {
                self.zones = zones;

                let selected = previous_zone
                    .and_then(|zone| self.zones.iter().position(|candidate| candidate == &zone))
                    .or_else(|| (!self.zones.is_empty()).then_some(0));

                self.zone_state.select(selected);
                self.pending_zone_reload = None;
                self.reload_records(true);
            }
            Err(err) => {
                self.zones.clear();
                self.records.clear();
                self.rebuild_filtered_records();
                self.zone_state.select(None);
                self.record_state.select(None);
                self.message = Some(FlashMessage::error(err.to_string()));
            }
        }
    }

    fn reload_records(&mut self, clear_records: bool) {
        self.pending_zone_reload = None;
        let Some(zone) = self.selected_zone().map(ToOwned::to_owned) else {
            self.records.clear();
            self.rebuild_filtered_records();
            self.records_loading = false;
            self.active_records_request = None;
            self.record_state.select(None);
            return;
        };

        let Some(runner) = self.runner.as_ref().cloned() else {
            self.records.clear();
            self.rebuild_filtered_records();
            self.records_loading = false;
            self.active_records_request = None;
            self.record_state.select(None);
            return;
        };

        let request_id = self.next_request_id();
        self.active_records_request = Some(request_id);
        self.records_loading = true;

        if clear_records {
            self.records.clear();
            self.rebuild_filtered_records();
            self.record_state.select(None);
        }

        let background_tx = self.background_tx.clone();
        thread::spawn(move || {
            let result = runner.list_zone_records(&zone);
            let _ = background_tx.send(BackgroundEvent::RecordsLoaded {
                request_id,
                zone,
                result,
            });
        });
    }

    fn move_selection(&mut self, delta: isize) {
        match self.focus {
            Focus::Zones => self.move_zone_selection(delta),
            Focus::Records => self.move_record_selection(delta),
        }
    }

    fn move_zone_selection(&mut self, delta: isize) {
        if self.zones.is_empty() {
            return;
        }

        let current = self.zone_state.selected().unwrap_or(0);
        let next = clamp_offset(current, self.zones.len(), delta);
        if next != current {
            self.zone_state.select(Some(next));
            self.schedule_record_reload();
        }
    }

    fn move_record_selection(&mut self, delta: isize) {
        if self.records_loading || self.filtered_records.is_empty() {
            self.record_state.select(None);
            return;
        }

        let current = self.record_state.selected().unwrap_or(0);
        let next = clamp_offset(current, self.filtered_records.len(), delta);
        self.record_state.select(Some(next));
    }

    fn toggle_focus(&mut self) {
        self.focus = match self.focus {
            Focus::Zones => Focus::Records,
            Focus::Records => Focus::Zones,
        };

        if self.focus == Focus::Records && self.pending_zone_reload.is_some() {
            self.flush_pending_zone_reload();
        }
    }

    fn ensure_record_selection(&mut self) {
        let filtered_len = self.filtered_records.len();
        if filtered_len == 0 {
            self.record_state.select(None);
        } else {
            let next = self
                .record_state
                .selected()
                .map(|selected| selected.min(filtered_len.saturating_sub(1)))
                .unwrap_or(0);
            self.record_state.select(Some(next));
        }
    }

    fn rebuild_filtered_records(&mut self) {
        let filter = self.filter.trim().to_ascii_lowercase();
        self.filtered_records = self
            .records
            .iter()
            .enumerate()
            .filter(|(_, record)| {
                if filter.is_empty() {
                    return true;
                }

                let ttl = record
                    .ttl
                    .map(|ttl| ttl.to_string())
                    .unwrap_or_else(|| "-".to_string());
                let haystack = format!(
                    "{} {} {} {}",
                    record.name, record.record_type, ttl, record.content
                )
                .to_ascii_lowercase();
                haystack.contains(&filter)
            })
            .map(|(index, _)| index)
            .collect();
    }

    fn selected_zone(&self) -> Option<&str> {
        self.zone_state
            .selected()
            .and_then(|index| self.zones.get(index))
            .map(String::as_str)
    }

    fn selected_record(&self) -> Option<&ZoneRecord> {
        if self.records_loading {
            return None;
        }

        let visible_index = self.record_state.selected()?;
        let record_index = *self.filtered_records.get(visible_index)?;
        self.records.get(record_index)
    }

    fn submit_edit_form(&mut self, form: &EditForm) -> AppResult<()> {
        if self.active_mutation_request.is_some() {
            self.message = Some(FlashMessage::warning(
                "wait for the current change to finish",
            ));
            return Ok(());
        }

        let runner = self
            .runner
            .as_ref()
            .cloned()
            .ok_or_else(|| AppError::Message("pdnsutil is unavailable".to_string()))?;

        let ttl = if form.ttl.trim().is_empty() {
            None
        } else {
            Some(parse_ttl(&form.ttl)?)
        };
        let new_content =
            normalize_add_content(&form.spec.record_type, &form.content, &form.spec.zone)?;
        let replace_spec = build_edit_replace_spec(&self.records, &form.spec, new_content, ttl)?;
        let zone = form.spec.zone.clone();
        let spec = form.spec.clone();

        if self.global.dry_run {
            let output =
                run_mutation_with_runner(&runner, &runner.replace_rrset_args(&replace_spec))?;
            let serial_warning = bump_serial_with_runner(&zone, &runner);
            self.message = Some(self.build_mutation_message(
                format!("dry run: edit record {}", spec.name),
                output,
                serial_warning,
            ));
            return Ok(());
        }

        let request_id = self.next_request_id();
        self.active_mutation_request = Some(request_id);
        self.message = Some(FlashMessage::info("updating record..."));

        let background_tx = self.background_tx.clone();
        thread::spawn(move || {
            let result = (|| {
                let output =
                    run_mutation_with_runner(&runner, &runner.replace_rrset_args(&replace_spec))?;
                let serial_warning = bump_serial_with_runner(&zone, &runner);
                let records = verify_rrset_replaced(&runner, &replace_spec)?;
                let zone_warning = zone_health_warning(&zone, &records);
                Ok(MutationResult::Edit {
                    spec,
                    replace_spec,
                    records,
                    output,
                    serial_warning,
                    zone_warning,
                })
            })();

            let _ = background_tx.send(BackgroundEvent::MutationFinished {
                request_id,
                zone,
                result,
            });
        });

        Ok(())
    }

    fn submit_soa_repair(&mut self, dialog: &SoaDialog) -> AppResult<()> {
        if self.active_mutation_request.is_some() {
            self.message = Some(FlashMessage::warning(
                "wait for the current change to finish",
            ));
            return Ok(());
        }

        let Some(replace_spec) = dialog.inspection.repair_spec.clone() else {
            self.message = Some(FlashMessage::warning(
                "no automatic SOA repair is available",
            ));
            return Ok(());
        };
        let runner = self
            .runner
            .as_ref()
            .cloned()
            .ok_or_else(|| AppError::Message("pdnsutil is unavailable".to_string()))?;
        let zone = dialog.zone.clone();

        if self.global.dry_run {
            let output =
                run_mutation_with_runner(&runner, &runner.replace_rrset_args(&replace_spec))?;
            let serial_warning = bump_serial_with_runner(&zone, &runner);
            self.message = Some(self.build_mutation_message(
                format!("dry run: repair SOA {}", zone),
                output,
                serial_warning,
            ));
            return Ok(());
        }

        let request_id = self.next_request_id();
        self.active_mutation_request = Some(request_id);
        self.message = Some(FlashMessage::info("repairing SOA..."));

        let background_tx = self.background_tx.clone();
        thread::spawn(move || {
            let result = (|| {
                let output =
                    run_mutation_with_runner(&runner, &runner.replace_rrset_args(&replace_spec))?;
                let serial_warning = bump_serial_with_runner(&zone, &runner);
                let records = verify_rrset_replaced(&runner, &replace_spec)?;
                let zone_warning = zone_health_warning(&zone, &records);
                Ok(MutationResult::RepairSoa {
                    zone: zone.clone(),
                    records,
                    output,
                    serial_warning,
                    zone_warning,
                })
            })();

            let _ = background_tx.send(BackgroundEvent::MutationFinished {
                request_id,
                zone,
                result,
            });
        });

        Ok(())
    }

    fn submit_soa_edit_form(&mut self, form: &SoaEditForm) -> AppResult<()> {
        if self.active_mutation_request.is_some() {
            self.message = Some(FlashMessage::warning(
                "wait for the current change to finish",
            ));
            return Ok(());
        }

        let runner = self
            .runner
            .as_ref()
            .cloned()
            .ok_or_else(|| AppError::Message("pdnsutil is unavailable".to_string()))?;
        let replace_spec = match build_soa_edit_replace_spec(&form.zone, &form.input) {
            Ok(spec) => spec,
            Err(err) => {
                self.message = Some(FlashMessage::error(err.to_string()));
                return Ok(());
            }
        };
        let zone = form.zone.clone();

        if self.global.dry_run {
            let output =
                run_mutation_with_runner(&runner, &runner.replace_rrset_args(&replace_spec))?;
            self.message = Some(self.build_mutation_message(
                format!("dry run: edit SOA {}", zone),
                output,
                None,
            ));
            return Ok(());
        }

        let request_id = self.next_request_id();
        self.active_mutation_request = Some(request_id);
        self.message = Some(FlashMessage::info("updating SOA..."));

        let background_tx = self.background_tx.clone();
        thread::spawn(move || {
            let result = (|| {
                let output =
                    run_mutation_with_runner(&runner, &runner.replace_rrset_args(&replace_spec))?;
                let records = verify_rrset_replaced(&runner, &replace_spec)?;
                let zone_warning = zone_health_warning(&zone, &records);
                Ok(MutationResult::EditSoa {
                    zone: zone.clone(),
                    records,
                    output,
                    serial_warning: None,
                    zone_warning,
                })
            })();

            let _ = background_tx.send(BackgroundEvent::MutationFinished {
                request_id,
                zone,
                result,
            });
        });

        Ok(())
    }

    fn submit_create_zone_form(&mut self, form: &CreateZoneForm) -> AppResult<()> {
        if self.active_mutation_request.is_some() {
            self.message = Some(FlashMessage::warning(
                "wait for the current change to finish",
            ));
            return Ok(());
        }

        let runner = self
            .runner
            .as_ref()
            .cloned()
            .ok_or_else(|| AppError::Message("pdnsutil is unavailable".to_string()))?;

        let spec = match build_create_zone_spec(&runner, &form.zone, &form.nameserver) {
            Ok(spec) => spec,
            Err(err) => {
                self.message = Some(FlashMessage::error(err.to_string()));
                return Ok(());
            }
        };
        let zone = spec.zone.clone();

        if self.global.dry_run {
            let create_output = run_mutation_with_runner(&runner, &runner.create_zone_args(&spec))?;
            let soa_input = SoaEditInput::default_for_zone(&spec.zone, &spec.primary_nameserver);
            let soa_spec = build_soa_edit_replace_spec(&spec.zone, &soa_input)?;
            let soa_output =
                run_mutation_with_runner(&runner, &runner.replace_rrset_args(&soa_spec))?;
            let output = combine_optional_warnings([create_output, soa_output]);
            self.message = Some(self.build_mutation_message(
                format!("dry run: create zone {}", spec.zone),
                output,
                None,
            ));
            return Ok(());
        }

        let request_id = self.next_request_id();
        self.active_mutation_request = Some(request_id);
        self.message = Some(FlashMessage::info("creating zone..."));

        let background_tx = self.background_tx.clone();
        thread::spawn(move || {
            let result = (|| {
                let create_output =
                    run_mutation_with_runner(&runner, &runner.create_zone_args(&spec))?;
                let mut records = verify_zone_created(&runner, &spec)?;
                let mut output_parts = Vec::new();
                let mut soa_setup_warning = None;

                if let Some(output) = create_output {
                    if !output.is_empty() {
                        output_parts.push(output);
                    }
                }

                let soa_input =
                    SoaEditInput::default_for_zone(&spec.zone, &spec.primary_nameserver);
                match build_soa_edit_replace_spec(&spec.zone, &soa_input) {
                    Ok(soa_spec) => {
                        match run_mutation_with_runner(
                            &runner,
                            &runner.replace_rrset_args(&soa_spec),
                        ) {
                            Ok(soa_output) => match verify_rrset_replaced(&runner, &soa_spec) {
                                Ok(soa_records) => {
                                    records = soa_records;
                                    if let Some(output) = soa_output {
                                        if !output.is_empty() {
                                            output_parts.push(output);
                                        }
                                    }
                                }
                                Err(err) => {
                                    soa_setup_warning = Some(format!(
                                        "zone created, but failed to initialize SOA: {err}"
                                    ));
                                }
                            },
                            Err(err) => {
                                soa_setup_warning = Some(format!(
                                    "zone created, but failed to initialize SOA: {err}"
                                ));
                            }
                        }
                    }
                    Err(err) => {
                        soa_setup_warning =
                            Some(format!("zone created, but failed to prepare SOA: {err}"));
                    }
                }

                let output = if output_parts.is_empty() {
                    None
                } else {
                    Some(output_parts.join(" | "))
                };
                let zones = runner.list_zones()?;
                let zone_warning = combine_optional_warnings([
                    soa_setup_warning,
                    zone_health_warning(&spec.zone, &records),
                ]);
                Ok(MutationResult::CreateZone {
                    zone: spec.zone,
                    zones,
                    records,
                    output,
                    zone_warning,
                })
            })();

            let _ = background_tx.send(BackgroundEvent::MutationFinished {
                request_id,
                zone,
                result,
            });
        });

        Ok(())
    }

    fn submit_add_form(&mut self, form: &AddForm) -> AppResult<()> {
        if self.active_mutation_request.is_some() {
            self.message = Some(FlashMessage::warning(
                "wait for the current change to finish",
            ));
            return Ok(());
        }

        let zone = self
            .selected_zone()
            .ok_or_else(|| AppError::Message("no zone selected".to_string()))?
            .to_string();
        let runner = self
            .runner
            .as_ref()
            .cloned()
            .ok_or_else(|| AppError::Message("pdnsutil is unavailable".to_string()))?;

        let record_type = normalize_record_type(&form.record_type);
        if record_type.is_empty() {
            self.message = Some(FlashMessage::error("record type is required"));
            return Ok(());
        }

        let name_raw = if form.name.trim().is_empty() {
            "@"
        } else {
            form.name.trim()
        };
        let ttl = if form.ttl.trim().is_empty() {
            None
        } else {
            Some(parse_ttl(&form.ttl)?)
        };

        let spec = AddRecordSpec {
            zone: zone.clone(),
            name: normalize_owner_name(name_raw, &zone),
            record_type: record_type.clone(),
            content: normalize_add_content(&record_type, &form.content, &zone)?,
            ttl,
        };

        if self.global.dry_run {
            let add_output = run_mutation_with_runner(&runner, &runner.add_record_args(&spec))?;
            let serial_warning = bump_serial_with_runner(&zone, &runner);
            self.message = Some(self.build_mutation_message(
                "dry run: add command prepared".to_string(),
                add_output,
                serial_warning,
            ));
            return Ok(());
        }

        let request_id = self.next_request_id();
        self.active_mutation_request = Some(request_id);
        self.message = Some(FlashMessage::info("adding record..."));

        let background_tx = self.background_tx.clone();
        thread::spawn(move || {
            let result = (|| {
                let output = run_mutation_with_runner(&runner, &runner.add_record_args(&spec))?;
                let serial_warning = bump_serial_with_runner(&zone, &runner);
                let records = verify_add_record_applied(&runner, &spec)?;
                let zone_warning = zone_health_warning(&zone, &records);
                Ok(MutationResult::Add {
                    spec,
                    records,
                    output,
                    serial_warning,
                    zone_warning,
                })
            })();

            let _ = background_tx.send(BackgroundEvent::MutationFinished {
                request_id,
                zone,
                result,
            });
        });

        Ok(())
    }

    fn delete_record(&mut self, spec: &DeleteRecordSpec) -> AppResult<()> {
        if self.active_mutation_request.is_some() {
            self.message = Some(FlashMessage::warning(
                "wait for the current change to finish",
            ));
            return Ok(());
        }

        let runner = self
            .runner
            .as_ref()
            .cloned()
            .ok_or_else(|| AppError::Message("pdnsutil is unavailable".to_string()))?;
        let plan = build_delete_plan(&spec.zone, &self.records, spec)?;
        let zone = spec.zone.clone();

        if self.global.dry_run {
            let delete_output = run_mutation_with_runner(&runner, &runner.delete_plan_args(&plan))?;
            let serial_warning = bump_serial_with_runner(&zone, &runner);
            self.message = Some(self.build_mutation_message(
                "dry run: delete command prepared".to_string(),
                delete_output,
                serial_warning,
            ));
            return Ok(());
        }

        let request_id = self.next_request_id();
        self.active_mutation_request = Some(request_id);
        self.message = Some(FlashMessage::info("deleting record..."));

        let spec = spec.clone();
        let background_tx = self.background_tx.clone();
        thread::spawn(move || {
            let result = (|| {
                let output = run_mutation_with_runner(&runner, &runner.delete_plan_args(&plan))?;
                let serial_warning = bump_serial_with_runner(&zone, &runner);
                let records = verify_delete_record_applied(&runner, &spec, &plan)?;
                let zone_warning = zone_health_warning(&zone, &records);
                Ok(MutationResult::Delete {
                    spec,
                    records,
                    output,
                    serial_warning,
                    zone_warning,
                })
            })();

            let _ = background_tx.send(BackgroundEvent::MutationFinished {
                request_id,
                zone,
                result,
            });
        });

        Ok(())
    }
    fn panel_block(&self, title: &str, focused: bool) -> Block<'static> {
        Block::default()
            .title(title.to_string())
            .borders(Borders::ALL)
            .border_type(BorderType::Plain)
            .border_style(Style::default().fg(if focused { BRAND } else { BORDER }))
            .style(Style::default().bg(PANEL_BG))
    }

    fn modal_block(&self, title: &str) -> Block<'static> {
        Block::default()
            .title(title.to_string())
            .borders(Borders::ALL)
            .border_type(BorderType::Plain)
            .border_style(Style::default().fg(BRAND_DIM))
            .style(Style::default().bg(PANEL_ALT_BG))
    }
}

fn run_mutation_with_runner(runner: &PdnsUtil, args: &[String]) -> AppResult<Option<String>> {
    if runner.global.dry_run {
        return Ok(Some(format!("DRY RUN {}", runner.preview_command(args))));
    }

    let output = runner.run_capture(args)?;
    let trimmed = output.trim();
    if trimmed.is_empty() {
        Ok(None)
    } else {
        Ok(Some(trimmed.to_string()))
    }
}

fn bump_serial_with_runner(zone: &str, runner: &PdnsUtil) -> Option<String> {
    let args = runner.increase_serial_args(zone);
    match run_mutation_with_runner(runner, &args) {
        Ok(_) if runner.global.dry_run => Some("SOA serial bump planned".to_string()),
        Ok(_) => None,
        Err(err) => Some(format!("failed to increase SOA serial: {err}")),
    }
}

fn combine_optional_warnings<const N: usize>(warnings: [Option<String>; N]) -> Option<String> {
    let combined = warnings
        .into_iter()
        .flatten()
        .filter(|warning| !warning.is_empty())
        .collect::<Vec<_>>();

    if combined.is_empty() {
        None
    } else {
        Some(combined.join(" | "))
    }
}

impl CreateZoneForm {
    fn default() -> Self {
        Self {
            zone: String::new(),
            nameserver: "ns1".to_string(),
            field: CreateZoneField::Zone,
            cursor: 0,
        }
    }

    fn next_field(&mut self) {
        self.field = match self.field {
            CreateZoneField::Zone => CreateZoneField::Nameserver,
            CreateZoneField::Nameserver => CreateZoneField::Zone,
        };
        self.cursor = value_char_len(self.active_value());
    }

    fn previous_field(&mut self) {
        self.next_field();
    }

    fn active_value(&self) -> &str {
        match self.field {
            CreateZoneField::Zone => &self.zone,
            CreateZoneField::Nameserver => &self.nameserver,
        }
    }

    fn move_cursor_left(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    fn move_cursor_right(&mut self) {
        self.cursor = (self.cursor + 1).min(value_char_len(self.active_value()));
    }

    fn backspace(&mut self) {
        match self.field {
            CreateZoneField::Zone => backspace_at_cursor(&mut self.zone, &mut self.cursor),
            CreateZoneField::Nameserver => {
                backspace_at_cursor(&mut self.nameserver, &mut self.cursor)
            }
        }
    }

    fn delete(&mut self) {
        match self.field {
            CreateZoneField::Zone => delete_at_cursor(&mut self.zone, self.cursor),
            CreateZoneField::Nameserver => delete_at_cursor(&mut self.nameserver, self.cursor),
        }
    }

    fn insert_char(&mut self, ch: char) {
        match self.field {
            CreateZoneField::Zone => insert_char_at_cursor(&mut self.zone, &mut self.cursor, ch),
            CreateZoneField::Nameserver => {
                insert_char_at_cursor(&mut self.nameserver, &mut self.cursor, ch)
            }
        }
    }

    fn insert_str(&mut self, text: &str) {
        match self.field {
            CreateZoneField::Zone => insert_str_at_cursor(&mut self.zone, &mut self.cursor, text),
            CreateZoneField::Nameserver => {
                insert_str_at_cursor(&mut self.nameserver, &mut self.cursor, text)
            }
        }
    }
}

impl EditForm {
    fn from_record(zone: String, record: &ZoneRecord) -> Self {
        Self {
            spec: DeleteRecordSpec {
                zone,
                name: record.name.clone(),
                record_type: record.record_type.clone(),
                content: record.content.clone(),
            },
            content: record.content.clone(),
            ttl: record.ttl.map(|ttl| ttl.to_string()).unwrap_or_default(),
            field: EditField::Content,
            cursor: value_char_len(&record.content),
        }
    }

    fn next_field(&mut self) {
        self.field = match self.field {
            EditField::Content => EditField::Ttl,
            EditField::Ttl => EditField::Content,
        };
        self.cursor = value_char_len(self.active_value());
    }

    fn previous_field(&mut self) {
        self.next_field();
    }

    fn active_value(&self) -> &str {
        match self.field {
            EditField::Content => &self.content,
            EditField::Ttl => &self.ttl,
        }
    }

    fn move_cursor_left(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    fn move_cursor_right(&mut self) {
        self.cursor = (self.cursor + 1).min(value_char_len(self.active_value()));
    }

    fn backspace(&mut self) {
        match self.field {
            EditField::Content => backspace_at_cursor(&mut self.content, &mut self.cursor),
            EditField::Ttl => backspace_at_cursor(&mut self.ttl, &mut self.cursor),
        }
    }

    fn delete(&mut self) {
        match self.field {
            EditField::Content => delete_at_cursor(&mut self.content, self.cursor),
            EditField::Ttl => delete_at_cursor(&mut self.ttl, self.cursor),
        }
    }

    fn insert_char(&mut self, ch: char) {
        match self.field {
            EditField::Content => insert_char_at_cursor(&mut self.content, &mut self.cursor, ch),
            EditField::Ttl => insert_char_at_cursor(&mut self.ttl, &mut self.cursor, ch),
        }
    }

    fn insert_str(&mut self, text: &str) {
        match self.field {
            EditField::Content => insert_str_at_cursor(&mut self.content, &mut self.cursor, text),
            EditField::Ttl => insert_str_at_cursor(&mut self.ttl, &mut self.cursor, text),
        }
    }
}

impl SoaEditForm {
    fn from_dialog(dialog: &SoaDialog) -> Self {
        let note = if dialog.inspection.apex_soa.is_empty() {
            Some("No apex SOA record is present. Saving will write one apex SOA rrset.".to_string())
        } else if dialog.inspection.apex_soa.len() > 1 {
            Some(format!(
                "{} apex SOA records are present. Saving will replace them with one SOA rrset.",
                dialog.inspection.apex_soa.len()
            ))
        } else {
            dialog.inspection.warning.clone()
        };

        let input = SoaEditInput::from_apex_soa(&dialog.inspection.apex_soa);
        let cursor = value_char_len(&input.primary_nameserver);

        Self {
            zone: dialog.zone.clone(),
            input,
            field: SoaEditField::PrimaryNameserver,
            note,
            cursor,
        }
    }

    fn next_field(&mut self) {
        self.field = match self.field {
            SoaEditField::PrimaryNameserver => SoaEditField::Mailbox,
            SoaEditField::Mailbox => SoaEditField::Serial,
            SoaEditField::Serial => SoaEditField::Refresh,
            SoaEditField::Refresh => SoaEditField::Retry,
            SoaEditField::Retry => SoaEditField::Expire,
            SoaEditField::Expire => SoaEditField::Minimum,
            SoaEditField::Minimum => SoaEditField::Ttl,
            SoaEditField::Ttl => SoaEditField::PrimaryNameserver,
        };
        self.cursor = value_char_len(self.active_value());
    }

    fn previous_field(&mut self) {
        self.field = match self.field {
            SoaEditField::PrimaryNameserver => SoaEditField::Ttl,
            SoaEditField::Mailbox => SoaEditField::PrimaryNameserver,
            SoaEditField::Serial => SoaEditField::Mailbox,
            SoaEditField::Refresh => SoaEditField::Serial,
            SoaEditField::Retry => SoaEditField::Refresh,
            SoaEditField::Expire => SoaEditField::Retry,
            SoaEditField::Minimum => SoaEditField::Expire,
            SoaEditField::Ttl => SoaEditField::Minimum,
        };
        self.cursor = value_char_len(self.active_value());
    }

    fn active_value(&self) -> &str {
        match self.field {
            SoaEditField::PrimaryNameserver => &self.input.primary_nameserver,
            SoaEditField::Mailbox => &self.input.mailbox,
            SoaEditField::Serial => &self.input.serial,
            SoaEditField::Refresh => &self.input.refresh,
            SoaEditField::Retry => &self.input.retry,
            SoaEditField::Expire => &self.input.expire,
            SoaEditField::Minimum => &self.input.minimum,
            SoaEditField::Ttl => &self.input.ttl,
        }
    }

    fn move_cursor_left(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    fn move_cursor_right(&mut self) {
        self.cursor = (self.cursor + 1).min(value_char_len(self.active_value()));
    }

    fn backspace(&mut self) {
        match self.field {
            SoaEditField::PrimaryNameserver => {
                backspace_at_cursor(&mut self.input.primary_nameserver, &mut self.cursor)
            }
            SoaEditField::Mailbox => backspace_at_cursor(&mut self.input.mailbox, &mut self.cursor),
            SoaEditField::Serial => backspace_at_cursor(&mut self.input.serial, &mut self.cursor),
            SoaEditField::Refresh => backspace_at_cursor(&mut self.input.refresh, &mut self.cursor),
            SoaEditField::Retry => backspace_at_cursor(&mut self.input.retry, &mut self.cursor),
            SoaEditField::Expire => backspace_at_cursor(&mut self.input.expire, &mut self.cursor),
            SoaEditField::Minimum => backspace_at_cursor(&mut self.input.minimum, &mut self.cursor),
            SoaEditField::Ttl => backspace_at_cursor(&mut self.input.ttl, &mut self.cursor),
        }
    }

    fn delete(&mut self) {
        match self.field {
            SoaEditField::PrimaryNameserver => {
                delete_at_cursor(&mut self.input.primary_nameserver, self.cursor)
            }
            SoaEditField::Mailbox => delete_at_cursor(&mut self.input.mailbox, self.cursor),
            SoaEditField::Serial => delete_at_cursor(&mut self.input.serial, self.cursor),
            SoaEditField::Refresh => delete_at_cursor(&mut self.input.refresh, self.cursor),
            SoaEditField::Retry => delete_at_cursor(&mut self.input.retry, self.cursor),
            SoaEditField::Expire => delete_at_cursor(&mut self.input.expire, self.cursor),
            SoaEditField::Minimum => delete_at_cursor(&mut self.input.minimum, self.cursor),
            SoaEditField::Ttl => delete_at_cursor(&mut self.input.ttl, self.cursor),
        }
    }

    fn insert_char(&mut self, ch: char) {
        match self.field {
            SoaEditField::PrimaryNameserver => {
                insert_char_at_cursor(&mut self.input.primary_nameserver, &mut self.cursor, ch)
            }
            SoaEditField::Mailbox => {
                insert_char_at_cursor(&mut self.input.mailbox, &mut self.cursor, ch)
            }
            SoaEditField::Serial => {
                insert_char_at_cursor(&mut self.input.serial, &mut self.cursor, ch)
            }
            SoaEditField::Refresh => {
                insert_char_at_cursor(&mut self.input.refresh, &mut self.cursor, ch)
            }
            SoaEditField::Retry => {
                insert_char_at_cursor(&mut self.input.retry, &mut self.cursor, ch)
            }
            SoaEditField::Expire => {
                insert_char_at_cursor(&mut self.input.expire, &mut self.cursor, ch)
            }
            SoaEditField::Minimum => {
                insert_char_at_cursor(&mut self.input.minimum, &mut self.cursor, ch)
            }
            SoaEditField::Ttl => insert_char_at_cursor(&mut self.input.ttl, &mut self.cursor, ch),
        }
    }

    fn insert_str(&mut self, text: &str) {
        match self.field {
            SoaEditField::PrimaryNameserver => {
                insert_str_at_cursor(&mut self.input.primary_nameserver, &mut self.cursor, text)
            }
            SoaEditField::Mailbox => {
                insert_str_at_cursor(&mut self.input.mailbox, &mut self.cursor, text)
            }
            SoaEditField::Serial => {
                insert_str_at_cursor(&mut self.input.serial, &mut self.cursor, text)
            }
            SoaEditField::Refresh => {
                insert_str_at_cursor(&mut self.input.refresh, &mut self.cursor, text)
            }
            SoaEditField::Retry => {
                insert_str_at_cursor(&mut self.input.retry, &mut self.cursor, text)
            }
            SoaEditField::Expire => {
                insert_str_at_cursor(&mut self.input.expire, &mut self.cursor, text)
            }
            SoaEditField::Minimum => {
                insert_str_at_cursor(&mut self.input.minimum, &mut self.cursor, text)
            }
            SoaEditField::Ttl => insert_str_at_cursor(&mut self.input.ttl, &mut self.cursor, text),
        }
    }
}

impl AddForm {
    fn default() -> Self {
        Self {
            record_type: "A".to_string(),
            name: "@".to_string(),
            content: String::new(),
            ttl: String::new(),
            field: AddField::Type,
            cursor: 1,
        }
    }

    fn next_field(&mut self) {
        self.field = match self.field {
            AddField::Type => AddField::Name,
            AddField::Name => AddField::Content,
            AddField::Content => AddField::Ttl,
            AddField::Ttl => AddField::Type,
        };
        self.cursor = value_char_len(self.active_value());
    }

    fn previous_field(&mut self) {
        self.field = match self.field {
            AddField::Type => AddField::Ttl,
            AddField::Name => AddField::Type,
            AddField::Content => AddField::Name,
            AddField::Ttl => AddField::Content,
        };
        self.cursor = value_char_len(self.active_value());
    }

    fn active_value(&self) -> &str {
        match self.field {
            AddField::Type => &self.record_type,
            AddField::Name => &self.name,
            AddField::Content => &self.content,
            AddField::Ttl => &self.ttl,
        }
    }

    fn move_cursor_left(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    fn move_cursor_right(&mut self) {
        self.cursor = (self.cursor + 1).min(value_char_len(self.active_value()));
    }

    fn backspace(&mut self) {
        match self.field {
            AddField::Type => backspace_at_cursor(&mut self.record_type, &mut self.cursor),
            AddField::Name => backspace_at_cursor(&mut self.name, &mut self.cursor),
            AddField::Content => backspace_at_cursor(&mut self.content, &mut self.cursor),
            AddField::Ttl => backspace_at_cursor(&mut self.ttl, &mut self.cursor),
        }
    }

    fn delete(&mut self) {
        match self.field {
            AddField::Type => delete_at_cursor(&mut self.record_type, self.cursor),
            AddField::Name => delete_at_cursor(&mut self.name, self.cursor),
            AddField::Content => delete_at_cursor(&mut self.content, self.cursor),
            AddField::Ttl => delete_at_cursor(&mut self.ttl, self.cursor),
        }
    }

    fn insert_char(&mut self, ch: char) {
        match self.field {
            AddField::Type => insert_char_at_cursor(&mut self.record_type, &mut self.cursor, ch),
            AddField::Name => insert_char_at_cursor(&mut self.name, &mut self.cursor, ch),
            AddField::Content => insert_char_at_cursor(&mut self.content, &mut self.cursor, ch),
            AddField::Ttl => insert_char_at_cursor(&mut self.ttl, &mut self.cursor, ch),
        }
    }

    fn insert_str(&mut self, text: &str) {
        match self.field {
            AddField::Type => insert_str_at_cursor(&mut self.record_type, &mut self.cursor, text),
            AddField::Name => insert_str_at_cursor(&mut self.name, &mut self.cursor, text),
            AddField::Content => insert_str_at_cursor(&mut self.content, &mut self.cursor, text),
            AddField::Ttl => insert_str_at_cursor(&mut self.ttl, &mut self.cursor, text),
        }
    }
}

impl FlashMessage {
    fn new(kind: FlashKind, text: impl Into<String>) -> Self {
        let kind_copy = kind;
        Self {
            kind,
            text: text.into(),
            expires_at: Instant::now() + flash_duration(kind_copy),
        }
    }

    fn info(text: impl Into<String>) -> Self {
        Self::new(FlashKind::Info, text)
    }

    fn success(text: impl Into<String>) -> Self {
        Self::new(FlashKind::Success, text)
    }

    fn warning(text: impl Into<String>) -> Self {
        Self::new(FlashKind::Warning, text)
    }

    fn error(text: impl Into<String>) -> Self {
        Self::new(FlashKind::Error, text)
    }
}

impl FilterState {
    fn move_cursor_left(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    fn move_cursor_right(&mut self) {
        self.cursor = (self.cursor + 1).min(value_char_len(&self.value));
    }

    fn backspace(&mut self) {
        backspace_at_cursor(&mut self.value, &mut self.cursor);
    }

    fn delete(&mut self) {
        delete_at_cursor(&mut self.value, self.cursor);
    }

    fn insert_char(&mut self, ch: char) {
        insert_char_at_cursor(&mut self.value, &mut self.cursor, ch);
    }

    fn insert_str(&mut self, text: &str) {
        insert_str_at_cursor(&mut self.value, &mut self.cursor, text);
    }
}

fn flash_duration(kind: FlashKind) -> Duration {
    match kind {
        FlashKind::Info => Duration::from_secs(3),
        FlashKind::Success => Duration::from_secs(4),
        FlashKind::Warning => Duration::from_secs(6),
        FlashKind::Error => Duration::from_secs(8),
    }
}

fn input_line(
    label: String,
    label_style: Style,
    value: &str,
    cursor: Option<usize>,
) -> Line<'static> {
    let mut spans = vec![Span::styled(label, label_style)];

    match cursor {
        None => spans.push(Span::styled(
            if value.is_empty() {
                " ".to_string()
            } else {
                value.to_string()
            },
            Style::default().fg(Color::White),
        )),
        Some(cursor) => {
            let cursor = clamp_cursor(cursor, value);
            let cursor_byte = char_to_byte_index(value, cursor);
            let value_style = Style::default().fg(Color::White);
            let cursor_style = Style::default()
                .fg(Color::Black)
                .bg(BRAND)
                .add_modifier(Modifier::BOLD);

            if cursor_byte > 0 {
                spans.push(Span::styled(value[..cursor_byte].to_string(), value_style));
            }

            if cursor < value_char_len(value) {
                let next_byte = char_to_byte_index(value, cursor + 1);
                spans.push(Span::styled(
                    value[cursor_byte..next_byte].to_string(),
                    cursor_style,
                ));
                if next_byte < value.len() {
                    spans.push(Span::styled(value[next_byte..].to_string(), value_style));
                }
            } else {
                spans.push(Span::styled("█", Style::default().fg(BRAND)));
            }
        }
    }

    Line::from(spans)
}

fn value_char_len(value: &str) -> usize {
    value.chars().count()
}

fn clamp_cursor(cursor: usize, value: &str) -> usize {
    cursor.min(value_char_len(value))
}

fn char_to_byte_index(value: &str, char_index: usize) -> usize {
    if char_index == 0 {
        return 0;
    }

    value
        .char_indices()
        .nth(char_index)
        .map(|(byte_index, _)| byte_index)
        .unwrap_or_else(|| value.len())
}

fn insert_char_at_cursor(value: &mut String, cursor: &mut usize, ch: char) {
    let insert_at = char_to_byte_index(value, clamp_cursor(*cursor, value));
    value.insert(insert_at, ch);
    *cursor += 1;
}

fn insert_str_at_cursor(value: &mut String, cursor: &mut usize, text: &str) {
    let insert_at = char_to_byte_index(value, clamp_cursor(*cursor, value));
    value.insert_str(insert_at, text);
    *cursor += text.chars().count();
}

fn backspace_at_cursor(value: &mut String, cursor: &mut usize) {
    let cursor_pos = clamp_cursor(*cursor, value);
    if cursor_pos == 0 {
        return;
    }

    let remove_start = char_to_byte_index(value, cursor_pos - 1);
    let remove_end = char_to_byte_index(value, cursor_pos);
    value.replace_range(remove_start..remove_end, "");
    *cursor -= 1;
}

fn delete_at_cursor(value: &mut String, cursor: usize) {
    let cursor_pos = clamp_cursor(cursor, value);
    if cursor_pos >= value_char_len(value) {
        return;
    }

    let remove_start = char_to_byte_index(value, cursor_pos);
    let remove_end = char_to_byte_index(value, cursor_pos + 1);
    value.replace_range(remove_start..remove_end, "");
}

fn normalize_add_content(record_type: &str, content: &str, zone: &str) -> AppResult<String> {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return Err(AppError::Message("record content is required".to_string()));
    }

    Ok(match record_type {
        "CNAME" | "NS" | "PTR" => normalize_target_name(trimmed, zone),
        "TXT" => quote_txt_content(trimmed),
        _ => trimmed.to_string(),
    })
}

fn clamp_offset(current: usize, len: usize, delta: isize) -> usize {
    if len == 0 {
        return 0;
    }

    let max = len.saturating_sub(1) as isize;
    (current as isize + delta).clamp(0, max) as usize
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let popup = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup[1])[1]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dry_run_panel(syntax: PdnsSyntax) -> DnsPanel {
        let global = GlobalOptions {
            dry_run: true,
            ..GlobalOptions::default()
        };
        let runner = PdnsUtil {
            global: global.clone(),
            syntax,
        };
        let mut panel = DnsPanel::new(global);
        panel.runner = Some(runner);
        panel
    }

    fn soa_edit_form(ttl: &str) -> SoaEditForm {
        SoaEditForm {
            zone: "example.com.".to_string(),
            input: SoaEditInput {
                primary_nameserver: "ns1.example.com.".to_string(),
                mailbox: "hostmaster.example.com.".to_string(),
                serial: "2026041401".to_string(),
                refresh: "3600".to_string(),
                retry: "600".to_string(),
                expire: "1209600".to_string(),
                minimum: "300".to_string(),
                ttl: ttl.to_string(),
            },
            field: SoaEditField::PrimaryNameserver,
            note: None,
            cursor: 0,
        }
    }

    #[test]
    fn dry_run_soa_edit_does_not_plan_serial_bump() {
        let mut panel = dry_run_panel(PdnsSyntax::Legacy);

        panel
            .submit_soa_edit_form(&soa_edit_form("300"))
            .expect("SOA edit should succeed");

        let message = panel.message.expect("expected flash message");
        assert!(message.text.contains("dry run: edit SOA example.com."));
        assert!(message.text.contains("replace-rrset"));
        assert!(!message.text.contains("SOA serial bump planned"));
    }

    #[test]
    fn soa_edit_validation_errors_stay_in_tui() {
        let mut panel = dry_run_panel(PdnsSyntax::Legacy);

        panel
            .submit_soa_edit_form(&soa_edit_form(""))
            .expect("validation errors should not exit the TUI");

        let message = panel.message.expect("expected flash message");
        assert_eq!(message.text, "SOA TTL is required");
        assert!(matches!(message.kind, FlashKind::Error));
    }
}
