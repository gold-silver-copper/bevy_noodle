//! The Bevy UI editor: the [`NodeGraphEditor`] component and [`NodeGraphPlugin`].

mod finder;
mod input;
mod view;
mod widgets;
mod wires;

use std::collections::{HashMap, HashSet};
use std::marker::PhantomData;

use bevy::prelude::*;
use bevy::ui::UiSystems;

use crate::graph::{AnyParameterId, InputId, NodeId, OutputId};
use crate::render::{NodeGraphRenderPlugin, WireMaterial};
use crate::state::{GraphEditorState, NodeGraphResponse};
use crate::style::{NodeGraphSettings, NodeGraphStyle};
use crate::traits::{NodeGraphSchema, SchemaGraph};

/// System sets of the editor, in execution order.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum NodeGraphSystems {
    /// `Update`: keyboard, pinch, text widgets and node finder input.
    Input,
    /// `PostUpdate`, before UI layout: graph → UI entities.
    Sync,
    /// `PostUpdate`, after UI layout: reads back port positions for the wires.
    Measure,
}

/// Adds the editor for one [`NodeGraphSchema`]. Add it once per schema; the
/// shared rendering bits are only registered once.
pub struct NodeGraphPlugin<S>(PhantomData<fn() -> S>);

impl<S> Default for NodeGraphPlugin<S> {
    fn default() -> Self {
        Self(PhantomData)
    }
}

impl<S: NodeGraphSchema> Plugin for NodeGraphPlugin<S> {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<NodeGraphCorePlugin>() {
            app.add_plugins(NodeGraphCorePlugin);
        }

        app.add_message::<NodeGraphResponse<S>>()
            .add_systems(
                Update,
                (
                    input::track_pointer::<S>,
                    input::handle_keyboard::<S>,
                    input::handle_pinch::<S>,
                    widgets::commit_focused_field::<S>,
                    finder::handle_finder_input::<S>,
                )
                    .chain()
                    .in_set(NodeGraphSystems::Input),
            )
            .add_systems(
                PostUpdate,
                (
                    view::setup_editors::<S>,
                    view::sync_nodes::<S>,
                    widgets::sync_widget_values::<S>,
                    view::sync_canvas::<S>,
                    wires::sync_wires::<S>,
                    wires::sync_ports::<S>,
                    finder::sync_finder::<S>,
                )
                    .chain()
                    .in_set(NodeGraphSystems::Sync)
                    .before(UiSystems::Prepare),
            )
            .add_systems(
                PostUpdate,
                view::measure_layout::<S>
                    .in_set(NodeGraphSystems::Measure)
                    .in_set(UiSystems::PostLayout),
            );
    }
}

/// Schema-independent parts, added automatically by [`NodeGraphPlugin`].
struct NodeGraphCorePlugin;

impl Plugin for NodeGraphCorePlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(NodeGraphRenderPlugin)
            .add_systems(Update, widgets::update_hover_highlights);
    }
}

/// A node graph editor. Spawn it as a UI node, give it a size, and it fills
/// itself with the canvas:
///
/// ```ignore
/// commands.spawn((
///     NodeGraphEditor::<MyGraph>::new(MyTemplate::all()),
///     Node { width: percent(100), height: percent(100), ..default() },
/// ));
/// ```
///
/// The graph, positions, selection and camera live in [`state`](Self::state)
/// and can be read or modified from any system. Changes show up in the UI the
/// same frame.
#[derive(Component)]
#[require(Node, NodeGraphStyle)]
pub struct NodeGraphEditor<S: NodeGraphSchema> {
    pub state: GraphEditorState<S::NodeData>,
    /// The node kinds offered by the node finder.
    pub templates: Vec<S::NodeTemplate>,
    pub settings: NodeGraphSettings,
    pub(crate) ui: EditorUi,
}

impl<S: NodeGraphSchema> NodeGraphEditor<S> {
    /// An empty editor offering `templates` in its node finder.
    pub fn new(templates: impl IntoIterator<Item = S::NodeTemplate>) -> Self {
        Self {
            state: GraphEditorState::default(),
            templates: templates.into_iter().collect(),
            settings: NodeGraphSettings::default(),
            ui: EditorUi::default(),
        }
    }

