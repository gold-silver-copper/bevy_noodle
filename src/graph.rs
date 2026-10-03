//! The graph data model.
//!
//! This module is deliberately free of any UI code: a [`Graph`] can be built,
//! inspected, evaluated and (with the `serde` feature) persisted without the
//! editor ever running. The shape mirrors `egui_node_graph2` so code written
//! against that crate ports over with few changes.

use std::fmt;
use std::num::NonZeroU32;
use std::ops::{Index, IndexMut};

use slotmap::{SecondaryMap, SlotMap};

slotmap::new_key_type! {
    /// Identifies a node in a [`Graph`].
    pub struct NodeId;
    /// Identifies an input parameter (an input port) in a [`Graph`].
    pub struct InputId;
    /// Identifies an output parameter (an output port) in a [`Graph`].
    pub struct OutputId;
}

/// Either side of a connection.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum AnyParameterId {
    Input(InputId),
    Output(OutputId),
}

impl AnyParameterId {
    pub fn assume_input(self) -> InputId {
        match self {
            Self::Input(input) => input,
            Self::Output(output) => panic!("{output:?} is not an InputId"),
        }
    }

    pub fn assume_output(self) -> OutputId {
        match self {
            Self::Output(output) => output,
            Self::Input(input) => panic!("{input:?} is not an OutputId"),
        }
    }
}

impl From<InputId> for AnyParameterId {
    fn from(input: InputId) -> Self {
        Self::Input(input)
    }
}

impl From<OutputId> for AnyParameterId {
    fn from(output: OutputId) -> Self {
        Self::Output(output)
    }
}

/// How an input parameter receives its value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum InputParamKind {
    /// Only a wire can provide the value; no inline widget is shown.
    ConnectionOnly,
    /// Only the inline widget provides the value; no port is shown.
    ConstantOnly,
    /// A wire provides the value when connected, otherwise the inline widget does.
    #[default]
    ConnectionOrConstant,
}

/// An input parameter of a node.
#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct InputParam<DataType, ValueType> {
    pub id: InputId,
    /// The data type of this input. Only outputs whose type is compatible
    /// (see [`DataTypeTrait::is_compatible_with`](crate::DataTypeTrait::is_compatible_with))
    /// can connect to it.
    pub typ: DataType,
    /// The constant value, edited through the inline widget when unconnected.
    pub value: ValueType,
    pub kind: InputParamKind,
    /// The node this parameter belongs to.
    pub node: NodeId,
    /// Whether the inline value widget is shown for this parameter.
    pub shown_inline: bool,
    /// Maximum number of wires this input accepts. `None` means unlimited.
    pub max_connections: Option<NonZeroU32>,
}

impl<DataType, ValueType> InputParam<DataType, ValueType> {
    pub fn value(&self) -> &ValueType {
        &self.value
    }

    pub fn kind(&self) -> InputParamKind {
        self.kind
    }

    pub fn node(&self) -> NodeId {
        self.node
    }

    pub fn max_connections(&self) -> Option<NonZeroU32> {
        self.max_connections
    }
}

/// An output parameter of a node.
#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct OutputParam<DataType> {
    pub id: OutputId,
    pub node: NodeId,
    pub typ: DataType,
}

impl<DataType> OutputParam<DataType> {
    pub fn node(&self) -> NodeId {
        self.node
    }
}

/// A node: a label, an ordered list of named inputs and outputs, and
/// arbitrary user data.
#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Node<NodeData> {
    pub id: NodeId,
    pub label: String,
    pub inputs: Vec<(String, InputId)>,
    pub outputs: Vec<(String, OutputId)>,
    pub user_data: NodeData,
}

impl<NodeData> Node<NodeData> {
    pub fn input_ids(&self) -> impl Iterator<Item = InputId> + '_ {
        self.inputs.iter().map(|(_, id)| *id)
    }

    pub fn output_ids(&self) -> impl Iterator<Item = OutputId> + '_ {
        self.outputs.iter().map(|(_, id)| *id)
    }

    /// Looks up an input parameter by name.
    pub fn get_input(&self, name: &str) -> Result<InputId, NodeGraphError> {
        self.inputs
            .iter()
            .find(|(param_name, _)| param_name == name)
            .map(|(_, id)| *id)
            .ok_or_else(|| NodeGraphError::NoParameterNamed(self.id, name.into()))
    }

    /// Looks up an output parameter by name.
    pub fn get_output(&self, name: &str) -> Result<OutputId, NodeGraphError> {
        self.outputs
            .iter()
            .find(|(param_name, _)| param_name == name)
            .map(|(_, id)| *id)
            .ok_or_else(|| NodeGraphError::NoParameterNamed(self.id, name.into()))
    }

    pub fn input_name(&self, input: InputId) -> Option<&str> {
        self.inputs
            .iter()
            .find(|(_, id)| *id == input)
            .map(|(name, _)| name.as_str())
    }

    pub fn output_name(&self, output: OutputId) -> Option<&str> {
        self.outputs
            .iter()
            .find(|(_, id)| *id == output)
            .map(|(name, _)| name.as_str())
    }
}

