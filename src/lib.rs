//! A minimal, headless node graph library for Bevy UI: you build and style the
//! nodes and edges, it handles the graph.
//!
//! ```text
//! NodeCanvas              your UI node: one graph and its viewport (no background)
//! └── CanvasContent       pans and zooms; holds the nodes
//!     ├── GraphNode       your UI node, styled however you like
//!     │   └── … Port      your UI node marking a connection point
//!     └── Edge            spawned on connect; EdgeSource/EdgeTarget relate it to ports
//! ```
//!
//! Any number of canvases can coexist or nest; every lookup resolves to the
//! nearest one. Change graphs with [`GraphCommandsExt::graph_edit`]; react to
//! [`EditRequested`] (to veto) and [`EditApplied`]. Interaction is opt-in per
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
pub use geometry::FrameAll;
pub use interaction::*;
pub use query::GraphQuery;

/// System sets, all in `PostUpdate`.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum NoodleSystems {
    /// Before UI layout: node positions, canvas view, edge geometry.
    Sync,
    /// After `Sync`, before layout: drawing (put your edge renderers here).
    Render,
    /// After layout: ports are measured.
    Measure,
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
            .add_observer(frame_all)
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
                (follow_reparented, sync_layout, update_edge_geometry)
                    .chain()
                    .in_set(NoodleSystems::Sync),
            )
            .add_systems(PostUpdate, measure_ports.in_set(NoodleSystems::Measure));
    }
}

/// Everything most apps need: `use bevy_noodle::prelude::*;`.
pub mod prelude {
    #[cfg(feature = "default_style")]
    pub use crate::style::{CanvasGrid, EdgeLayer, EdgeStyle, NoodleDefaultStylePlugin, PortColor};
    pub use crate::{
        CanvasContent, CanvasInteraction, CanvasView, Edge, EdgeGeometry, EdgeHitbox, EdgeSource,
        EdgeTarget, EditApplied, EditOrigin, EditRequested, FrameAll, GraphCommandsExt, GraphEdit,
        GraphNode, GraphQuery, GraphWorldExt, NodeCanvas, NodeDragHandle, NodePosition,
        NoodleCorePlugin, NoodleInteractionPlugin, NoodlePlugins, PendingWire, Port, PortDirection,
        PortType, SelectMode, WireDropped,
    };
}
