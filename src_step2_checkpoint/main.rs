use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Terminal,
};
use std::{
    env,
    io::{stdout, Result},
    thread,
    time::Duration,
};
use crossbeam_channel::unbounded;
use hw::{hw_thread, DriveStatus, HwCmd};

mod hw;

fn build_ruler_line<'a>(piste: u8) -> Line<'a> {
    // Règle d'origine ImageDisk avec diamants '◆' aux demi-dizaines (5, 15, 25...)
    let base_ruler = "0....◆....1....◆....2....◆....3....◆....4....◆....5....◆....6....◆....7....◆";
    let current_pos = (piste as usize).min(79);

    let mut spans = Vec::new();
    for (i, ch) in base_ruler.chars().enumerate() {
        let s = ch.to_string();
        if i <= current_pos {
            // Jauge Blanche IMD : Surbrillance inversée de la piste 0 jusqu'à la piste active !
            spans.push(Span::styled(
                s,
                Style::default().bg(Color::White).fg(Color::Red).add_modifier(Modifier::BOLD),
            ));
        } else {
            // Pistes restantes (Fond rouge normal, texte jaune)
            spans.push(Span::styled(
                s,
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            ));
        }
    }
    Line::from(spans)
}

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    let port_name = args.get(1).cloned();

    // Setup TUI
    enable_raw_mode()?;
    stdout().execute(EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout()))?;

    // Canaux de communication bidirectionnels
    let (tx_status, rx_status) = unbounded::<DriveStatus>();
    let (tx_cmd, rx_cmd) = unbounded::<HwCmd>();

    // Démarrage du thread matériel
    thread::spawn(move || {
        hw_thread(tx_status, rx_cmd, port_name);
    });

    // Boucle Principale TUI
    let mut status = DriveStatus::default();
    loop {
        // Mise à jour non-bloquante du statut envoyé par le thread HW
        while let Ok(new_status) = rx_status.try_recv() {
            status = new_status;
        }

        terminal.draw(|f| {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(4), // Banner / Header Pistes
                    Constraint::Min(1),    // Zone Principale
                ])
                .split(f.size());

            // --- Zone Haute (Header & Règle d'Origine ImageDisk / IMD) ---
            let drive_letter = if status.unit_id == 0 { "A:" } else { "B:" };
            let header_lines = vec![
                Line::from(vec![
                    Span::styled(
                        format!(
                            "{} 500k {}    T{:02}  H{}   fwRzd      15x512  27  84         Single step",
                            drive_letter,
                            if status.density { "HD" } else { "DD" },
                            status.piste,
                            status.tete
                        ),
                        Style::default().fg(Color::White),
                    ),
                ]),
                Line::from(Span::styled(
                    " 1  2  3  4  5  6  7  8  9 10 11 12 13 14 15",
                    Style::default().fg(Color::Yellow),
                )),
                build_ruler_line(status.piste),
                Line::from(""),
            ];
            let header = Paragraph::new(header_lines).style(Style::default().bg(Color::Red));
            f.render_widget(header, chunks[0]);

            // --- Zone Basse (Menu & Panneau de Log) ---
            let lower_chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Length(32),
                    Constraint::Min(1),
                ])
                .split(chunks[1]);

            // Menu de gauche
            let rpm_display = if status.connected {
                if status.motor_on {
                    format!("{} RPM", status.rpm)
                } else {
                    "OFF".to_string()
                }
            } else {
                "[ DÉCONNECTÉ ]".to_string()
            };

            let menu_text = format!(
                "Insert formatted\n\
                 diskette\n\n\
                 TRK0 : {}\nINDEX: {}\nMOT  : {}\nWPROT: {}\nRPM  : {}\n\n\
                 A = Analyze\n\
                 B = Beep on/off\n\
                 D = read Data\n\
                 F = Format\n\
                 H = Head 0/1\n\
                 I = track Image\n\
                 P = fmt Parms\n\
                 R = Recal/seek\n\
                 S = Step S/D\n\
                 W = Write data\n\
                 Z = Zero track\n\
                 0-9 = seek 0-90\n\
                 +/- = Seek +/-1\n\
                 X   = eXit",
                if status.trk0 { "ON " } else { "OFF" },
                if status.index { "ON " } else { "OFF" },
                if status.motor_on { "ON " } else { "OFF" },
                if status.write_protect { "ON " } else { "OFF" },
                rpm_display
            );

            let menu = Paragraph::new(menu_text)
                .style(Style::default().fg(Color::White).bg(Color::Blue));
            f.render_widget(menu, lower_chunks[0]);

            // Colonne de droite (Log & Diagnostics)
            let rpm_stabilite = if status.motor_on && status.rpm > 0 {
                let diff = (status.rpm as i32 - 300).abs();
                if diff <= 5 {
                    "STABLE (300 RPM ± 1%)"
                } else {
                    "EN AJUSTEMENT"
                }
            } else {
                "MOTEUR ÉTEINT"
            };

            let log_content = format!(
                "=== DIAGNOSTIC MATÉRIEL (ÉTAPES 1 & 2) ===\n\n\
                 Statut Connexion : {}\n\
                 Unité Sélectionnée: Drive {} ({})\n\
                 Piste Actuelle    : {}\n\
                 Tête Actuelle     : Tête {}\n\
                 Vitesse Moteur    : {} ({})\n\n\
                 Dernier Message   :\n -> {}\n",
                if status.connected { "Connecté (Greaseweazle v4.1)" } else { "Recherche..." },
                status.unit_id,
                drive_letter,
                status.piste,
                status.tete,
                rpm_display,
                rpm_stabilite,
                status.log_msg
            );

            let logs = Paragraph::new(log_content)
                .style(Style::default().fg(Color::White).bg(Color::Blue));
            f.render_widget(logs, lower_chunks[1]);
        })?;

        // Gestion Clavier non-bloquante avec R (Recal/seek) et Z (Zero track)
        if event::poll(Duration::from_millis(16))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Char('x') | KeyCode::Char('X') | KeyCode::Esc => {
                            let _ = tx_cmd.send(HwCmd::Exit);
                            thread::sleep(Duration::from_millis(50));
                            break;
                        }
                        KeyCode::Char('+') | KeyCode::Char('=') => {
                            let target = status.piste.saturating_add(1);
                            let _ = tx_cmd.send(HwCmd::Seek(target));
                        }
                        KeyCode::Char('-') => {
                            let target = status.piste.saturating_sub(1);
                            let _ = tx_cmd.send(HwCmd::Seek(target));
                        }
                        KeyCode::Char('r') | KeyCode::Char('R') => {
                            let _ = tx_cmd.send(HwCmd::RecalibrateSeek);
                        }
                        KeyCode::Char('z') | KeyCode::Char('Z') => {
                            let _ = tx_cmd.send(HwCmd::ZeroTrack);
                        }
                        KeyCode::Char('h') | KeyCode::Char('H') => {
                            let _ = tx_cmd.send(HwCmd::SetHead(1 - status.tete));
                        }
                        KeyCode::Char('m') | KeyCode::Char('M') => {
                            let _ = tx_cmd.send(HwCmd::SetMotor(!status.motor_on));
                        }
                        KeyCode::Char('u') | KeyCode::Char('U') => {
                            let next_unit = 1 - status.unit_id;
                            let _ = tx_cmd.send(HwCmd::SelectUnit(next_unit));
                        }
                        KeyCode::Char(c) if c.is_ascii_digit() => {
                            if let Some(digit) = c.to_digit(10) {
                                let target = (digit as u8) * 10;
                                let _ = tx_cmd.send(HwCmd::Seek(target));
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    // Cleanup Terminal
    disable_raw_mode()?;
    stdout().execute(LeaveAlternateScreen)?;
    Ok(())
}