/// Errors returned by graph lookups.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NodeGraphError {
    NoParameterNamed(NodeId, String),
    InvalidParameterId(AnyParameterId),
}

impl fmt::Display for NodeGraphError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoParameterNamed(node, name) => {
                write!(f, "node {node:?} has no parameter named {name:?}")
            }
            Self::InvalidParameterId(id) => write!(f, "parameter {id:?} does not exist"),
        }
    }
}

impl std::error::Error for NodeGraphError {}

/// A typed node graph.
///
/// * `NodeData` – your per-node payload (usually the template it was built from).
/// * `DataType` – the type of values flowing over wires; decides wire colors and
///   which ports may connect.
/// * `ValueType` – the constant stored in each input, edited inline when unconnected.
///
/// Connections always go from an output to an input. An input may hold several
/// connections when created with [`Graph::add_wide_input_param`].
#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Graph<NodeData, DataType, ValueType> {
    pub nodes: SlotMap<NodeId, Node<NodeData>>,
    pub inputs: SlotMap<InputId, InputParam<DataType, ValueType>>,
    pub outputs: SlotMap<OutputId, OutputParam<DataType>>,
    /// For every input, the outputs connected to it, in connection order.
    pub connections: SecondaryMap<InputId, Vec<OutputId>>,
}

impl<NodeData, DataType, ValueType> Default for Graph<NodeData, DataType, ValueType> {
    fn default() -> Self {
        Self {
            nodes: SlotMap::default(),
            inputs: SlotMap::default(),
            outputs: SlotMap::default(),
            connections: SecondaryMap::default(),
        }
    }
}