    /// Starts from an existing (e.g. deserialized) state.
    pub fn with_state(mut self, state: GraphEditorState<S::NodeData>) -> Self {
        self.state = state;
        self.ui.rebuild_all = true;
        self
    }

    pub fn with_settings(mut self, settings: NodeGraphSettings) -> Self {
        self.settings = settings;
        self
    }

    pub fn graph(&self) -> &SchemaGraph<S> {
        &self.state.graph
    }

    pub fn graph_mut(&mut self) -> &mut SchemaGraph<S> {
        &mut self.state.graph
    }

    /// Rebuilds a node's UI next frame. Needed only after changing something
    /// the editor cannot see, e.g. state read by a custom
    /// [`spawn_body`](crate::NodeDataTrait::spawn_body) without bumping
    /// [`body_revision`](crate::NodeDataTrait::body_revision).
    pub fn refresh_node(&mut self, node_id: NodeId) {
        self.ui.dirty.insert(node_id);
    }

    /// Rebuilds every node's UI next frame.
    pub fn refresh_all(&mut self) {
        self.ui.rebuild_all = true;
    }

    /// Whether the user is currently dragging a wire.
    pub fn is_connecting(&self) -> bool {
        self.ui.connection.is_some()
    }

    /// Whether the node finder popup is open.
    pub fn is_finder_open(&self) -> bool {
        self.ui.finder.is_some()
    }

    /// Opens the node finder at `screen` (logical pixels relative to the
    /// canvas' top-left corner). New nodes are placed there.
    pub fn open_finder(&mut self, screen: Vec2) {
        let world = self.state.pan_zoom.screen_to_world(screen);
        self.ui.open_finder(screen, world, None);
    }

    pub fn close_finder(&mut self) {
        self.ui.finder = None;
    }

    /// Pointer position relative to the canvas' top-left corner, if the
    /// pointer is over the canvas.
    pub fn pointer_position(&self) -> Option<Vec2> {
        self.ui.pointer
    }

    /// Pointer position in graph coordinates, if the pointer is over the canvas.
    pub fn pointer_world_position(&self) -> Option<Vec2> {
        self.ui
            .pointer
            .map(|screen| self.state.pan_zoom.screen_to_world(screen))
    }

    /// Size of the canvas in logical pixels (zero until the first layout).
    pub fn canvas_size(&self) -> Vec2 {
        self.ui.canvas_size
    }

    /// Where a port is drawn, relative to the canvas' top-left corner.
    /// `None` until the node has been laid out.
    pub fn port_screen_position(&self, param: impl Into<AnyParameterId>) -> Option<Vec2> {
        input::port_world_position(self, param.into())
            .map(|world| self.state.pan_zoom.world_to_screen(world))
    }

    /// Where a node is drawn, relative to the canvas' top-left corner.
    /// `None` until the node has been laid out.
    pub fn node_screen_rect(&self, node_id: NodeId) -> Option<Rect> {
        let size = self.ui.nodes.get(&node_id)?.size;
        if size == Vec2::ZERO {
            return None;
        }
        let top_left = self.state.node_position(node_id)?;
        let pan_zoom = &self.state.pan_zoom;
        Some(Rect::from_corners(
            pan_zoom.world_to_screen(top_left),
            pan_zoom.world_to_screen(top_left + size),
        ))
    }

    /// Centers the view on the bounding box of all nodes.
    pub fn frame_all(&mut self) {
        let mut bounds: Option<Rect> = None;
        for (node_id, position) in &self.state.node_positions {
            let size = self
                .ui
                .nodes
                .get(&node_id)
                .map(|view| view.size)
                .unwrap_or(Vec2::new(180.0, 80.0));
            let rect = Rect::from_corners(*position, *position + size);
            bounds = Some(bounds.map_or(rect, |b| b.union(rect)));
        }
        let (Some(bounds), canvas) = (bounds, self.ui.canvas_size) else {
            return;
        };
        if canvas.min_element() <= 0.0 {
            return;
        }
        let padding = 60.0;
        let fit = (canvas - Vec2::splat(padding * 2.0)) / bounds.size().max(Vec2::ONE);
        let zoom = fit
            .min_element()
            .clamp(self.settings.zoom_min, self.settings.zoom_max.min(1.0));
        self.state.pan_zoom.zoom = zoom;
        self.state.pan_zoom.pan = canvas * 0.5 - bounds.center() * zoom;
    }
}

