//! The smallest useful node graph: numbers flowing into an "Add" node.
//!
//! ```sh
//! cargo run --example minimal
//! ```

use std::borrow::Cow;

use bevy::prelude::*;
use bevy_noodle::prelude::*;

/// One data type: every port carries a number.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Number;

impl DataTypeTrait for Number {
    fn color(&self) -> Color {
        Color::srgb(0.3, 0.6, 1.0)
    }

    fn name(&self) -> Cow<'_, str> {
        "number".into()
    }
}

/// The constant stored in each input, edited inline when unconnected.
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

/// The node kinds users can create.
#[derive(Clone, Copy)]
enum Template {
    Constant,
    Add,
}

impl NodeTemplateTrait for Template {
    type NodeData = NodeData;

    fn node_finder_label(&self) -> Cow<'_, str> {
        match self {
            Template::Constant => "Constant".into(),
            Template::Add => "Add".into(),
        }
    }

    fn user_data(&self) -> NodeData {
        NodeData
    }

    fn build_node(&self, graph: &mut GraphOf<NodeData>, node: NodeId) {
        let kind = InputParamKind::ConnectionOrConstant;
        match self {
            Template::Constant => {
                graph.add_input_param(
                    node,
                    "value",
                    Number,
                    Value(1.0),
                    InputParamKind::ConstantOnly,
                    true,
                );
            }
            Template::Add => {
                graph.add_input_param(node, "a", Number, Value(0.0), kind, true);
                graph.add_input_param(node, "b", Number, Value(0.0), kind, true);
            }
        }
        graph.add_output_param(node, "out", Number);
    }
}

/// Per-node data. Nothing needed here.
#[derive(Clone)]
struct NodeData;

impl NodeDataTrait for NodeData {
    type DataType = Number;
    type ValueType = Value;
}

/// Ties the types together.
struct MyGraph;

impl NodeGraphSchema for MyGraph {
    type NodeData = NodeData;
    type NodeTemplate = Template;
}

fn main() {
    App::new()
        .add_plugins((DefaultPlugins, NodeGraphPlugin::<MyGraph>::default()))
        .add_systems(Startup, setup)
        .add_systems(Update, print_responses)
        .run();
}

fn setup(mut commands: Commands) {
    commands.spawn(Camera2d);

    let mut editor = NodeGraphEditor::<MyGraph>::new([Template::Constant, Template::Add]);
    editor
        .state
        .add_node(&Template::Constant, Vec2::new(40.0, 40.0));
    editor
        .state
        .add_node(&Template::Add, Vec2::new(320.0, 80.0));

    // With no size given, the editor fills its parent.
    commands.spawn(editor);
}

/// Everything the user does arrives as a message.
fn print_responses(mut responses: MessageReader<NodeGraphResponse<MyGraph>>) {
    for message in responses.read() {
        match &message.response {
            NodeResponse::ConnectEventEnded { output, input } => {
                info!("connected {output:?} -> {input:?}")
            }
            NodeResponse::CreatedNode(node) => info!("created {node:?}"),
            NodeResponse::DeleteNodeFull { node_id, .. } => info!("deleted {node_id:?}"),
            _ => {}
        }
    }
}
