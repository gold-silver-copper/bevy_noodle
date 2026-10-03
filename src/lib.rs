//! A generic, typed node graph editor for Bevy UI, in the spirit of
//! [egui_node_graph2](https://github.com/trevyn/egui_node_graph2).
//!
//! The crate gives you the *editor*: nodes with typed input and output
//! ports, wires, inline value widgets, a searchable node finder, selection,
//! panning and zooming. What the graph *means*, and how it is evaluated or
//! compiled, is up to your application.
//!
//! # Overview
//!
//! 1. Describe your graph with a few traits:
//!    * [`DataTypeTrait`] – the types flowing over wires (and their colors).
//!    * [`WidgetValueTrait`] – the constant stored in each input, edited inline.
//!    * [`NodeDataTrait`] – per-node user data (optionally custom node UI).
//!    * [`NodeTemplateTrait`] – the node kinds users can create.
//!    * [`NodeGraphSchema`] – a marker type tying them together.
//! 2. Add [`NodeGraphPlugin::<YourSchema>`](NodeGraphPlugin) to your app.
//! 3. Spawn a [`NodeGraphEditor`] as a UI node.
//! 4. Read [`NodeGraphResponse`] messages to react to edits, and read or
//!    modify [`NodeGraphEditor::state`] from any system.
//!
//! ```ignore
//! use bevy::prelude::*;
//! use bevy_noodle::prelude::*;
//!
//! App::new()
//!     .add_plugins((DefaultPlugins, NodeGraphPlugin::<MyGraph>::default()))
//!     .add_systems(Startup, |mut commands: Commands| {
//!         commands.spawn(Camera2d);
//!         commands.spawn((
//!             NodeGraphEditor::<MyGraph>::new(MyTemplate::ALL),
//!             Node { width: percent(100), height: percent(100), ..default() },
//!         ));
//!     })
//!     .run();
//! ```
//!
//! See `examples/math_graph.rs` for a complete, evaluated graph.
//!
//! # Controls
//!
//! | Action | Input |
//! |---|---|
//! | Add a node | Right-click the canvas, or drop a wire on empty canvas |
//! | Connect | Drag from a port to a compatible port |
//! | Disconnect | Drag a wire off its input |
//! | Select | Click a node; Shift/Ctrl/Cmd+click to toggle; drag on the canvas to box-select |
//! | Move | Drag a node (moves the whole selection) |
//! | Delete | The × in the title bar, or Delete/Backspace |
//! | Pan | Middle-drag, Space+drag, or two-finger scroll |
//! | Zoom | Mouse wheel, Ctrl/Cmd+scroll, or pinch |
//! | Frame all | Ctrl/Cmd+0 |
//!
//! Number fields can also be scrubbed by dragging their label sideways.

// Bevy's `AsBindGroup` derive trips a recursion lint on recent compilers.
#![recursion_limit = "256"]

mod editor;
pub mod graph;
mod render;
pub mod state;
pub mod style;
pub mod traits;

pub use editor::{NodeGraphEditor, NodeGraphPlugin, NodeGraphSystems};
pub use graph::{
    AnyParameterId, Graph, InputId, InputParam, InputParamKind, Node, NodeGraphError, NodeId,
    OutputId, OutputParam,
};
pub use state::{
    ConnectError, GraphEditorState, NodeGraphResponse, NodeResponse, PanZoom, RemovedNode,
};
pub use style::{NodeGraphSettings, NodeGraphStyle, ScrollBehavior};
pub use traits::{
    DataTypeOf, DataTypeTrait, GraphOf, NodeBodyContext, NodeDataTrait, NodeGraphSchema,
    NodeTemplateTrait, NumberField, SchemaGraph, ValueEdit, ValueTypeOf, ValueWidget,
    WidgetValueTrait,
};

/// Everything needed to define and use a node graph.
pub mod prelude {
    pub use crate::{
        AnyParameterId, DataTypeTrait, Graph, GraphEditorState, GraphOf, InputId, InputParamKind,
        NodeBodyContext, NodeDataTrait, NodeGraphEditor, NodeGraphPlugin, NodeGraphResponse,
        NodeGraphSchema, NodeGraphSettings, NodeGraphStyle, NodeId, NodeResponse,
        NodeTemplateTrait, NumberField, OutputId, ValueEdit, ValueWidget, WidgetValueTrait,
    };
}
