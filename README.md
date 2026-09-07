# io

Native isometric prototype with a Rust state core and a C/OpenGL renderer.

## Stack

- Rust `staticlib` for world state and normalized-space math
- C for the window loop and OpenGL drawing
- SDL2 for windowing and context creation
- OpenGL for the lightweight native renderer

## Concept

The scene is defined in a shared normalized space:

- `axis_a`
- `axis_b`
- `axis_c`
- `units_a`
- `units_b`
- `units_c`

Points are projected lazily from `{a, b, c}` into 2D screen space only when
the renderer needs them.

Rust owns the scene and item data. C requests plain line and point buffers over
an FFI boundary and renders them in a native window.

## Build

```bash
make
```

## Run

```bash
make run
```

## Controls

- Left-button drag to orbit horizontally and tilt vertically
- Mouse wheel / trackpad scroll to zoom (or `+` / `-`)
- `R` to reset the camera
- `Esc` to quit
- `Q` / `A` to increase or decrease `axis_a` units
- `W` / `S` to increase or decrease `axis_b` units
- `E` / `D` to increase or decrease `axis_c` units

## Camera

Rust owns an orthographic orbit camera. It maps normalized A/B/C coordinates
through three projected axis vectors, centered on the volume's midpoint.
The default view is isometric; orbiting changes the viewing angle. Zoom changes
the projection scale without changing grid units, item anchors, or footprints.
The camera fits the logical window on resize and limits zoom to 0.1x-10x and
elevation to 5-85 degrees. `R` restores the default angle and zoom at the current
window size. Wire thickness and the temporary item markers keep their screen size.

Axis vectors are recalculated when the camera changes; render buffers are
rebuilt lazily. C forwards [SDL mouse input](https://wiki.libsdl.org/SDL2/SDL_MouseWheelEvent)
and consumes the projected buffers. This remains a wireframe view with no hidden
surface removal; solid models will need depth-aware rendering.

## Display Quality

The window uses native Retina/high-DPI framebuffer dimensions while keeping
scene positions and stroke sizes in logical window units. It requests 4x MSAA,
retries with 2x if needed, then falls back to driver line smoothing if neither
multisample context can be created. Fallback quality depends on the driver.
Startup logs report the actual antialiasing mode and window/framebuffer sizes.

The macOS build embeds `NSHighResolutionCapable` metadata in the executable so
`make run` supports Retina rendering without requiring an app bundle. The sizing
follows SDL's [drawable-size guidance](https://wiki.libsdl.org/SDL2/SDL_GL_GetDrawableSize).