impl<NodeData, DataType, ValueType> Graph<NodeData, DataType, ValueType> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a node, then calls `build` so parameters can be added to it.
    pub fn add_node(
        &mut self,
        label: impl Into<String>,
        user_data: NodeData,
        build: impl FnOnce(&mut Self, NodeId),
    ) -> NodeId {
        let label = label.into();
        let node_id = self.nodes.insert_with_key(|id| Node {
            id,
            label,
            inputs: Vec::new(),
            outputs: Vec::new(),
            user_data,
        });
        build(self, node_id);
        node_id
    }

    /// Adds an input that accepts at most one connection.
    pub fn add_input_param(
        &mut self,
        node_id: NodeId,
        name: impl Into<String>,
        typ: DataType,
        value: ValueType,
        kind: InputParamKind,
        shown_inline: bool,
    ) -> InputId {
        self.add_wide_input_param(
            node_id,
            name,
            typ,
            value,
            kind,
            NonZeroU32::new(1),
            shown_inline,
        )
    }

    /// Adds an input that accepts up to `max_connections` connections
    /// (`None` for unlimited).
    #[allow(clippy::too_many_arguments)]
    pub fn add_wide_input_param(
        &mut self,
        node_id: NodeId,
        name: impl Into<String>,
        typ: DataType,
        value: ValueType,
        kind: InputParamKind,
        max_connections: Option<NonZeroU32>,
        shown_inline: bool,
    ) -> InputId {
        let input_id = self.inputs.insert_with_key(|id| InputParam {
            id,
            typ,
            value,
            kind,
            node: node_id,
            shown_inline,
            max_connections,
        });
        self.nodes[node_id].inputs.push((name.into(), input_id));
        input_id
    }

    pub fn add_output_param(
        &mut self,
        node_id: NodeId,
        name: impl Into<String>,
        typ: DataType,
    ) -> OutputId {
        let output_id = self.outputs.insert_with_key(|id| OutputParam {
            id,
            node: node_id,
            typ,
        });
        self.nodes[node_id].outputs.push((name.into(), output_id));
        output_id
    }

    /// Removes an input parameter and every connection into it.
    pub fn remove_input_param(&mut self, param: InputId) {
        let Some(input) = self.inputs.remove(param) else {
            return;
        };
        if let Some(node) = self.nodes.get_mut(input.node) {
            node.inputs.retain(|(_, id)| *id != param);
        }
        self.connections.remove(param);
    }

    /// Removes an output parameter and every connection out of it.
    pub fn remove_output_param(&mut self, param: OutputId) {
        let Some(output) = self.outputs.remove(param) else {
            return;
        };
        if let Some(node) = self.nodes.get_mut(output.node) {
            node.outputs.retain(|(_, id)| *id != param);
        }
        for (_, outputs) in self.connections.iter_mut() {
            outputs.retain(|id| *id != param);
        }
        self.connections.retain(|_, outputs| !outputs.is_empty());
    }

    /// Removes a node with all its parameters. Returns the node and the
    /// connections that were severed, as `(input, output)` pairs.
    pub fn remove_node(&mut self, node_id: NodeId) -> (Node<NodeData>, Vec<(InputId, OutputId)>) {
        let mut disconnected = Vec::new();
        let node = self.nodes.remove(node_id).expect("node should exist");

        for (_, input) in &node.inputs {
            if let Some(outputs) = self.connections.remove(*input) {
                disconnected.extend(outputs.into_iter().map(|output| (*input, output)));
            }
            self.inputs.remove(*input);
        }

        let removed_outputs: Vec<OutputId> = node.output_ids().collect();
        for (input, outputs) in self.connections.iter_mut() {
            outputs.retain(|output| {
                let removed = removed_outputs.contains(output);
                if removed {
                    disconnected.push((input, *output));
                }
                !removed
            });
        }
        self.connections.retain(|_, outputs| !outputs.is_empty());
        for output in removed_outputs {
            self.outputs.remove(output);
        }

        (node, disconnected)
    }

    /// Connects `output` to `input`. This is a raw operation: it does not
    /// check types or connection limits (the editor does that, see
    /// [`GraphEditorState::try_connect`](crate::GraphEditorState::try_connect)).
    /// Returns `false` if the connection already existed.
    pub fn add_connection(&mut self, output: OutputId, input: InputId) -> bool {
        let outputs = self.connections.entry(input).map(|e| e.or_default());
        match outputs {
            Some(outputs) if !outputs.contains(&output) => {
                outputs.push(output);
                true
            }
            _ => false,
        }
    }

    /// Removes a single connection. Returns whether it existed.
    pub fn remove_connection(&mut self, input: InputId, output: OutputId) -> bool {
        let Some(outputs) = self.connections.get_mut(input) else {
            return false;
        };
        let before = outputs.len();
        outputs.retain(|id| *id != output);
        let removed = outputs.len() != before;
        if outputs.is_empty() {
            self.connections.remove(input);
        }
        removed
    }

    /// Removes every connection into `input`, returning the outputs that were attached.
    pub fn remove_connections_to(&mut self, input: InputId) -> Vec<OutputId> {
        self.connections.remove(input).unwrap_or_default()
    }

    pub fn iter_nodes(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.nodes.keys()
    }

    /// Every connection as an `(input, output)` pair.
    pub fn iter_connections(&self) -> impl Iterator<Item = (InputId, OutputId)> + '_ {
        self.connections
            .iter()
            .flat_map(|(input, outputs)| outputs.iter().map(move |output| (input, *output)))
    }

    /// All outputs connected to `input`.
    pub fn connections(&self, input: InputId) -> &[OutputId] {
        self.connections
            .get(input)
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    /// The first output connected to `input`, if any.
    pub fn connection(&self, input: InputId) -> Option<OutputId> {
        self.connections(input).first().copied()
    }

    /// All inputs that `output` feeds.
    pub fn output_targets(&self, output: OutputId) -> impl Iterator<Item = InputId> + '_ {
        self.iter_connections()
            .filter(move |(_, connected)| *connected == output)
            .map(|(input, _)| input)
    }

    pub fn is_connected(&self, param: AnyParameterId) -> bool {
        match param {
            AnyParameterId::Input(input) => !self.connections(input).is_empty(),
            AnyParameterId::Output(output) => self.output_targets(output).next().is_some(),
        }
    }

    pub fn any_param_type(&self, param: AnyParameterId) -> Result<&DataType, NodeGraphError> {
        match param {
            AnyParameterId::Input(input) => self.inputs.get(input).map(|p| &p.typ),
            AnyParameterId::Output(output) => self.outputs.get(output).map(|p| &p.typ),
        }
        .ok_or(NodeGraphError::InvalidParameterId(param))
    }

    /// The node that owns `param`.
    pub fn param_node(&self, param: AnyParameterId) -> Option<NodeId> {
        match param {
            AnyParameterId::Input(input) => self.inputs.get(input).map(|p| p.node),
            AnyParameterId::Output(output) => self.outputs.get(output).map(|p| p.node),
        }
    }

    pub fn try_get_input(&self, input: InputId) -> Option<&InputParam<DataType, ValueType>> {
        self.inputs.get(input)
    }

    pub fn get_input(&self, input: InputId) -> &InputParam<DataType, ValueType> {
        &self.inputs[input]
    }

    pub fn try_get_output(&self, output: OutputId) -> Option<&OutputParam<DataType>> {
        self.outputs.get(output)
    }

    pub fn get_output(&self, output: OutputId) -> &OutputParam<DataType> {
        &self.outputs[output]
    }
}

