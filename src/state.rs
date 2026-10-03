//! Editor state that lives alongside the graph: node positions, selection,
//! draw order and the camera. This is what you persist to save a document.

use std::fmt;

use bevy::prelude::*;
use slotmap::SecondaryMap;

use crate::graph::{AnyParameterId, InputId, InputParamKind, Node, NodeId, OutputId};
use crate::traits::{DataTypeTrait, GraphOf, NodeDataTrait, NodeGraphSchema, NodeTemplateTrait};

/// Pan and zoom of the canvas. `pan` is the screen position (in logical
/// pixels, relative to the canvas' top-left corner) of the graph origin.
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PanZoom {
    pub pan: Vec2,
    pub zoom: f32,
}

impl Default for PanZoom {
    fn default() -> Self {
        Self {
            pan: Vec2::new(40.0, 40.0),
            zoom: 1.0,
        }
    }
}

impl PanZoom {
    pub fn world_to_screen(&self, world: Vec2) -> Vec2 {
        self.pan + world * self.zoom
    }

    pub fn screen_to_world(&self, screen: Vec2) -> Vec2 {
        (screen - self.pan) / self.zoom.max(f32::EPSILON)
    }

    /// Multiplies the zoom by `factor` (clamped to `min..=max`), keeping the
    /// graph point under `screen_anchor` fixed on screen.
    pub fn zoom_around(&mut self, screen_anchor: Vec2, factor: f32, min: f32, max: f32) {
        let anchored_world = self.screen_to_world(screen_anchor);
        self.zoom = (self.zoom * factor).clamp(min, max);
        self.pan = screen_anchor - anchored_world * self.zoom;
    }
}

/// Why a connection was refused by [`GraphEditorState::try_connect`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConnectError {
    /// One of the parameters does not exist.
    InvalidParameter,
    /// Both parameters belong to the same node.
    SameNode,
    /// The input is [`InputParamKind::ConstantOnly`].
    ConstantOnly,
    /// The output type is not compatible with the input type.
    IncompatibleTypes,
    /// The connection already exists.
    AlreadyConnected,
    /// The input already holds its maximum number of connections.
    InputFull,
}

impl fmt::Display for ConnectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidParameter => "parameter does not exist",
            Self::SameNode => "cannot connect a node to itself",
            Self::ConstantOnly => "input does not accept connections",
            Self::IncompatibleTypes => "incompatible data types",
            Self::AlreadyConnected => "already connected",
            Self::InputFull => "input accepts no more connections",
        })
    }
}

impl std::error::Error for ConnectError {}

/// A removed node and the `(input, output)` connections that were severed.
pub type RemovedNode<N> = (Node<N>, Vec<(InputId, OutputId)>);

/// The graph plus everything the editor needs to display it.
#[derive(Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "serde",
    serde(bound(
        serialize = "N: serde::Serialize, N::DataType: serde::Serialize, N::ValueType: serde::Serialize",
        deserialize = "N: serde::Deserialize<'de>, N::DataType: serde::Deserialize<'de>, N::ValueType: serde::Deserialize<'de>"
    ))
)]
pub struct GraphEditorState<N: NodeDataTrait> {
    pub graph: GraphOf<N>,
    /// Top-left corner of every node, in graph units.
    pub node_positions: SecondaryMap<NodeId, Vec2>,
    /// Draw order, back to front.
    pub node_order: Vec<NodeId>,
    pub selected_nodes: Vec<NodeId>,
    pub pan_zoom: PanZoom,
}

impl<N: NodeDataTrait> Default for GraphEditorState<N> {
    fn default() -> Self {
        Self {
            graph: GraphOf::<N>::default(),
            node_positions: SecondaryMap::default(),
            node_order: Vec::new(),
            selected_nodes: Vec::new(),
            pan_zoom: PanZoom::default(),
        }
    }
}

