# bevy_noodle

[![crates.io](https://img.shields.io/crates/v/bevy_noodle.svg)](https://crates.io/crates/bevy_noodle)
[![docs.rs](https://docs.rs/bevy_noodle/badge.svg)](https://docs.rs/bevy_noodle)
[![CI](https://github.com/gold-silver-copper/bevy_noodle/actions/workflows/ci.yml/badge.svg)](https://github.com/gold-silver-copper/bevy_noodle/actions/workflows/ci.yml)

A headless node graph library for [Bevy](https://bevyengine.org) UI.
**You build and style the nodes; bevy_noodle handles the graph.**

![A wire snapping onto a port](https://raw.githubusercontent.com/gold-silver-copper/bevy_noodle/main/docs/connecting.png)

- **Just entities.** Nodes and ports are your UI entities with a marker
  component; edges are Bevy relationships.
- **Draws nothing by default.** Draw edges from `EdgeGeometry` yourself, or
  enable the `default_style` look.
- **Any number of graphs**, side by side or nested inside nodes.
- **One edit pipeline** for UI and code, with observers that can veto or
  rewrite any edit.

Requires Bevy **0.19**.

## Examples

```sh
cargo run --example <name> --all-features
```

<table>
<tr>
<td width="50%"><img src="https://raw.githubusercontent.com/gold-silver-copper/bevy_noodle/main/docs/scene_builder_3d.png" alt="scene_builder_3d"><br><b>scene_builder_3d</b>: build a live 3D scene from a graph</td>
<td width="50%"><img src="https://raw.githubusercontent.com/gold-silver-copper/bevy_noodle/main/docs/edge_styles.png" alt="edge_styles"><br><b>edge_styles</b>: gradients, dashes and animated flow</td>
</tr>
<tr>
<td><img src="https://raw.githubusercontent.com/gold-silver-copper/bevy_noodle/main/docs/subgraph.png" alt="subgraph"><br><b>subgraph</b>: graphs nested inside nodes</td>
<td><img src="https://raw.githubusercontent.com/gold-silver-copper/bevy_noodle/main/docs/styled.png" alt="styled"><br><b>styled</b>: the default look with feathers controls</td>
</tr>
<tr>
<td><img src="https://raw.githubusercontent.com/gold-silver-copper/bevy_noodle/main/docs/math_graph.png" alt="math_graph"><br><b>math_graph</b>: a live calculator that rejects cycles</td>
<td><img src="https://raw.githubusercontent.com/gold-silver-copper/bevy_noodle/main/docs/editor.png" alt="editor"><br><b>editor</b>: undo/redo, copy/paste, edge selection</td>
</tr>
<tr>
<td><img src="https://raw.githubusercontent.com/gold-silver-copper/bevy_noodle/main/docs/type_conversion.png" alt="type_conversion"><br><b>type_conversion</b>: auto-inserted converter nodes</td>
<td><img src="https://raw.githubusercontent.com/gold-silver-copper/bevy_noodle/main/docs/comment_frames.png" alt="comment_frames"><br><b>comment_frames</b>: frames that carry their nodes</td>
</tr>
<tr>
<td><img src="https://raw.githubusercontent.com/gold-silver-copper/bevy_noodle/main/docs/reroute.png" alt="reroute"><br><b>reroute</b>: reroute dots on edges</td>
<td><img src="https://raw.githubusercontent.com/gold-silver-copper/bevy_noodle/main/docs/minimap.png" alt="minimap"><br><b>minimap</b>: a synced overview you can drag</td>
</tr>
<tr>
<td><img src="https://raw.githubusercontent.com/gold-silver-copper/bevy_noodle/main/docs/auto_layout.png" alt="auto_layout"><br><b>auto_layout</b>: layered layout as undoable edits</td>
<td><img src="https://raw.githubusercontent.com/gold-silver-copper/bevy_noodle/main/docs/keyboard.png" alt="keyboard"><br><b>keyboard</b>: Tab, arrows and Space to connect</td>
</tr>
<tr>
<td><img src="https://raw.githubusercontent.com/gold-silver-copper/bevy_noodle/main/docs/save_load.png" alt="save_load"><br><b>save_load</b>: save and load graphs as RON</td>
<td><img src="https://raw.githubusercontent.com/gold-silver-copper/bevy_noodle/main/docs/stress.png" alt="stress"><br><b>stress</b>: hundreds of nodes rewired every frame (use <code>--release</code>)</td>
</tr>
<tr>
<td><img src="https://raw.githubusercontent.com/gold-silver-copper/bevy_noodle/main/docs/minimal.png" alt="minimal"><br><b>minimal</b>: plain Bevy UI, edges drawn as gizmos</td>
<td></td>
</tr>
</table>

## Quick start

```rust
use bevy::prelude::*;
use bevy_noodle::prelude::*;

const NUMBER: PortType = PortType::named("number");

fn main() {
    App::new()
        .add_plugins((DefaultPlugins, NoodlePlugins))
        .add_systems(Startup, setup)
        .run();
}

fn setup(mut commands: Commands) {
    commands.spawn(Camera2d);
    let canvas = commands
        .spawn((
            NodeCanvas,
            CanvasInteraction::default(),
            Node { width: percent(100), height: percent(100), ..default() },
        ))
        .id();
    commands.spawn((
        GraphNode,
        NodePosition(Vec2::new(80.0, 120.0)),
        ChildOf(canvas),
        Node { padding: UiRect::all(px(10)), column_gap: px(8), ..default() },
        BackgroundColor(Color::srgb(0.16, 0.18, 0.22)),
        children![
            (Text::new("Source"), Pickable::IGNORE),
            (
                Port::output(NUMBER),
                Node { width: px(14), height: px(14), ..default() },
                BackgroundColor(Color::WHITE),
            ),
        ],
    ));
}
```

## How it works

```text
NodeCanvas              one graph and its viewport
└── CanvasContent       spawned by the canvas; pans and zooms (CanvasView), holds the nodes
    ├── GraphNode       your UI node
    │   └── … Port      your UI node marking a connection point
    └── Edge            relates an output port to an input port; has EdgeGeometry
```

- **Read** with the `GraphQuery` system param (`nodes_in`, `edges_of`,
  `peers_of`, …).
- **Write** with `commands.graph_edit(canvas, GraphEdit::Connect { from, to })`;
  also `Disconnect`, `MoveNodes` and `Delete`. Nodes are just spawned, as
  children of the canvas.
- **Select** with `commands.select(canvas, items, mode)`; selection is Bevy's
  `Selected` component, not an edit.
- **Validate** in `On<EditRequested>` observers: `allow()` or `reject()` any
  edit, including the built-in type rules.
- **React** to `EditApplied`.
- **Style** from `Selected`, `WireCandidate`, `WireTarget` and `SelectionBox`.
- **Controls inside nodes** (sliders, text fields, menus) keep their own
  presses and drags.

## Features

| Feature | Adds |
|---|---|
| `default_style` | Wire shader (`EdgeStyle`), `CanvasGrid`, selection box, port highlights, and `style::kit` node builders |
| `scene` | `DynamicWorld` snapshots for undo, copy/paste and save/load |

Keyboard use is a plugin: add `NoodleKeyboardPlugin` and `CanvasKeyboard`.

## License

MIT
