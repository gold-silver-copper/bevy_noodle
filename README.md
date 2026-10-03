# bevy_noodle

[![crates.io](https://img.shields.io/crates/v/bevy_noodle.svg)](https://crates.io/crates/bevy_noodle)
[![docs.rs](https://docs.rs/bevy_noodle/badge.svg)](https://docs.rs/bevy_noodle)
[![CI](https://github.com/gold-silver-copper/bevy_noodle/actions/workflows/ci.yml/badge.svg)](https://github.com/gold-silver-copper/bevy_noodle/actions/workflows/ci.yml)

A minimal, headless node graph library for [Bevy](https://bevyengine.org) UI.
**You build and style the nodes and edges; bevy_noodle handles the graph.**

![A wire snapping onto a port, with the optional default style](https://raw.githubusercontent.com/gold-silver-copper/bevy_noodle/main/docs/connecting.png)

- **Everything is an entity.** Nodes are your UI entities tagged `GraphNode`.
  Ports are any UI entities tagged `Port`. Edges are entities related to ports
  through Bevy relationships, so despawning a node or port removes its edges.
- **Nothing forced.** No background, no drawing, no key bindings. Pointer
  interaction is opt-in per canvas (`CanvasInteraction`), with every button
  configurable. The core only computes `EdgeGeometry`, which you draw however
  you like, or you opt into the `default_style` look.
- **Multiple graphs.** Each `NodeCanvas` is an independent graph. Canvases can
  sit side by side or nest inside nodes.
- **One edit pipeline.** UI and code edits both go through validation, then
  `EditRequested` (observers can veto or rewrite the edit), then `EditApplied`.
- **Bevy-native.** It uses Bevy picking events (any pointer), relationships,
  `Selected`/`InteractionDisabled`, observers, required components, and
  automatic `Reflect` registration.

Requires Bevy **0.19**.

## Quick start

```rust
use bevy::prelude::*;
use bevy_noodle::prelude::*;

const NUMBER: PortType = PortType::named("number");

fn main() {
    App::new().add_plugins((DefaultPlugins, NoodlePlugins)).add_systems(Startup, setup).run();
}

fn setup(mut commands: Commands) {
    commands.spawn(Camera2d);
    let canvas = commands
        .spawn((NodeCanvas, CanvasInteraction::default(), Node { width: percent(100), height: percent(100), ..default() }))
        .id();
    let content = commands.spawn((CanvasContent, ChildOf(canvas))).id();
    commands.spawn((
        GraphNode,
        NodePosition(Vec2::new(80.0, 120.0)),
        ChildOf(content),
        Node { padding: UiRect::all(px(10)), column_gap: px(8), ..default() },
        BackgroundColor(Color::srgb(0.16, 0.18, 0.22)),
        children![
            (Text::new("Source"), Pickable::IGNORE),
            (Port::output(NUMBER), Node { width: px(14), height: px(14), ..default() }, BackgroundColor(Color::WHITE)),
        ],
    ));
}
```

## Examples

| Example | Shows |
|---|---|
| `cargo run --example minimal` | Plain Bevy UI nodes, with edges drawn as gizmos from `EdgeGeometry`. |
| `cargo run --example styled --features default_style` | The default look, plus app-level bindings: right-click adds a node, a dropped wire spawns a matching node, Delete removes the selection. |
| `cargo run --example math_graph --features default_style` | A live calculator: your own components, Bevy's `EditableText` inside nodes, evaluation with `GraphQuery`, and a veto that rejects cycles. |
| `cargo run --example edge_styles --features default_style` | Every `EdgeStyle` option: gradients, dashes, marching ants and travelling pulses, animated in the shader. Edges copy their look from the node they leave. |
| `cargo run --example subgraph --features default_style` | Graphs of graphs: a Group node holds its own canvas, with In/Out nodes carrying values across the boundary. |
| `cargo run --example scene_builder_3d --features default_style` | A graph panel over a 3D view, building `Mesh3d` entities (shapes, colors, spin, rings) whenever an edit applies. |
| `cargo run --example editor --features default_style,scene` | Editor commands in app code: undo/redo from snapshots, copy/paste/duplicate, selecting and deleting edges, right-click to remove an edge. |
| `cargo run --example keyboard --features default_style` | Keyboard-only use: Tab between nodes and ports, Enter to select, arrows to move, Space on two ports to connect, with a focus outline. |
| `cargo run --example comment_frames --features default_style` | Comment frames: a frame node moves the nodes inside it, through an `EditApplied` observer with its own `EditOrigin`. |
| `cargo run --example reroute --features default_style` | Reroute dots for routing edges; right-click an edge to insert one where you clicked. |
| `cargo run --example type_conversion --features default_style` | An `EditRequested` observer vetoes connections a node does not accept and inserts an "int to float" converter. |
| `cargo run --example minimap --features default_style` | A minimap of the same graph, kept in sync from `NodePosition`s and `CanvasView`; click or drag it to move the view. |
| `cargo run --example auto_layout --features default_style` | A layered automatic layout applied as one undoable `MoveNodes` edit per node (L to lay out, S to scramble). |
| `cargo run --release --example stress --features default_style` | A self-driving stress test: hundreds of nodes spawned, wired, moved, rewired and deleted every frame while the camera drifts, with FPS and edits per second on screen. Space pauses, Up/Down change the size. |
| `cargo run --example save_load --features default_style,scene` | Saving and loading as RON: a full snapshot (user components and entity references intact) or just the graph model, a few hundred bytes, rebuilt with ordinary spawns. |

![The minimal example](https://raw.githubusercontent.com/gold-silver-copper/bevy_noodle/main/docs/minimal.png)
![The styled example](https://raw.githubusercontent.com/gold-silver-copper/bevy_noodle/main/docs/styled.png)
![The math_graph example](https://raw.githubusercontent.com/gold-silver-copper/bevy_noodle/main/docs/math_graph.png)
![The edge_styles example](https://raw.githubusercontent.com/gold-silver-copper/bevy_noodle/main/docs/edge_styles.png)
![The subgraph example](https://raw.githubusercontent.com/gold-silver-copper/bevy_noodle/main/docs/subgraph.png)
![The scene_builder_3d example](https://raw.githubusercontent.com/gold-silver-copper/bevy_noodle/main/docs/scene_builder_3d.png)
![The editor example](https://raw.githubusercontent.com/gold-silver-copper/bevy_noodle/main/docs/editor.png)
![The save_load example](https://raw.githubusercontent.com/gold-silver-copper/bevy_noodle/main/docs/save_load.png)
![The stress example](https://raw.githubusercontent.com/gold-silver-copper/bevy_noodle/main/docs/stress.png)
![The keyboard example](https://raw.githubusercontent.com/gold-silver-copper/bevy_noodle/main/docs/keyboard.png)
![The comment_frames example](https://raw.githubusercontent.com/gold-silver-copper/bevy_noodle/main/docs/comment_frames.png)
![The reroute example](https://raw.githubusercontent.com/gold-silver-copper/bevy_noodle/main/docs/reroute.png)
![The type_conversion example](https://raw.githubusercontent.com/gold-silver-copper/bevy_noodle/main/docs/type_conversion.png)
![The minimap example](https://raw.githubusercontent.com/gold-silver-copper/bevy_noodle/main/docs/minimap.png)
![The auto_layout example](https://raw.githubusercontent.com/gold-silver-copper/bevy_noodle/main/docs/auto_layout.png)

## Concepts

```text
NodeCanvas              your UI node: one graph and its viewport
└── CanvasContent       pans and zooms (driven by CanvasView); holds the nodes
    ├── GraphNode       your UI node; NodePosition is optional (without it, your layout places it)
    │   └── … Port      your UI node marking a connection point
    └── Edge            EdgeSource → output port, EdgeTarget → input port, plus EdgeGeometry
PendingWire             the wire being dragged; also has EdgeGeometry
```

- **Reading.** `GraphQuery` is a system param: `nodes_in`, `edges_in`,
  `ports_of`, `inputs_of`, `outputs_of`, `peers_of`, `edges_of`, `edge_ports`,
  `node_of`, `canvas_of`, `check_connection`.
- **Writing.** `commands.graph_edit(canvas, GraphEdit::Connect { from, to })`.
  The other edits are `Disconnect`, `MoveNodes`, `Delete` and `Select`.
  Spawning a node is just a spawn.
- **Rules.** Ports connect when their `PortType`s match (or one is
  `PortType::ANY`), the ports are not connected yet, and limits allow. That
  verdict reaches `On<EditRequested>` observers as `refused`, and they have
  the last word: `request.allow()` (e.g. ints into floats) or
  `request.reject()`. Only connections that cannot exist at all (missing
  ports, ports of one node, two inputs) never get there. Dragged wires snap to
  the ports observers would allow: the library asks them with
  `preview: true`, so do nothing irreversible then (`world.preview_edit` asks
  the same way).
- **Reacting.** `EditApplied` is an entity event on the canvas and a message,
  with an `origin` (`Code`, `Interaction` or `Custom`), the `created` edge and
  the `(output, input)` `ports` of a connect or disconnect. Drags end with a
  `MoveNodes { is_final: true, total, .. }`.
- **Interaction state** you can style: `Selected` on nodes and edges,
  `WireCandidate` and `WireTarget` on ports, `SelectionBox` on the canvas. A
  `WireDropped` event fires when a wire is released over empty canvas.
- **Edges are pickable.** An edge with an `EdgeHitbox` (the default style adds
  one) gets `Pointer` events like any UI entity, from a small picking backend
  that respects overlays, clipping and ports. Clicking selects it, and
  `Delete` removes listed edges along with nodes.
- **Bindings** are just systems:
  `commands.graph_edit(canvas, GraphEdit::Delete { items: selected.iter().collect() })`.

### Optional default style (`default_style`)

Each piece is opt-in on the canvas or the entity:
- **`EdgeStyle`:** wires drawn above or below the nodes, ending at port rims,
  with optional gradients (`end_color`), dashes (`dash`) and animated flow
  (`flow_speed`), all in the shader, and hover and selection highlights. Put
  it on a canvas, or on a single edge to override.
- **`CanvasGrid`:** a background grid.
- **`SelectionBoxStyle`:** draws the selection box.
- **`PortHighlight` + `PortColor`:** ports show connection and drag state.
- **`SelectedBorderColor`:** a node border that follows selection.
- **`style::kit`:** plain functions returning bundles: `kit::canvas()` (an
  interactive canvas with the whole look), a node frame, a title and port rows.
  `kit::input_dot`/`kit::output_dot` are rows without labels: no text to lay
  out, for very large graphs.

### Keyboard

Add `NoodleKeyboardPlugin` and put `CanvasKeyboard` on a canvas. It builds on
Bevy's `bevy_input_focus`: the canvas is a `TabGroup` and its nodes and ports
get a `TabIndex`, so Tab and Shift+Tab move focus between them. Enter selects
the focused node and the arrow keys move it; with Ctrl/Cmd held they pan the
view, and +/- zoom it. Space on a port starts a connection and Space on a
second port completes it (focusing a port it may connect to snaps the wire);
Escape drops it. Every key is a field, and `None` unbinds it. With the default
style, `FocusOutline` (part of `kit::canvas()`) outlines the keyboard focus.

### Snapshots (`scene`)

Snapshots are Bevy `DynamicWorld`s of nodes, ports, edges, nested canvases and
your reflected components, with entity references remapped:
- `scene::snapshot(world, canvas)` captures a whole graph, and
  `scene::restore(world, canvas, &snapshot)` puts it back (undo, loading).
- `scene::snapshot_nodes(world, &nodes)` captures some nodes and the edges
  between them, and `scene::insert(world, canvas, &snapshot)` adds them to any
  graph (copy and paste).
- Save to files with `DynamicWorld::serialize`.

See the `editor` and `save_load` examples.

## Migrating from 0.2

**Interaction and bindings**
- Interaction is opt-in: add `CanvasInteraction::default()` to canvases that
  should respond to the pointer. Its buttons and modifier keys are fields.
- `NoodleKeyBindingsPlugin`/`CanvasKeymap` are gone: bind keys with a system,
  as in the `styled` example.
- The `DeleteSelection`, `SelectAll`, `ClearSelection`, `PanBy`, `ZoomBy` and
  `CancelInteraction` actions are gone: use `graph_edit`, or change
  `CanvasView`. `FrameAll` remains.
- `CanvasWantsInput` is gone: `CanvasInteraction` requires `Hovered`; read that.
- The node finder popup is gone: handle `WireDropped` (or a canvas click) and
  spawn nodes yourself, as in the `styled` example.

**Edits**
- `GraphEdit::DeleteNodes { nodes }` is `GraphEdit::Delete { items }`, and
  `GraphEdit::Select { nodes, mode }` is `GraphEdit::Select { items, mode }`.
  Both take nodes and edges.
- `EditApplied` has a `ports: Option<(output, input)>` field for connects and
  disconnects.
- `GraphCommandsExt`/`GraphWorldExt` only need `graph_edit_with_origin`
  implemented; `graph_edit` is provided. `EditResult` names the return type.

**Graph structure**
- `Edge` is a marker (no `canvas` field) and edges are children of the
  canvas's `CanvasContent`. Find an edge's canvas with `GraphQuery::canvas_of`.
- `PortAnchor::measured` is gone: `PortAnchor::position` is `Some` once measured.
- `GraphQuery`: `nodes_of` is `nodes_in`; `sources_of`/`targets_of` are
  `peers_of`; `is_node`, `subtree_any` and `is_connected` are gone (use
  `node_of`, `edges_of`); `edges_in` is new.

**Default style (`default_style`)**
- `EdgeStyle` has new fields (`end_color`, `dash`, `flow_speed`, `layer`,
  `trim_to_ports`, `selected_color`, `hover_width`): build it with
  `..default()`.
- `kit` functions take no `KitTheme`; `kit::port_dot` is `kit::port`.
- `kit::body` is gone: put `kit::input`/`kit::output` rows straight into the
  node after `kit::title`. Give other content a horizontal margin of
  `kit::PADDING`.
- `kit::canvas()` is new: an interactive, clipped canvas with the whole look.

## License

MIT