impl<N: NodeDataTrait> GraphEditorState<N> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a node from `template` with its top-left corner at `position`.
    pub fn add_node<T>(&mut self, template: &T, position: Vec2) -> NodeId
    where
        T: NodeTemplateTrait<NodeData = N>,
    {
        let node_id = self.graph.add_node(
            template.node_graph_label(),
            template.user_data(),
            |graph, node_id| template.build_node(graph, node_id),
        );
        self.place_node(node_id, position);
        node_id
    }

    /// Creates a node without a template.
    pub fn add_node_with(
        &mut self,
        label: impl Into<String>,
        user_data: N,
        position: Vec2,
        build: impl FnOnce(&mut GraphOf<N>, NodeId),
    ) -> NodeId {
        let node_id = self.graph.add_node(label, user_data, build);
        self.place_node(node_id, position);
        node_id
    }

    fn place_node(&mut self, node_id: NodeId, position: Vec2) {
        self.node_positions.insert(node_id, position);
        self.node_order.push(node_id);
    }

    /// Removes a node and all bookkeeping about it. Returns the node and the
    /// severed `(input, output)` connections.
    pub fn remove_node(&mut self, node_id: NodeId) -> Option<RemovedNode<N>> {
        if !self.graph.nodes.contains_key(node_id) {
            return None;
        }
        self.node_positions.remove(node_id);
        self.node_order.retain(|id| *id != node_id);
        self.selected_nodes.retain(|id| *id != node_id);
        Some(self.graph.remove_node(node_id))
    }

    pub fn node_position(&self, node_id: NodeId) -> Option<Vec2> {
        self.node_positions.get(node_id).copied()
    }

    /// Moves a node to the top of the draw order.
    pub fn raise_node(&mut self, node_id: NodeId) {
        if self.node_order.last() == Some(&node_id) {
            return;
        }
        self.node_order.retain(|id| *id != node_id);
        self.node_order.push(node_id);
    }

    pub fn is_selected(&self, node_id: NodeId) -> bool {
        self.selected_nodes.contains(&node_id)
    }

    pub fn select_only(&mut self, node_id: NodeId) {
        self.selected_nodes.clear();
        self.selected_nodes.push(node_id);
    }

    /// Adds a node to the selection, or removes it if already selected.
    pub fn toggle_selected(&mut self, node_id: NodeId) {
        if self.is_selected(node_id) {
            self.selected_nodes.retain(|id| *id != node_id);
        } else {
            self.selected_nodes.push(node_id);
        }
    }

    pub fn clear_selection(&mut self) {
        self.selected_nodes.clear();
    }

    /// Whether `output` could be connected to `input` right now.
    pub fn can_connect(&self, output: OutputId, input: InputId) -> bool {
        self.check_connection(output, input)
            .is_ok_or_replaces_single()
    }

    fn check_connection(&self, output: OutputId, input: InputId) -> ConnectionCheck {
        let (Some(out_param), Some(in_param)) = (
            self.graph.try_get_output(output),
            self.graph.try_get_input(input),
        ) else {
            return ConnectionCheck::Refused(ConnectError::InvalidParameter);
        };
        if out_param.node == in_param.node {
            return ConnectionCheck::Refused(ConnectError::SameNode);
        }
        if in_param.kind == InputParamKind::ConstantOnly {
            return ConnectionCheck::Refused(ConnectError::ConstantOnly);
        }
        if !out_param.typ.is_compatible_with(&in_param.typ) {
            return ConnectionCheck::Refused(ConnectError::IncompatibleTypes);
        }
        let existing = self.graph.connections(input);
        if existing.contains(&output) {
            return ConnectionCheck::Refused(ConnectError::AlreadyConnected);
        }
        match in_param.max_connections {
            Some(max) if existing.len() >= max.get() as usize => {
                if max.get() == 1 {
                    ConnectionCheck::ReplacesSingle
                } else {
                    ConnectionCheck::Refused(ConnectError::InputFull)
                }
            }
            _ => ConnectionCheck::Ok,
        }
    }

    /// Connects `output` to `input` if the types and limits allow it.
    ///
    /// Connecting to an occupied single-connection input replaces the old
    /// wire; the displaced outputs are returned.
    pub fn try_connect(
        &mut self,
        output: OutputId,
        input: InputId,
    ) -> Result<Vec<OutputId>, ConnectError> {
        let displaced = match self.check_connection(output, input) {
            ConnectionCheck::Refused(error) => return Err(error),
            ConnectionCheck::ReplacesSingle => self.graph.remove_connections_to(input),
            ConnectionCheck::Ok => Vec::new(),
        };
        self.graph.add_connection(output, input);
        Ok(displaced)
    }

    /// Ensures every node has a position and a draw-order slot, and drops
    /// bookkeeping for nodes that no longer exist. Called by the editor every
    /// frame, so nodes added straight to `graph` show up at the origin.
    pub fn sanitize(&mut self) {
        let graph = &self.graph;
        self.node_positions
            .retain(|id, _| graph.nodes.contains_key(id));
        self.node_order.retain(|id| graph.nodes.contains_key(*id));
        self.selected_nodes
            .retain(|id| graph.nodes.contains_key(*id));
        for node_id in self.graph.nodes.keys() {
            if !self.node_positions.contains_key(node_id) {
                self.node_positions.insert(node_id, Vec2::ZERO);
            }
            if !self.node_order.contains(&node_id) {
                self.node_order.push(node_id);
            }
        }
    }
}