impl<NodeData, DataType, ValueType> Index<NodeId> for Graph<NodeData, DataType, ValueType> {
    type Output = Node<NodeData>;

    fn index(&self, index: NodeId) -> &Self::Output {
        &self.nodes[index]
    }
}

impl<NodeData, DataType, ValueType> IndexMut<NodeId> for Graph<NodeData, DataType, ValueType> {
    fn index_mut(&mut self, index: NodeId) -> &mut Self::Output {
        &mut self.nodes[index]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type TestGraph = Graph<(), u8, f32>;

    fn adder(graph: &mut TestGraph) -> NodeId {
        graph.add_node("add", (), |graph, node| {
            graph.add_input_param(
                node,
                "a",
                0,
                0.0,
                InputParamKind::ConnectionOrConstant,
                true,
            );
            graph.add_input_param(
                node,
                "b",
                0,
                0.0,
                InputParamKind::ConnectionOrConstant,
                true,
            );
            graph.add_output_param(node, "out", 0);
        })
    }

    #[test]
    fn builds_nodes_with_named_params() {
        let mut graph = TestGraph::new();
        let node = adder(&mut graph);
        assert_eq!(graph[node].inputs.len(), 2);
        assert_eq!(graph[node].outputs.len(), 1);
        assert!(graph[node].get_input("b").is_ok());
        assert_eq!(
            graph[node].get_input("missing"),
            Err(NodeGraphError::NoParameterNamed(node, "missing".into()))
        );
    }

    #[test]
    fn connections_are_deduplicated_and_removable() {
        let mut graph = TestGraph::new();
        let a = adder(&mut graph);
        let b = adder(&mut graph);
        let out = graph[a].get_output("out").unwrap();
        let input = graph[b].get_input("a").unwrap();

        assert!(graph.add_connection(out, input));
        assert!(!graph.add_connection(out, input));
        assert_eq!(graph.connection(input), Some(out));
        assert!(graph.is_connected(AnyParameterId::Output(out)));

        assert!(graph.remove_connection(input, out));
        assert!(graph.connections(input).is_empty());
        assert_eq!(graph.iter_connections().count(), 0);
    }

    #[test]
    fn removing_a_node_severs_its_connections_both_ways() {
        let mut graph = TestGraph::new();
        let a = adder(&mut graph);
        let b = adder(&mut graph);
        let c = adder(&mut graph);
        let a_out = graph[a].get_output("out").unwrap();
        let b_out = graph[b].get_output("out").unwrap();
        let b_in = graph[b].get_input("a").unwrap();
        let c_in = graph[c].get_input("a").unwrap();
        graph.add_connection(a_out, b_in);
        graph.add_connection(b_out, c_in);

        let (removed, disconnected) = graph.remove_node(b);
        assert_eq!(removed.label, "add");
        assert_eq!(disconnected.len(), 2);
        assert!(disconnected.contains(&(b_in, a_out)));
        assert!(disconnected.contains(&(c_in, b_out)));
        assert_eq!(graph.iter_connections().count(), 0);
        assert!(graph.try_get_input(b_in).is_none());
        assert!(graph.try_get_output(b_out).is_none());
    }

    #[test]
    fn wide_inputs_hold_many_connections() {
        let mut graph = TestGraph::new();
        let sum = graph.add_node("sum", (), |graph, node| {
            graph.add_wide_input_param(
                node,
                "values",
                0,
                0.0,
                InputParamKind::ConnectionOnly,
                None,
                false,
            );
        });
        let values = graph[sum].get_input("values").unwrap();
        let sources: Vec<_> = (0..3)
            .map(|_| {
                let node = adder(&mut graph);
                graph[node].get_output("out").unwrap()
            })
            .collect();
        for source in &sources {
            graph.add_connection(*source, values);
        }
        assert_eq!(graph.connections(values), sources.as_slice());
        assert_eq!(graph.remove_connections_to(values), sources);
    }

    #[test]
    fn removing_params_cleans_up_connections() {
        let mut graph = TestGraph::new();
        let a = adder(&mut graph);
        let b = adder(&mut graph);
        let out = graph[a].get_output("out").unwrap();
        let input = graph[b].get_input("a").unwrap();
        graph.add_connection(out, input);

        graph.remove_output_param(out);
        assert_eq!(graph.iter_connections().count(), 0);
        assert!(graph[a].outputs.is_empty());

        graph.remove_input_param(input);
        assert_eq!(graph[b].inputs.len(), 1);
    }
}
