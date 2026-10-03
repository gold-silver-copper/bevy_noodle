# bevy_noodle

[![crates.io](https://img.shields.io/crates/v/bevy_noodle.svg)](https://crates.io/crates/bevy_noodle)
[![docs.rs](https://docs.rs/bevy_noodle/badge.svg)](https://docs.rs/bevy_noodle)

A headless, entity-based node graph library for [Bevy](https://bevyengine.org) UI.
**You build and style the nodes; bevy_noodle handles the graph.**

![A wire snapping onto a port, with the optional default style](https://raw.githubusercontent.com/gold-silver-copper/bevy_noodle/main/docs/connecting.png)

- **Nodes, ports and edges are entities.** A node is your own UI entity tagged
  `GraphNode`. Style it with any Bevy UI component, attach your own components,
  query it, react to `Changed`.
- **Nothing is drawn unless you ask.** The core spawns no background, no grid,
  no wires and no chrome. It computes `EdgeGeometry` for every edge; draw edges
  however you like (UI, gizmos, meshes), or opt in to the default look.
- **No bindings forced.** The core reads no keyboard shortcuts. Editor
  operations (delete selection, select all, frame all, pan, zoom) are events
  you trigger from any input crate. Default key bindings are a separate, opt-in
  plugin that only listens while its canvas has focus.
- **One edit pipeline, with a veto.** Every change, from the pointer or your
  code, goes through validation, then an `EditRequested` event your observers can
  reject or rewrite, then `EditApplied` (an entity event and a message).
- **Any pointer.** Built on `bevy_picking` events, so mouse, touch, pen and
  render-to-texture all work.
- **Configurable per canvas.** Every interaction can be switched off or
  rebound, and `InteractionDisabled` turns off a whole canvas, node or port.
- **Inspector-friendly.** All components derive and register `Reflect`.

Requires Bevy **0.19**.

## Quick start

```toml
[dependencies]
bevy = "0.19"
bevy_noodle = "0.2"
```

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

    // The canvas is your UI node; its content child pans and zooms.
    let canvas = commands
        .spawn((NodeCanvas, Node { width: percent(100), height: percent(100), ..default() }))
        .id();
    let content = commands.spawn((CanvasContent, ChildOf(canvas))).id();

    // A node is any UI you like, marked `GraphNode`. Ports are any UI entities marked `Port`.
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

Nodes can now be dragged, selected and connected. Edges exist as entities
with an `EdgeGeometry`, but nothing draws them until you do (see
[`examples/minimal.rs`](examples/minimal.rs), which uses gizmos) or you enable
the default style.

## Examples

### minimal

```sh
cargo run --example minimal
```

No default style at all: hand-built nodes, edges drawn with gizmos from
`EdgeGeometry`, and selection shown by the user's own system reacting to `Selected`.

![The minimal example](https://raw.githubusercontent.com/gold-silver-copper/bevy_noodle/main/docs/minimal.png)

### styled

```sh
cargo run --example styled --features default_style
```

The optional default look, opted into piece by piece on the canvas:
Bézier wires, a grid, a selection box, a node finder and default key bindings.

![The styled example](https://raw.githubusercontent.com/gold-silver-copper/bevy_noodle/main/docs/styled.png)

### math_graph

```sh
cargo run --example math_graph --features default_style
```

A live calculator. It shows the headless pattern end to end:
- your own components on nodes (`MathOp`, `NumberValue`)
- Bevy's text input inside a node
- evaluation through `GraphQuery`
- an `EditRequested` observer that rejects connections that would create a cycle

![The math_graph example](https://raw.githubusercontent.com/gold-silver-copper/bevy_noodle/main/docs/math_graph.png)

## Concepts

### The graph is entities

```text
NodeCanvas                 your UI node: the viewport (no background)
└── CanvasContent          pans and zooms; holds the nodes
    ├── GraphNode          your UI node: style it however you like
    │   └── … Port         your UI node marking a connection point
    └── GraphNode …
Edge                       spawned on connect: EdgeSource → output port, EdgeTarget → input port
```

You insert `NodeCanvas`, `CanvasContent`, `GraphNode` (+ `NodePosition`) and
`Port`. On your entities, the library writes only `left`/`top` of each node's
`Node` and the content's `UiTransform`.

Edges are Bevy relationships (`EdgeSource`/`EdgeTarget`), so despawning a
port or node automatically removes its edges.

### Reading the graph

`GraphQuery` is a system parameter with the lookups you need for evaluation:

| Method | Returns |
|---|---|
| `nodes_of(canvas)` | the canvas' nodes |
| `ports_of(node)`, `inputs_of(node)`, `outputs_of(node)` | a node's ports |
| `sources_of(input)` | output ports feeding an input |
| `targets_of(output)` | input ports an output feeds |
| `edges_of(port)`, `edge_ports(edge)` | edges and their ends |
| `node_of(entity)`, `canvas_of(entity)` | the node or canvas an entity is in |

### Changing the graph

```rust
commands.graph_edit(canvas, GraphEdit::Connect { from: output, to: input });
```

Edits are `Connect`, `Disconnect`, `MoveNodes`, `DeleteNodes` and `Select`.
Spawning a node is not an edit: spawn it like any entity.

Every edit runs through one pipeline:
1. **Validation.** Ports exist, directions differ, types match (equal
   `PortType`s, or `PortType::ANY`), limits hold. A failure triggers
   `EditRejected` with a reason.
2. **`EditRequested`** is triggered on the canvas. Observers may `reject()`
   it or rewrite `edit` (for example, snapping moves to a grid).
3. **The edit is applied.**
4. **`EditApplied`** is triggered on the canvas and written as a message, with
   an `origin` (`Code`, `Interaction` or `Custom`) so undo stacks and netcode
   can skip their own echoes.

Interactive moves stream `MoveNodes { is_final: false }` and end with one
`is_final: true` edit carrying the gesture's `total`; record that one for undo.

```rust
// Rules are just observers.
app.add_observer(|mut request: On<EditRequested>, graph: GraphQuery| {
    if let GraphEdit::Connect { from, to } = request.edit {
        if would_create_cycle(&graph, from, to) {
            request.reject();
        }
    }
});
```

### Actions: no bindings in the core

Editor operations are entity events. Trigger them from anything:

```rust
commands.trigger(DeleteSelection { canvas });
commands.trigger(FrameAll { canvas, padding: 40.0 });
```

The available actions are `DeleteSelection`, `SelectAll`, `ClearSelection`,
`FrameAll`, `PanBy`, `ZoomBy` and `CancelInteraction`.

For keys, add `NoodleKeyBindingsPlugin` and a `CanvasKeymap` on the canvas:

```rust
CanvasKeymap::default()                          // Delete, Ctrl/Cmd+A, Escape, Ctrl/Cmd+0
CanvasKeymap::empty().with(KeyBinding::new(KeyCode::KeyX, CanvasAction::DeleteSelection))
```

Keys only reach a canvas while it has focus, and handled keys stop
propagating, so your app's own shortcuts are unaffected.

### Pointer interaction

`CanvasInteraction` on each canvas configures:
- `pan_button` (middle by default) and `box_select_button` (primary by default)
- node dragging and connecting
- picking up a wire by dragging it off an input
- selection on press and raising pressed nodes
- additive-selection and zoom modifier keys
- `ScrollMode`, pinch zoom, zoom limits and the wire snap distance

`CanvasInteraction::none()` turns everything off, so you can enable only what
you want.

**While dragging, the library sets state you can style or read:**
- `WireSource`, `WireCandidate` and `WireTarget` markers on ports
- `PendingWire` (with geometry) and `SelectionBox` on the canvas
- `Selected` on selected nodes

**Other signals:**
- `WireDropped` fires when a wire is released over empty canvas.
- `CanvasWantsInput` and the `canvas_wants_pointer_input` run condition tell
  the rest of your app when the graph is using the pointer.

### Optional default style (`default_style` feature)

Nothing applies unless opted in per entity:

| Add | To get |
|---|---|
| `EdgeStyle` on a canvas (or an edge, to override) | anti-aliased Bézier wires, plus the wire being dragged. Drawn above the nodes and ending at each port's rim by default (`layer: EdgeLayer::BelowNodes` and `trim_to_ports` change that) |
| `CanvasGrid` on a canvas | a grid that pans and zooms (transparent background by default) |
| `SelectionBoxStyle` on a canvas | a visible selection box |
| `PortHighlight` + `PortColor` on a port | connection-state highlighting while dragging |
| `SelectedBorderColor` on a node | a border that follows selection |
| `NodeFinder` on a canvas | a searchable "add node" popup (right-click, or drop a wire on empty canvas; only templates that accept the wire are listed) |

`style::kit` has plain functions returning node bundles (`node`, `title`,
`body`, `input`, `output`, `port_dot`). Use them, copy them, or ignore them.

![The node finder, filtered to nodes that accept the dropped wire](https://raw.githubusercontent.com/gold-silver-copper/bevy_noodle/main/docs/node_finder.png)

## Plugins

| Plugin | Contents |
|---|---|
| `NoodlePlugins` | Plugin group: `NoodleCorePlugin` + `NoodleInteractionPlugin`. Use `.disable::<NoodleInteractionPlugin>()` for a data-only graph. |
| `NoodleKeyBindingsPlugin` | Opt-in, separate. |
| `NoodleDefaultStylePlugin` | Opt-in, separate (feature `default_style`). |

All systems run in `PostUpdate` in the `NoodleSystems::{Sync, Render, Measure}`
sets around UI layout. Interaction is observer-driven, so idle canvases cost
almost nothing.

## Known limitations

- Text inside nodes is rasterized at its unzoomed size, so it softens when
  zoomed in past 100%.
- No built-in undo stack or serialization yet. `EditApplied` (with `origin`
  and final moves) is designed for building them.

## License

MIT
