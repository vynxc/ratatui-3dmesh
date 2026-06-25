# Home

`ratatui-3dmesh` is an embeddable Ratatui widget for rendering 3D OBJ and glTF/GLB meshes as terminal ASCII.

![ratatui-3dmesh rendering a textured fox glTF model in a terminal](images/ratatui-3dmesh-viewer-fox.png)

![Animated demo of ratatui-3dmesh rotating a textured fox glTF model](images/ratatui-3dmesh-viewer-fox.gif)

## Quick links

- [Getting Started](Getting-Started.md)
- [Embedding in Ratatui](Embedding-in-Ratatui.md)
- [Configuration](Configuration.md)
- [Model Formats](Model-Formats.md)
- [Roadmap](Roadmap.md)

## Design goals

- Work as a widget inside existing Ratatui apps.
- Provide good defaults and many typed customization options.
- Support OBJ (with companion MTL) and glTF/GLB, including PBR material semantics.
- Keep terminal initialization outside the library.