enum ConnectionCheck {
    Ok,
    ReplacesSingle,
    Refused(ConnectError),
}

impl ConnectionCheck {
    fn is_ok_or_replaces_single(&self) -> bool {
        !matches!(self, Self::Refused(_))
    }
}

/// Something the user did in the editor. Read them with a
/// `MessageReader<NodeGraphResponse<YourSchema>>`; they are the Bevy
/// counterpart of egui_node_graph2's `GraphResponse::node_responses`.
///
/// All of these have already been applied to the graph when you receive them.
#[derive(Clone, Debug)]
pub enum NodeResponse<N> {
    /// The user started dragging a wire from a port.
    ConnectEventStarted(NodeId, AnyParameterId),
    /// A wire was connected.
    ConnectEventEnded { output: OutputId, input: InputId },
    /// A wire was removed (by dragging it off an input or by replacing it).
    DisconnectEvent { output: OutputId, input: InputId },
    /// A node was created from the node finder.
    CreatedNode(NodeId),
    /// A node was selected.
    SelectNode(NodeId),
    /// A node was brought to the front.
    RaiseNode(NodeId),
    /// A node was dragged.
    MoveNode { node: NodeId, drag_delta: Vec2 },
    /// A node was deleted. Its connections were removed first and reported
    /// as [`DisconnectEvent`](Self::DisconnectEvent)s.
    DeleteNodeFull { node_id: NodeId, node: Node<N> },
    /// An inline value widget changed an input's constant value.
    ValueChanged { node_id: NodeId, input_id: InputId },
}

/// Message carrying a [`NodeResponse`] from the editor entity `editor`.
#[derive(Message)]
pub struct NodeGraphResponse<S: NodeGraphSchema> {
    pub editor: Entity,
    pub response: NodeResponse<S::NodeData>,
}

impl<S: NodeGraphSchema> Clone for NodeGraphResponse<S> {
    fn clone(&self) -> Self {
        Self {
            editor: self.editor,
            response: self.response.clone(),
        }
    }
}

