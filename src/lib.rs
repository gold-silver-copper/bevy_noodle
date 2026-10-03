//! A headless, entity-based node graph library for Bevy UI.
//!
//! You build and style the nodes with ordinary Bevy UI; `bevy_noodle`
//! handles the graph: ports, connections, dragging, selection, panning and
//! zooming. The core draws nothing (no background, no wires) and binds no
//! keys. An optional look lives behind the `default_style` feature.
//!
//! # The graph is entities
//!
//! ```text
//! NodeCanvas                 your UI node: the viewport
//! └── CanvasContent          pans and zooms; holds the nodes
//!     ├── GraphNode          your UI node: style it however you like
//!     │   └── … Port         your UI node marking a connection point
//!     └── GraphNode …
//! Edge                       spawned on connect; draw it from EdgeGeometry
//! ```
//!
//! ```ignore
//! use bevy::prelude::*;
//! use bevy_noodle::prelude::*;
//!
//! const NUMBER: PortType = PortType::named("number");
//!
//! fn setup(mut commands: Commands) {
//!     commands.spawn(Camera2d);
//!     let canvas = commands.spawn((NodeCanvas, Node { width: percent(100), height: percent(100), ..default() })).id();
//!     let content = commands.spawn((CanvasContent, ChildOf(canvas))).id();
//!     commands.spawn((
//!         GraphNode,
//!         NodePosition(Vec2::new(40.0, 40.0)),
//!         ChildOf(content),
//!         Node { padding: UiRect::all(px(8)), ..default() },
//!         BackgroundColor(Color::srgb(0.2, 0.2, 0.25)),
//!         children![
//!             Text::new("Number"),
//!             (Port::output(NUMBER), Node { width: px(10), height: px(10), ..default() }, BackgroundColor(Color::WHITE)),
//!         ],
//!     ));
//! }
//! ```
//!
//! # Changing the graph
//!
//! Every change, from interaction or code, goes through
//! [`GraphCommandsExt::graph_edit`]: built-in validation, then an
//! [`EditRequested`] event your observers may veto or modify, then the
//! change, then [`EditApplied`] (an entity event on the canvas and a message).
//!
//! # Plugins
//!
//! * [`NoodlePlugins`]: [`NoodleCorePlugin`] + [`NoodleInteractionPlugin`].
//! * [`NoodleKeyBindingsPlugin`]: opt-in key bindings via [`CanvasKeymap`].
//! * `NoodleDefaultStylePlugin` (feature `default_style`): wires, grid,
//!   selection box, port highlighting, node-building helpers, node finder.

// Bevy's `AsBindGroup` derive trips a recursion lint on recent compilers.
#![recursion_limit = "256"]
// Bevy systems routinely take many parameters and complex query types.
#![allow(clippy::too_many_arguments, clippy::type_complexity)]

pub mod actions;
pub mod components;
pub mod edit;
mod geometry;
pub mod interaction;
pub mod keymap;
pub mod query;
#[cfg(feature = "default_style")]
pub mod style;

use bevy::app::PluginGroupBuilder;
use bevy::prelude::*;
use bevy::ui::UiSystems;

pub use actions::{ClearSelection, DeleteSelection, FrameAll, PanBy, SelectAll, ZoomBy};
pub use components::*;
pub use edit::{
    EditApplied, EditOrigin, EditRejected, EditRequested, GraphCommandsExt, GraphEdit,
    GraphWorldExt, RejectReason, SelectMode,
};
pub use interaction::{
    CancelInteraction, CanvasInteraction, CanvasWantsInput, NoodleInteractionPlugin, PendingWire,
    ScrollMode, SelectionBox, WireCandidate, WireDropped, WireSource, WireTarget,
    canvas_wants_pointer_input,
};
pub use keymap::{CanvasAction, CanvasKeymap, KeyBinding, NoodleKeyBindingsPlugin};
pub use query::GraphQuery;

/// System sets, all in `PostUpdate`.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum NoodleSystems {
    /// Before UI layout: node positions, canvas view, edge geometry, drags.
    Sync,
    /// After [`Sync`](Self::Sync), before UI layout: drawing (the default
    /// style runs here; put your own edge renderers here too).
    Render,
    /// After UI layout: port positions are measured.
    Measure,
}

/// The core and pointer interaction. Add [`NoodleKeyBindingsPlugin`] and
/// (with `default_style`) `NoodleDefaultStylePlugin` separately if wanted.
pub struct NoodlePlugins;

impl PluginGroup for NoodlePlugins {
    fn build(self) -> PluginGroupBuilder {
        PluginGroupBuilder::start::<Self>()
            .add(NoodleCorePlugin)
            .add(NoodleInteractionPlugin)
    }
}

/// Graph components, the edit pipeline, canvas actions and geometry.
pub struct NoodleCorePlugin;

impl Plugin for NoodleCorePlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<EditApplied>()
            .register_type::<NodeCanvas>()
            .register_type::<CanvasView>()
            .register_type::<CanvasContent>()
            .register_type::<GraphNode>()
            .register_type::<NodePosition>()
            .register_type::<NodeDragHandle>()
            .register_type::<Port>()
            .register_type::<PortTangent>()
            .register_type::<PortAnchor>()
            .register_type::<Edge>()
            .register_type::<EdgeSource>()
            .register_type::<EdgeTarget>()
            .register_type::<OutgoingEdges>()
            .register_type::<IncomingEdges>()
            .register_type::<EdgeGeometry>()
            .register_type::<CanvasInteraction>()
            .register_type::<CanvasWantsInput>()
            .register_type::<PendingWire>()
            .register_type::<SelectionBox>()
            .register_type::<WireSource>()
            .register_type::<WireCandidate>()
            .register_type::<WireTarget>()
            .register_type::<CanvasKeymap>()
            .configure_sets(
                PostUpdate,
                (NoodleSystems::Sync, NoodleSystems::Render)
                    .chain()
                    .before(UiSystems::Prepare),
            )
            .configure_sets(
                PostUpdate,
                NoodleSystems::Measure.in_set(UiSystems::PostLayout),
            )
            .add_systems(
                PostUpdate,
                (
                    geometry::sync_node_positions,
                    geometry::sync_canvas_views,
                    geometry::update_edge_geometry,
                )
                    .chain()
                    .in_set(NoodleSystems::Sync),
            )
            .add_systems(
                PostUpdate,
                geometry::measure_ports.in_set(NoodleSystems::Measure),
            );
        actions::plugin(app);
    }
}

/// Everything needed to build and react to graphs.
pub mod prelude {
    #[cfg(feature = "default_style")]
    pub use crate::style::{CanvasGrid, EdgeLayer, EdgeStyle, NoodleDefaultStylePlugin, PortColor};
    pub use crate::{
        CancelInteraction, CanvasContent, CanvasInteraction, CanvasKeymap, CanvasView,
        ClearSelection, DeleteSelection, Edge, EdgeGeometry, EdgeSource, EdgeTarget, EditApplied,
        EditOrigin, EditRequested, FrameAll, GraphCommandsExt, GraphEdit, GraphNode, GraphQuery,
        GraphWorldExt, NodeCanvas, NodeDragHandle, NodePosition, NoodleCorePlugin,
        NoodleInteractionPlugin, NoodleKeyBindingsPlugin, NoodlePlugins, PendingWire, Port,
        PortDirection, PortType, SelectAll, SelectMode, WireDropped,
    };
}
