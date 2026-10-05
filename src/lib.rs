//! A minimal, headless node graph library for Bevy UI: you build and style the
//! nodes and edges, it handles the graph.
//!
//! ```text
//! NodeCanvas              your UI node: one graph and its viewport (no background)
//! └── CanvasContent       spawned by the canvas; pans and zooms, holds the nodes
//!     ├── GraphNode       your UI node, styled however you like
//!     │   └── … Port      your UI node marking a connection point
//!     └── Edge            spawned on connect; EdgeSource/EdgeTarget relate it to ports
//! ```
//!
//! Spawn nodes as children of a canvas; they move into its content. Any
//! number of canvases can coexist or nest; every lookup resolves to the
//! nearest one. Change graphs with [`GraphCommandsExt::graph_edit`]; react to
//! [`ConnectionCheck`] (connection rules), [`EditRequested`] (to veto or
//! redirect an edit) and [`EditApplied`]. Interaction is opt-in per
//! canvas with [`CanvasInteraction`]. The core draws nothing: draw edges from
//! [`EdgeGeometry`], or enable the `default_style` feature. The `scene` feature
//! adds graph snapshots (undo, save and load).

// Bevy's `AsBindGroup` derive trips a recursion lint on recent compilers.
#![recursion_limit = "256"]
#![allow(clippy::type_complexity)]
#![warn(missing_docs)]

pub mod components;
pub mod edit;
mod geometry;
pub mod interaction;
pub mod keyboard;
pub mod query;
#[cfg(feature = "scene")]
pub mod scene;
#[cfg(feature = "default_style")]
pub mod style;

use bevy::app::PluginGroupBuilder;
use bevy::prelude::*;
use bevy::ui::UiSystems;

pub use components::*;
pub use edit::*;
pub use interaction::*;
pub use keyboard::*;
pub use query::{Connection, GraphQuery};

/// System sets, all in `PostUpdate`.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum NoodleSystems {
    /// Before UI layout: node positions, canvas view, edge geometry.
    Sync,
    /// After `Sync`, before layout: drawing (put your edge renderers here).
    /// Ports are measured after layout, in Bevy's `UiSystems::PostLayout`.
    Render,
}

/// [`NoodleCorePlugin`] + [`NoodleInteractionPlugin`].
pub struct NoodlePlugins;

impl PluginGroup for NoodlePlugins {
    fn build(self) -> PluginGroupBuilder {
        PluginGroupBuilder::start::<Self>()
            .add(NoodleCorePlugin)
            .add(NoodleInteractionPlugin)
    }
}

/// Graph components, the edit pipeline and geometry.
pub struct NoodleCorePlugin;

impl Plugin for NoodleCorePlugin {
    fn build(&self, app: &mut App) {
        use geometry::*;
        app.add_message::<EditApplied>()
            .add_observer(components::adopt_nodes)
            .configure_sets(
                PostUpdate,
                (NoodleSystems::Sync, NoodleSystems::Render)
                    .chain()
                    .before(UiSystems::Prepare),
            )
            .add_systems(
                PostUpdate,
                (follow_reparented, sync_layout, update_edge_geometry)
                    .chain()
                    .in_set(NoodleSystems::Sync),
            )
            .add_systems(PostUpdate, measure_ports.in_set(UiSystems::PostLayout));
    }
}

/// Everything most apps need: `use bevy_noodle::prelude::*;`.
pub mod prelude {
    #[cfg(feature = "default_style")]
    pub use crate::style::{CanvasGrid, EdgeStyle, NoodleDefaultStylePlugin, PortColor};
    pub use crate::{
        CanvasContent, CanvasInteraction, CanvasKeyboard, CanvasView, ConnectionCheck, Edge,
        EdgeGeometry, EdgeHitbox, EdgeSource, EdgeTarget, EditApplied, EditOrigin, EditRequested,
        GraphCommandsExt, GraphEdit, GraphNode, GraphQuery, GraphWorldExt, NodeCanvas,
        NodeDragHandle, NodePosition, NoodleCorePlugin, NoodleInteractionPlugin,
        NoodleKeyboardPlugin, NoodlePlugins, PendingWire, Port, PortDirection, PortPair, PortType,
        SelectMode, WireDropped,
    };
}
