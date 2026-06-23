use std::{
    env, io,
    path::PathBuf,
    time::{Duration, Instant},
};

use crossterm::{
    event::{self, Event},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Style},
    text::Line,
    widgets::{Block, Borders, Paragraph},
    Terminal,
};
use ratatui_3dmesh::{ControlAction, ControlMap, Mesh, Mesh3dConfig, Mesh3dState, Mesh3dWidget};

fn main() -> ratatui_3dmesh::Result<()> {
    let path = env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("examples/assets/pyramid.obj"));
    let mesh = Mesh::load(&path)?;
    run(mesh).map_err(|source| ratatui_3dmesh::Error::Io {
        path: PathBuf::from("terminal"),
        source,
    })
}

fn run(mesh: Mesh) -> io::Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let result = run_app(&mut terminal, mesh);
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    result
}

fn run_app(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>, mesh: Mesh) -> io::Result<()> {
    let controls = ControlMap::default();
    let mut state = Mesh3dState {
        auto_spin_enabled: true,
        ..Mesh3dState::default()
    };
    let mut config = Mesh3dConfig::default()
        .auto_spin([0.0, 0.45, 0.0])
        .background_style(Some(Style::default().bg(Color::Black)));
    let mut last_tick = Instant::now();
    let mut last_action = String::from("loaded");

    loop {
        let now = Instant::now();
        let dt = now.duration_since(last_tick).as_secs_f32();
        last_tick = now;
        state.tick(dt, &config);

        terminal.draw(|frame| {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(3), Constraint::Length(3)])
                .split(frame.area());
            let viewer = Mesh3dWidget::new(&mesh).config(config.clone());
            frame.render_stateful_widget(viewer, chunks[0], &mut state);

            let status = Paragraph::new(vec![
                Line::from(format!(
                    "{} | vertices:{} faces:{} | mode:{:?} color:{:?} | auto-spin:{}",
                    mesh.name,
                    mesh.vertices.len(),
                    mesh.faces.len(),
                    config.render_mode,
                    config.color_mode,
                    state.auto_spin_enabled
                )),
                Line::from(format!("last: {last_action} | arrows/WASD rotate, hjkl pan, +/- zoom, m/c toggles, ? help, q quit")),
            ])
            .block(Block::default().borders(Borders::ALL).title("ratatui-3dmesh"));
            frame.render_widget(status, chunks[1]);
        })?;

        if event::poll(Duration::from_millis(16))? {
            if let Event::Key(key) = event::read()? {
                if let Some(action) = controls.handle_key(key, &mut state, &mut config) {
                    if action == ControlAction::Quit {
                        break;
                    }
                    last_action = format!("{action:?}");
                }
            }
        }
    }
    Ok(())
}
