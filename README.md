# bevy_noodle

[![crates.io](https://img.shields.io/crates/v/bevy_noodle.svg)](https://crates.io/crates/bevy_noodle)
[![docs.rs](https://docs.rs/bevy_noodle/badge.svg)](https://docs.rs/bevy_noodle)

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

![The minimal example](https://raw.githubusercontent.com/gold-silver-copper/bevy_noodle/main/docs/minimal.png)
![The styled example](https://raw.githubusercontent.com/gold-silver-copper/bevy_noodle/main/docs/styled.png)
![The math_graph example](https://raw.githubusercontent.com/gold-silver-copper/bevy_noodle/main/docs/math_graph.png)

## Concepts

```text
NodeCanvas              your UI node: one graph and its viewport
└── CanvasContent       pans and zooms (driven by CanvasView); holds the nodes
    └── GraphNode       your UI node; NodePosition is optional (without it, your layout places it)
        └── … Port      your UI node marking a connection point
Edge                    EdgeSource → output port, EdgeTarget → input port, plus EdgeGeometry
PendingWire             the wire being dragged; also has EdgeGeometry
```

- **Reading.** `GraphQuery` is a system param: `nodes_of`, `ports_of`,
  `inputs_of`, `outputs_of`, `peers_of`, `edges_of`, `edge_ports`, `node_of`,
  `canvas_of`, `check_connection`.
- **Writing.** `commands.graph_edit(canvas, GraphEdit::Connect { from, to })`.
  The other edits are `Disconnect`, `MoveNodes`, `DeleteNodes` and `Select`.
  Spawning a node is just a spawn.
- **Rules.** Ports connect when their `PortType`s match (or one is
  `PortType::ANY`) and limits allow. Anything else is an observer:
  `On<EditRequested>` → `request.reject()`.
- **Reacting.** `EditApplied` is an entity event on the canvas and a message,
  with an `origin` (`Code`, `Interaction` or `Custom`). Drags end with a
  `MoveNodes { is_final: true, total, .. }`.
- **Interaction state** you can style: `Selected` on nodes, `WireCandidate` and
  `WireTarget` on ports, `SelectionBox` on the canvas. A `WireDropped` event
  fires when a wire is released over empty canvas.
- **Bindings** are just systems:
  `commands.graph_edit(canvas, GraphEdit::DeleteNodes { nodes: selected.iter().collect() })`.

### Optional default style (`default_style`)

Each piece is opt-in on the canvas or the entity:
- **`EdgeStyle`:** wires drawn above or below the nodes, ending at port rims.
  Put it on a canvas, or on a single edge to override.
- **`CanvasGrid`:** a background grid.
- **`SelectionBoxStyle`:** draws the selection box.
- **`PortHighlight` + `PortColor`:** ports show connection and drag state.
- **`SelectedBorderColor`:** a node border that follows selection.
- **`style::kit`:** plain functions returning node bundles.

## Migrating from 0.2

- Add `CanvasInteraction::default()` to canvases that should be interactive.
- `NoodleKeyBindingsPlugin`/`CanvasKeymap` are gone. Bind keys with a system,
  as in the `styled` example.
- `NodeFinder` is gone. Handle `WireDropped`, or a click on the canvas, and
  spawn nodes yourself.
- The `DeleteSelection`, `SelectAll`, `ClearSelection`, `PanBy`, `ZoomBy` and
  `CancelInteraction` actions are gone. Use `graph_edit`, or change `CanvasView`
  directly. `FrameAll` remains.
- `Edge` is now a marker: find an edge's canvas with `GraphQuery::canvas_of`.
- `sources_of`/`targets_of` became `peers_of`.
- `CanvasWantsInput` is gone. `CanvasInteraction` requires `Hovered`; read
  that instead.
- `kit` functions take no theme argument.

## License

MIT
