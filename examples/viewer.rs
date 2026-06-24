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
use ratatui_3dmesh::{
    ColorMode, ControlAction, ControlMap, Mesh, Mesh3dConfig, Mesh3dState, Mesh3dWidget,
    MeshLoadOptions, ProjectionMode, Vec3,
};

#[derive(Debug, Clone)]
struct Args {
    mesh: PathBuf,
    texture: Option<PathBuf>,
}

fn main() -> ratatui_3dmesh::Result<()> {
    let args = parse_args();
    let mut options = MeshLoadOptions::default().load_material_textures(true);
    if let Some(texture) = args.texture.as_ref() {
        options = options.texture_override(texture);
    }
    let mesh = Mesh::load_with_options(&args.mesh, options)?;
    run(mesh, args.texture).map_err(|source| ratatui_3dmesh::Error::Io {
        path: PathBuf::from("terminal"),
        source,
    })
}

fn parse_args() -> Args {
    let mut mesh = None;
    let mut texture = None;
    let mut args = env::args_os().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--texture" || arg == "-t" {
            texture = args.next().map(PathBuf::from);
        } else if mesh.is_none() {
            mesh = Some(PathBuf::from(arg));
        }
    }
    Args {
        mesh: mesh.unwrap_or_else(|| PathBuf::from("examples/assets/pyramid.obj")),
        texture,
    }
}

fn run(mesh: Mesh, texture_arg: Option<PathBuf>) -> io::Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let result = run_app(&mut terminal, mesh, texture_arg);
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    result
}

fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    mesh: Mesh,
    texture_arg: Option<PathBuf>,
) -> io::Result<()> {
    let controls = ControlMap::default();
    let mut state = Mesh3dState {
        auto_spin_enabled: false,
        rotation: Vec3::default(),
        ..Mesh3dState::default()
    };
    state.clamp_animation_selection(mesh.animations.len());
    let initial_color = if mesh.default_texture.is_some()
        || mesh
            .materials
            .iter()
            .any(|m| m.diffuse_texture.as_ref().and_then(|t| t.index).is_some())
    {
        ColorMode::Texture
    } else {
        ColorMode::Material
    };
    let mut config = Mesh3dConfig::default()
        .color_mode(initial_color)
        .projection(ProjectionMode::Orthographic)
        .color_brightness(1.45)
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
                .constraints([Constraint::Min(3), Constraint::Length(5)])
                .split(frame.area());
            let viewer = Mesh3dWidget::new(&mesh).config(config.clone());
            frame.render_stateful_widget(viewer, chunks[0], &mut state);

            let texture_status = texture_status(&mesh, texture_arg.as_ref());
            let animation_status = animation_status(&mesh, &state);
            let status = Paragraph::new(vec![
                Line::from(format!(
                    "{} | vertices:{} faces:{} uvs:{} textures:{} | mode:{:?} color:{:?} brightness:{:.2} | auto-spin:{}",
                    mesh.name,
                    mesh.vertices.len(),
                    mesh.faces.len(),
                    mesh.tex_coords.len(),
                    mesh.textures.len(),
                    config.render_mode,
                    config.color_mode,
                    config.color_brightness,
                    state.auto_spin_enabled
                )),
                Line::from(texture_status),
                Line::from(animation_status),
                Line::from(format!("last: {last_action} | arrows/WASD rotate, hjkl pan, +/- zoom, m/c/o, [/] brightness, p/n/b/0 anim, ,/. speed, ? help, q quit")),
            ])
            .block(Block::default().borders(Borders::ALL).title("ratatui-3dmesh"));
            frame.render_widget(status, chunks[1]);
        })?;

        if event::poll(Duration::from_millis(16))? {
            if let Event::Key(key) = event::read()? {
                if let Some(action) = controls.handle_key_with_animation_count(
                    key,
                    &mut state,
                    &mut config,
                    mesh.animations.len(),
                ) {
                    if action == ControlAction::Quit {
                        break;
                    }
                    state.clamp_animation_selection(mesh.animations.len());
                    last_action = format!("{action:?}");
                }
            }
        }
    }
    Ok(())
}

fn animation_status(mesh: &Mesh, state: &Mesh3dState) -> String {
    if mesh.animations.is_empty() {
        return "animation: none (OBJ/STL/static glTF render as static meshes)".to_string();
    }
    let selected = state
        .selected_animation
        .and_then(|index| mesh.animations.get(index).map(|clip| (index, clip)));
    let Some((index, clip)) = selected else {
        return format!(
            "animation: {} clips loaded, none selected",
            mesh.animations.len()
        );
    };
    let time = state.animation_display_time(clip.duration_seconds);
    format!(
        "animation: {}/{} {} | {:.2}/{:.2}s | channels:{} | speed:{:.2} | {} | loop:{}",
        index + 1,
        mesh.animations.len(),
        clip.name,
        time,
        clip.duration_seconds,
        clip.channel_count(),
        state.animation_speed,
        if state.animation_playing {
            "playing"
        } else {
            "paused"
        },
        state.animation_looping
    )
}

fn texture_status(mesh: &Mesh, texture_arg: Option<&PathBuf>) -> String {
    if mesh.textures.is_empty() {
        let hint = if mesh.tex_coords.is_empty() {
            "model has no UVs"
        } else if texture_arg.is_some() {
            "texture failed to load or textures feature is disabled"
        } else {
            "pass --texture <image> or use MTL map_Kd"
        };
        format!("texture: inactive ({hint})")
    } else {
        let source = mesh
            .default_texture
            .as_ref()
            .map(|t| t.path.display().to_string())
            .or_else(|| {
                mesh.materials.iter().find_map(|m| {
                    m.diffuse_texture
                        .as_ref()
                        .map(|t| t.path.display().to_string())
                })
            })
            .unwrap_or_else(|| "material map_Kd".to_string());
        format!("texture: active from {source}")
    }
}