/// Entities making up an editor's canvas.
#[derive(Clone, Copy, Debug)]
pub(crate) struct CanvasParts {
    pub grid: Entity,
    pub world: Entity,
    pub wire_layer: Entity,
    pub node_layer: Entity,
    pub preview_wire: Entity,
    pub selection_rect: Entity,
}

/// The UI entities of one node.
pub(crate) struct NodeViewRecord {
    pub root: Entity,
    /// Container of the custom body, rebuilt on its own.
    pub body: Entity,
    pub key: u64,
    pub body_key: u64,
    /// Laid-out size in graph units.
    pub size: Vec2,
    pub params: Vec<AnyParameterId>,
}

pub(crate) struct PortRecord {
    pub entity: Entity,
    pub node: NodeId,
    /// Port center relative to the node's top-left corner, once laid out.
    pub offset: Option<Vec2>,
}

pub(crate) struct WireRecord {
    pub entity: Entity,
    pub material: Handle<WireMaterial>,
    pub last: Option<WireMaterial>,
    pub last_rect: Rect,
}

pub(crate) struct ConnectionDrag {
    pub from: AnyParameterId,
    pub pointer_world: Vec2,
    /// The port the wire would connect to if released now.
    pub target: Option<AnyParameterId>,
}

pub(crate) struct NodeDrag {
    /// Canvas-local pointer position at the previous frame.
    pub last_pointer: Vec2,
}

pub(crate) struct PanDrag {
    pub button: PointerButton,
    /// Canvas-local pointer position at the previous drag event.
    pub last_pointer: Vec2,
}

pub(crate) struct BoxSelection {
    /// Canvas-local positions.
    pub start: Vec2,
    pub end: Vec2,
    pub initial: Vec<NodeId>,
}

pub(crate) struct FinderState {
    /// Canvas-local position of the popup.
    pub screen: Vec2,
    /// Where created nodes go.
    pub world: Vec2,
    /// A wire waiting for the created node.
    pub pending: Option<AnyParameterId>,
    pub search: String,
}

pub(crate) struct FinderView {
    pub root: Entity,
    pub search: Entity,
    pub list: Entity,
    pub built_search: Option<String>,
}

/// Interaction and view bookkeeping. Not part of the persisted state.
#[derive(Default)]
pub(crate) struct EditorUi {
    pub parts: Option<CanvasParts>,
    pub nodes: HashMap<NodeId, NodeViewRecord>,
    pub ports: HashMap<AnyParameterId, PortRecord>,
    pub wires: HashMap<(InputId, OutputId), WireRecord>,
    pub dirty: HashSet<NodeId>,
    pub rebuild_all: bool,
    pub connection: Option<ConnectionDrag>,
    pub node_drag: Option<NodeDrag>,
    pub box_selection: Option<BoxSelection>,
    pub panning: Option<PanDrag>,
    /// Window position of the last press on the empty canvas, to tell clicks from drags.
    pub press_position: Option<Vec2>,
    pub finder: Option<FinderState>,
    pub finder_view: Option<FinderView>,
    /// Respawn the finder popup (it was reopened somewhere else).
    pub finder_view_reset: bool,
    /// Canvas-local pointer position while the pointer is over the canvas.
    pub pointer: Option<Vec2>,
    pub canvas_size: Vec2,
    pub preview_material: Option<Handle<WireMaterial>>,
    /// Shared by every node's delete button.
    pub cross_material: Handle<WireMaterial>,
}

impl EditorUi {
    pub fn open_finder(&mut self, screen: Vec2, world: Vec2, pending: Option<AnyParameterId>) {
        self.finder = Some(FinderState {
            screen,
            world,
            pending,
            search: String::new(),
        });
        // Rebuild the popup so it moves and refocuses.
        self.finder_view_reset = true;
    }
}
