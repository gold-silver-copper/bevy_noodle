# bevy_noodle

[![crates.io](https://img.shields.io/crates/v/bevy_noodle.svg)](https://crates.io/crates/bevy_noodle)
[![docs.rs](https://docs.rs/bevy_noodle/badge.svg)](https://docs.rs/bevy_noodle)

A generic, typed node graph editor for [Bevy](https://bevyengine.org) UI, in the
spirit of [egui_node_graph2](https://github.com/trevyn/egui_node_graph2).

![Dragging a new wire in the math_graph example](https://raw.githubusercontent.com/gold-silver-copper/bevy_noodle/main/docs/connecting.png)

The crate is the *editor*: nodes with typed input and output ports, wires, inline
value widgets, a searchable node finder, selection, panning and zooming. What
your graph *means* (a shader, a dialogue tree, an AI pipeline, a calculator) and
how you evaluate or compile it is up to you.

- **Typed ports.** Wires only connect compatible types, and you can define your own
  compatibility rules (e.g. implicit conversions).
- **Inline constants.** Unconnected inputs show a text, number, vector, checkbox or
  choice widget. Number labels can be dragged sideways to scrub the value.
- **Inputs with many wires**, constant-only parameters, and connection-only parameters.
- **Custom node UI.** Spawn any Bevy UI at the bottom of a node.
- **Node finder.** Right-click the canvas, or drop a wire on empty space. In the
  second case only templates that accept the wire are listed, and the new node is
  connected for you.
- **Box selection, multi-node dragging, keyboard deletion, pan and zoom** (mouse,
  trackpad scroll and pinch).
- **Crisp rendering.** Wires and the grid are anti-aliased shaders.
- **Pure-data graph.** The graph has no UI types inside, so you can build and evaluate
  it headless. With the `serde` feature it serializes.
- **Several editors at once**, of the same or of different graph types.

Requires Bevy **0.19**.

## Quick start

```toml
[dependencies]
bevy = "0.19"
bevy_noodle = "0.1"
```

Describe your graph with four small trait impls, tie them together with a
`NodeGraphSchema`, add the plugin and spawn the editor:

```rust
use std::borrow::Cow;
use bevy::prelude::*;
use bevy_noodle::prelude::*;

// What flows over wires (decides wire colors and what may connect).
#[derive(Clone, Copy, PartialEq, Eq)]
struct Number;

impl DataTypeTrait for Number {
    fn color(&self) -> Color { Color::srgb(0.3, 0.6, 1.0) }
    fn name(&self) -> Cow<'_, str> { "number".into() }
}

// The constant stored in each input, edited inline when unconnected.
#[derive(Clone)]
struct Value(f64);

impl WidgetValueTrait for Value {
    fn value_widget(&self, _param_name: &str) -> ValueWidget {
        ValueWidget::Number(NumberField::new(self.0))
    }
    fn apply_edit(&mut self, edit: ValueEdit) {
        if let ValueEdit::Number { value, .. } = edit {
            self.0 = value;
        }
    }
}

// The node kinds offered in the node finder.
#[derive(Clone, Copy)]
enum Template { Constant, Add }

impl NodeTemplateTrait for Template {
    type NodeData = NodeData;

    fn node_finder_label(&self) -> Cow<'_, str> {
        match self { Template::Constant => "Constant".into(), Template::Add => "Add".into() }
    }
    fn user_data(&self) -> NodeData { NodeData }
    fn build_node(&self, graph: &mut GraphOf<NodeData>, node: NodeId) {
        let kind = InputParamKind::ConnectionOrConstant;
        match self {
            Template::Constant => {
                graph.add_input_param(node, "value", Number, Value(1.0), InputParamKind::ConstantOnly, true);
            }
            Template::Add => {
                graph.add_input_param(node, "a", Number, Value(0.0), kind, true);
                graph.add_input_param(node, "b", Number, Value(0.0), kind, true);
            }
        }
        graph.add_output_param(node, "out", Number);
    }
}

// Per-node user data.
#[derive(Clone)]
struct NodeData;

impl NodeDataTrait for NodeData {
    type DataType = Number;
    type ValueType = Value;
}

// Ties the types together.
struct MyGraph;

impl NodeGraphSchema for MyGraph {
    type NodeData = NodeData;
    type NodeTemplate = Template;
}

fn main() {
    App::new()
        .add_plugins((DefaultPlugins, NodeGraphPlugin::<MyGraph>::default()))
        .add_systems(Startup, |mut commands: Commands| {
            commands.spawn(Camera2d);
            commands.spawn(NodeGraphEditor::<MyGraph>::new([Template::Constant, Template::Add]));
        })
        .run();
}
```

This is [`examples/minimal.rs`](examples/minimal.rs).

## Examples

### minimal

```sh
cargo run --example minimal
```

The smallest complete integration: two node kinds, inline number widgets, and
responses logged to the console.

![The minimal example](https://raw.githubusercontent.com/gold-silver-copper/bevy_noodle/main/docs/minimal.png)

### math_graph

```sh
cargo run --example math_graph
```

A port of egui_node_graph2's example over scalars and 2D vectors. It uses finder
categories, vector widgets, and live evaluation: each node shows its computed
value in a custom body.

![The math_graph example](https://raw.githubusercontent.com/gold-silver-copper/bevy_noodle/main/docs/math_graph.png)

### dialogue

```sh
cargo run --example dialogue
```

A dialogue tree editor. It shows multiline text, choice and checkbox widgets,
constant-only parameters, inputs that take many wires, an undeletable Start
node, `frame_all`, and walking the graph to build the script preview on the
right.

![The dialogue example](https://raw.githubusercontent.com/gold-silver-copper/bevy_noodle/main/docs/dialogue.png)

## Concepts

| egui_node_graph2 | bevy_noodle |
|---|---|
| `Graph<NodeData, DataType, ValueType>` | `Graph` (same shape: `nodes`, `inputs`, `outputs`, `connections`) |
| `DataTypeTrait` | `DataTypeTrait` (`color`, `name`, plus an overridable `is_compatible_with`) |
| `WidgetValueTrait::value_widget` draws a widget | `WidgetValueTrait::value_widget` *describes* one (`ValueWidget`), and `apply_edit` receives the edits |
| `NodeDataTrait::bottom_ui` | `NodeDataTrait::spawn_body` (rebuilt when `body_revision` changes) |
| `NodeTemplateTrait` | `NodeTemplateTrait` |
| `NodeTemplateIter` | the template list passed to `NodeGraphEditor::new` |
| `GraphEditorState` | `GraphEditorState` inside the `NodeGraphEditor` component |
| `draw_graph_editor` returning `NodeResponse`s | `NodeGraphResponse` messages |
| five generic parameters | one `NodeGraphSchema` marker type |

### Reading and changing the graph

`NodeGraphEditor` is an ordinary component. Query it from any system. The graph,
node positions, selection and camera live in `editor.state`, and changes made
from code show up in the UI the same frame:

```rust
fn add_node_on_key(keys: Res<ButtonInput<KeyCode>>, mut editors: Query<&mut NodeGraphEditor<MyGraph>>) {
    if keys.just_pressed(KeyCode::KeyN) {
        let mut editor = editors.single_mut().unwrap();
        let at = editor.pointer_world_position().unwrap_or_default();
        editor.state.add_node(&Template::Add, at);
    }
}
```

Other helpers: `try_connect`, `remove_node`, `select_only`, `frame_all`,
`open_finder`, `port_screen_position` and `node_screen_rect`.

### Reacting to edits

Every user action arrives as a `NodeGraphResponse<YourSchema>` message after it
has been applied to the graph: connects and disconnects, created, deleted, moved
and selected nodes, and `ValueChanged` for inline widget edits. This is the
natural place to re-evaluate your graph:

```rust
fn evaluate(mut responses: MessageReader<NodeGraphResponse<MyGraph>>, editors: Query<&NodeGraphEditor<MyGraph>>) {
    if responses.read().count() == 0 {
        return;
    }
    for editor in &editors {
        // Walk editor.graph(): follow graph.connection(input) to upstream outputs,
        // or read graph.get_input(input).value for unconnected inputs.
    }
}
```

### Inline widgets

| `ValueWidget` | UI | Edits |
|---|---|---|
| `None` | the parameter name only | none |
| `Label(text)` | read-only text | none |
| `Text { value, multiline }` | text field | `ValueEdit::Text` |
| `Number(NumberField)` | number field, label scrubs | `ValueEdit::Number` |
| `Numbers(Vec<NumberField>)` | a row of number fields (vectors, colors, ...) | `ValueEdit::Number { component, .. }` |
| `Bool(b)` | checkbox | `ValueEdit::Bool` |
| `Choice { options, selected }` | `<` value `>` cycler | `ValueEdit::Choice` |

`NumberField` supports `range`, `speed` (scrub sensitivity), `decimals` and
`integer` fields. Text fields are Bevy's own `EditableText`, so selection,
clipboard and IME all work.

### Custom node UI

Implement `NodeDataTrait::spawn_body` to add anything under a node's parameters.
Return a new `body_revision` whenever the body should be rebuilt; only the body
is rebuilt, so the parameter widgets keep focus. See `math_graph`, which shows
each node's evaluated value this way.

### Styling and settings

Insert a `NodeGraphStyle` component on the editor entity to change colors, font,
sizes and the grid. `NodeGraphEditor::with_settings` takes a `NodeGraphSettings`:
zoom limits, scroll behavior (zoom or pan), whether a primary-button drag pans
or box-selects, wire snapping distance, and whether dropping a wire opens the
finder.

### Saving and loading

Enable the `serde` feature, derive `Serialize`/`Deserialize` on your types, and
persist `editor.state` (a `GraphEditorState`). Restore it with
`NodeGraphEditor::new(templates).with_state(state)`.

## Controls

| Action | Input |
|---|---|
| Add a node | Right-click the canvas, or drop a wire on empty canvas |
| Connect | Drag from a port to a compatible port |
| Disconnect | Drag a wire off its input |
| Select | Click a node. Shift/Ctrl/Cmd+click toggles. Drag on empty canvas to box-select |
| Move | Drag a node (moves the whole selection) |
| Delete | The × in the title bar, or Delete/Backspace |
| Pan | Middle-drag, Space+drag, or two-finger scroll |
| Zoom | Mouse wheel, Ctrl/Cmd+scroll, or pinch |
| Select all / frame all | Ctrl/Cmd+A, Ctrl/Cmd+0 |
| Node finder | Type to filter, Enter adds the first match, Esc closes |

Keyboard shortcuts go to the editor under the pointer and are ignored while a
text field has focus.

![The node finder, filtered to nodes that accept the dragged wire](https://raw.githubusercontent.com/gold-silver-copper/bevy_noodle/main/docs/node_finder.png)

## How it works

- **Rendering is plain `bevy_ui`.** Each node is a UI hierarchy inside a "world"
  container. Pan and zoom are one `UiTransform` on that container, so custom node
  UI needs no special handling.
- **Wires are `UiMaterial`s.** Each wire is one UI node covering the curve's bounding
  box, with a fragment shader that draws an anti-aliased cubic Bézier. The grid is
  a shader too.
- **Port positions are read back after layout**, so wires attach to wherever the
  ports actually end up, whatever your widgets do to the node's size.
- **Views update incrementally.** A node's UI is rebuilt only when its structure
  changes (parameters, connections, label). Value changes update widgets in place.
- **Drags are robust to rebuilds.** Node and wire drags are tracked from the pointer
  position, so they survive the node being rebuilt mid-drag.

## Known limitations

- Text is rasterized at its unzoomed size, so it gets slightly soft when zoomed in
  past 100%. Below 100% it stays sharp.
- Wire and node dragging is implemented for the mouse pointer. Touch input is not
  handled yet.
- No undo/redo, copy/paste or node groups yet.

## License

MIT
