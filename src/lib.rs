//! Embeddable Ratatui 3D mesh viewer.
//!
//! `ratatui-3dmesh` renders OBJ and glTF/GLB meshes into a Ratatui
//! [`ratatui::buffer::Buffer`] as shaded terminal glyphs. The library is designed as a
//! reusable widget: your app owns terminal setup, event loops, and layout, while this
//! crate owns mesh loading, projection, rasterization, and viewer state.
//!
//! # Quick start
//!
//! ```no_run
//! use ratatui_3dmesh::{Mesh, Mesh3dConfig, Mesh3dState, Mesh3dWidget};
//!
//! # fn draw(frame: &mut ratatui::Frame<'_>, area: ratatui::layout::Rect) -> ratatui_3dmesh::Result<()> {
//! let mesh = Mesh::load("model.obj")?;
//! let config = Mesh3dConfig::default().auto_fit(true);
//! let mut state = Mesh3dState::default();
//! frame.render_stateful_widget(Mesh3dWidget::new(&mesh).config(config), area, &mut state);
//! # Ok(())
//! # }
//! ```

pub mod animation;
pub mod config;
#[cfg(feature = "cli-example")]
pub mod controls;
pub mod error;
pub mod loader;
pub mod model;
pub mod render;
pub mod widget;

pub use animation::{
    AnimatedProperty, AnimationChannel, AnimationClip, AnimationNode, AnimationSampler,
    AnimationValue, Interpolation, MeshRange, NodeTransform, Quaternion, SkinBinding,
    SkinnedVertex,
};
pub use config::{ColorMode, Mesh3dConfig, ProjectionMode, RenderMode, TextureFilter, TextureWrap};
#[cfg(feature = "cli-example")]
pub use controls::{ControlAction, ControlMap};
pub use error::{Error, Result};
pub use loader::MeshLoadOptions;
pub use model::{AlphaMode, Bounds, Face, Material, Mesh, Texture, TextureRef, Vec2, Vec3};
pub use widget::{Mesh3dState, Mesh3dWidget};