impl<S: NodeGraphSchema> fmt::Debug for NodeGraphResponse<S>
where
    S::NodeData: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NodeGraphResponse")
            .field("editor", &self.editor)
            .field("response", &self.response)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use super::*;
    use crate::graph::InputParamKind;
    use crate::traits::{ValueEdit, ValueWidget, WidgetValueTrait};

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    enum Ty {
        A,
        B,
    }

    impl crate::traits::DataTypeTrait for Ty {
        fn color(&self) -> Color {
            Color::WHITE
        }
        fn name(&self) -> Cow<'_, str> {
            "ty".into()
        }
    }

    #[derive(Clone, Debug, PartialEq)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    struct Val(f32);

    impl WidgetValueTrait for Val {
        fn value_widget(&self, _: &str) -> ValueWidget {
            ValueWidget::None
        }
        fn apply_edit(&mut self, _: ValueEdit) {}
    }

    #[derive(Clone, Debug, PartialEq)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    struct Data;

    impl NodeDataTrait for Data {
        type DataType = Ty;
        type ValueType = Val;
    }

    fn node(
        state: &mut GraphEditorState<Data>,
        input: Ty,
        wide: bool,
    ) -> (NodeId, InputId, OutputId) {
        let mut ids = None;
        let node = state.add_node_with("n", Data, Vec2::ZERO, |graph, node| {
            let max = if wide {
                None
            } else {
                std::num::NonZeroU32::new(1)
            };
            let input = graph.add_wide_input_param(
                node,
                "in",
                input,
                Val(0.0),
                InputParamKind::ConnectionOrConstant,
                max,
                true,
            );
            let output = graph.add_output_param(node, "out", Ty::A);
            ids = Some((input, output));
        });
        let (input, output) = ids.unwrap();
        (node, input, output)
    }

    #[test]
    fn connections_respect_types_and_limits() {
        let mut state = GraphEditorState::<Data>::new();
        let (_, _, out_a) = node(&mut state, Ty::A, false);
        let (_, _, out_b) = node(&mut state, Ty::A, false);
        let (_, single, self_out) = node(&mut state, Ty::A, false);
        let (_, typed_b, _) = node(&mut state, Ty::B, false);
        let (_, wide, _) = node(&mut state, Ty::A, true);

        assert_eq!(
            state.try_connect(self_out, single),
            Err(ConnectError::SameNode)
        );
        assert_eq!(
            state.try_connect(out_a, typed_b),
            Err(ConnectError::IncompatibleTypes)
        );
        assert_eq!(state.try_connect(out_a, single), Ok(vec![]));
        assert_eq!(
            state.try_connect(out_a, single),
            Err(ConnectError::AlreadyConnected)
        );
        // A single-connection input swaps its wire.
        assert_eq!(state.try_connect(out_b, single), Ok(vec![out_a]));
        assert_eq!(state.graph.connections(single), &[out_b]);
        // A wide input keeps both.
        assert_eq!(state.try_connect(out_a, wide), Ok(vec![]));
        assert_eq!(state.try_connect(out_b, wide), Ok(vec![]));
        assert_eq!(state.graph.connections(wide).len(), 2);
    }

    #[test]
    fn removing_a_node_cleans_up_editor_state() {
        let mut state = GraphEditorState::<Data>::new();
        let (a, _, out) = node(&mut state, Ty::A, false);
        let (b, input, _) = node(&mut state, Ty::A, false);
        state.try_connect(out, input).unwrap();
        state.select_only(a);
        let (_, severed) = state.remove_node(a).unwrap();
        assert_eq!(severed, vec![(input, out)]);
        assert!(state.selected_nodes.is_empty());
        assert_eq!(state.node_order, vec![b]);
        assert!(state.node_position(a).is_none());
    }

    #[test]
    fn sanitize_places_nodes_added_directly_to_the_graph() {
        let mut state = GraphEditorState::<Data>::new();
        let node = state.graph.add_node("raw", Data, |_, _| {});
        state.sanitize();
        assert_eq!(state.node_position(node), Some(Vec2::ZERO));
        assert_eq!(state.node_order, vec![node]);
    }

    #[test]
    fn zoom_keeps_the_anchor_fixed() {
        let mut pan_zoom = PanZoom::default();
        let anchor = Vec2::new(300.0, 200.0);
        let before = pan_zoom.screen_to_world(anchor);
        pan_zoom.zoom_around(anchor, 1.7, 0.1, 4.0);
        assert!((pan_zoom.screen_to_world(anchor) - before).length() < 1e-3);
        assert_eq!(pan_zoom.zoom, 1.7);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn editor_state_round_trips_through_serde() {
        let mut state = GraphEditorState::<Data>::new();
        let (a, _, out) = node(&mut state, Ty::A, false);
        let (_, input, _) = node(&mut state, Ty::A, false);
        state.try_connect(out, input).unwrap();
        state.node_positions[a] = Vec2::new(12.0, 34.0);
        state.pan_zoom.zoom = 0.5;

        let json = serde_json::to_string(&state).unwrap();
        let restored: GraphEditorState<Data> = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.graph.connections(input), &[out]);
        assert_eq!(restored.node_position(a), Some(Vec2::new(12.0, 34.0)));
        assert_eq!(restored.pan_zoom, state.pan_zoom);
        assert_eq!(restored.node_order, state.node_order);
    }
}
