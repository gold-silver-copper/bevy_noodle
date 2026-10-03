//! A small calculator over scalars and 2D vectors, ported from the
//! egui_node_graph2 example. Every node shows the value it evaluates to.
//!
//! ```sh
//! cargo run --example math_graph
//! ```

use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt;
use std::hash::{DefaultHasher, Hash, Hasher};

use bevy::prelude::*;
use bevy_noodle::prelude::*;

// ---------------------------------------------------------------------------
// The graph's types
// ---------------------------------------------------------------------------

/// What flows over the wires.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MathType {
    Scalar,
    Vec2,
}

impl DataTypeTrait for MathType {
    fn color(&self) -> Color {
        match self {
            MathType::Scalar => Color::srgb_u8(64, 132, 230),
            MathType::Vec2 => Color::srgb_u8(238, 200, 96),
        }
    }

    fn name(&self) -> Cow<'_, str> {
        match self {
            MathType::Scalar => "scalar".into(),
            MathType::Vec2 => "2d vector".into(),
        }
    }
}

/// The constant held by each input, edited inline when it isn't connected.
#[derive(Clone, Copy, Debug, PartialEq)]
enum MathValue {
    Scalar(f32),
    Vec2(Vec2),
}

impl MathValue {
    fn scalar(self) -> Result<f32, String> {
        match self {
            MathValue::Scalar(value) => Ok(value),
            other => Err(format!("expected a scalar, got {other}")),
        }
    }

    fn vec2(self) -> Result<Vec2, String> {
        match self {
            MathValue::Vec2(value) => Ok(value),
            other => Err(format!("expected a vector, got {other}")),
        }
    }
}

impl fmt::Display for MathValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MathValue::Scalar(value) => write!(f, "{value:.2}"),
            MathValue::Vec2(value) => write!(f, "({:.2}, {:.2})", value.x, value.y),
        }
    }
}

impl WidgetValueTrait for MathValue {
    fn value_widget(&self, _param_name: &str) -> ValueWidget {
        let number = |value: f32| NumberField::new(value as f64).speed(0.05).decimals(2);
        match self {
            MathValue::Scalar(value) => ValueWidget::Number(number(*value)),
            MathValue::Vec2(value) => {
                ValueWidget::Numbers(vec![number(value.x).label("x"), number(value.y).label("y")])
            }
        }
    }

    fn apply_edit(&mut self, edit: ValueEdit) {
        let ValueEdit::Number { component, value } = edit else {
            return;
        };
        match (self, component) {
            (MathValue::Scalar(scalar), _) => *scalar = value as f32,
            (MathValue::Vec2(vector), 0) => vector.x = value as f32,
            (MathValue::Vec2(vector), _) => vector.y = value as f32,
        }
    }
}

/// The node kinds offered in the node finder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MathTemplate {
    MakeScalar,
    AddScalar,
    SubtractScalar,
    MakeVector,
    AddVector,
    SubtractVector,
    VectorTimesScalar,
}

impl MathTemplate {
    const ALL: [MathTemplate; 7] = [
        MathTemplate::MakeScalar,
        MathTemplate::AddScalar,
        MathTemplate::SubtractScalar,
        MathTemplate::MakeVector,
        MathTemplate::AddVector,
        MathTemplate::SubtractVector,
        MathTemplate::VectorTimesScalar,
    ];
}

impl NodeTemplateTrait for MathTemplate {
    type NodeData = MathNode;

    fn node_finder_label(&self) -> Cow<'_, str> {
        match self {
            MathTemplate::MakeScalar => "New scalar",
            MathTemplate::AddScalar => "Scalar add",
            MathTemplate::SubtractScalar => "Scalar subtract",
            MathTemplate::MakeVector => "New vector",
            MathTemplate::AddVector => "Vector add",
            MathTemplate::SubtractVector => "Vector subtract",
            MathTemplate::VectorTimesScalar => "Vector times scalar",
        }
        .into()
    }

    fn node_finder_categories(&self) -> Vec<&'static str> {
        match self {
            MathTemplate::MakeScalar | MathTemplate::AddScalar | MathTemplate::SubtractScalar => {
                vec!["Scalar"]
            }
            MathTemplate::MakeVector | MathTemplate::AddVector | MathTemplate::SubtractVector => {
                vec!["Vector"]
            }
            MathTemplate::VectorTimesScalar => vec!["Vector", "Scalar"],
        }
    }

    fn user_data(&self) -> MathNode {
        MathNode {
            template: *self,
            result: String::new(),
        }
    }

    fn build_node(&self, graph: &mut GraphOf<MathNode>, node_id: NodeId) {
        let scalar_in = |graph: &mut GraphOf<MathNode>, name: &str| {
            graph.add_input_param(
                node_id,
                name,
                MathType::Scalar,
                MathValue::Scalar(0.0),
                InputParamKind::ConnectionOrConstant,
                true,
            );
        };
        let vector_in = |graph: &mut GraphOf<MathNode>, name: &str| {
            graph.add_input_param(
                node_id,
                name,
                MathType::Vec2,
                MathValue::Vec2(Vec2::ZERO),
                InputParamKind::ConnectionOrConstant,
                true,
            );
        };

        match self {
            MathTemplate::MakeScalar => scalar_in(graph, "value"),
            MathTemplate::AddScalar | MathTemplate::SubtractScalar => {
                scalar_in(graph, "A");
                scalar_in(graph, "B");
            }
            MathTemplate::MakeVector => {
                scalar_in(graph, "x");
                scalar_in(graph, "y");
            }
            MathTemplate::AddVector | MathTemplate::SubtractVector => {
                vector_in(graph, "v1");
                vector_in(graph, "v2");
            }
            MathTemplate::VectorTimesScalar => {
                scalar_in(graph, "scalar");
                vector_in(graph, "vector");
            }
        }

        let output = match self {
            MathTemplate::MakeScalar | MathTemplate::AddScalar | MathTemplate::SubtractScalar => {
                MathType::Scalar
            }
            _ => MathType::Vec2,
        };
        graph.add_output_param(node_id, "out", output);
    }
}

/// Per-node data: the template it came from and the last evaluated value.
#[derive(Clone, Debug)]
struct MathNode {
    template: MathTemplate,
    result: String,
}

impl NodeDataTrait for MathNode {
    type DataType = MathType;
    type ValueType = MathValue;

    fn titlebar_color(&self) -> Option<Color> {
        match self.template {
            MathTemplate::MakeScalar | MathTemplate::MakeVector => Some(Color::srgb_u8(52, 72, 66)),
            _ => None,
        }
    }

    /// Shows the evaluated value under the parameters.
    fn spawn_body(&self, ctx: NodeBodyContext<'_, Self>, body: &mut ChildSpawnerCommands) {
        if self.result.is_empty() {
            return;
        }
        let color = if self.result.starts_with('=') {
            Color::srgb_u8(150, 214, 160)
        } else {
            Color::srgb_u8(232, 128, 120)
        };
        body.spawn((
            Text::new(self.result.clone()),
            ctx.text_font(),
            TextColor(color),
            Pickable::IGNORE,
        ));
    }

    fn body_revision(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        self.result.hash(&mut hasher);
        hasher.finish()
    }
}

/// Ties the types together.
struct MathGraph;

impl NodeGraphSchema for MathGraph {
    type NodeData = MathNode;
    type NodeTemplate = MathTemplate;
}

// ---------------------------------------------------------------------------
// Evaluation
// ---------------------------------------------------------------------------

struct Evaluator<'a> {
    graph: &'a GraphOf<MathNode>,
    cache: HashMap<OutputId, MathValue>,
    stack: Vec<NodeId>,
}

impl<'a> Evaluator<'a> {
    fn new(graph: &'a GraphOf<MathNode>) -> Self {
        Self {
            graph,
            cache: HashMap::new(),
            stack: Vec::new(),
        }
    }

    fn node(&mut self, node_id: NodeId) -> Result<MathValue, String> {
        if let Some(output) = self.graph[node_id].output_ids().next()
            && let Some(value) = self.cache.get(&output)
        {
            return Ok(*value);
        }
        if self.stack.contains(&node_id) {
            return Err("cycle".into());
        }
        self.stack.push(node_id);
        let result = self.compute(node_id);
        self.stack.pop();
        if let (Ok(value), Some(output)) = (&result, self.graph[node_id].output_ids().next()) {
            self.cache.insert(output, *value);
        }
        result
    }

    fn input(&mut self, node_id: NodeId, name: &str) -> Result<MathValue, String> {
        let input = self.graph[node_id]
            .get_input(name)
            .map_err(|error| error.to_string())?;
        match self.graph.connection(input) {
            Some(output) => self.node(self.graph.get_output(output).node),
            None => Ok(self.graph.get_input(input).value),
        }
    }

    fn scalar(&mut self, node_id: NodeId, name: &str) -> Result<f32, String> {
        self.input(node_id, name)?.scalar()
    }

    fn vec2(&mut self, node_id: NodeId, name: &str) -> Result<Vec2, String> {
        self.input(node_id, name)?.vec2()
    }

    fn compute(&mut self, node: NodeId) -> Result<MathValue, String> {
        Ok(match self.graph[node].user_data.template {
            MathTemplate::MakeScalar => MathValue::Scalar(self.scalar(node, "value")?),
            MathTemplate::AddScalar => {
                MathValue::Scalar(self.scalar(node, "A")? + self.scalar(node, "B")?)
            }
            MathTemplate::SubtractScalar => {
                MathValue::Scalar(self.scalar(node, "A")? - self.scalar(node, "B")?)
            }
            MathTemplate::MakeVector => {
                MathValue::Vec2(Vec2::new(self.scalar(node, "x")?, self.scalar(node, "y")?))
            }
            MathTemplate::AddVector => {
                MathValue::Vec2(self.vec2(node, "v1")? + self.vec2(node, "v2")?)
            }
            MathTemplate::SubtractVector => {
                MathValue::Vec2(self.vec2(node, "v1")? - self.vec2(node, "v2")?)
            }
            MathTemplate::VectorTimesScalar => {
                MathValue::Vec2(self.vec2(node, "vector")? * self.scalar(node, "scalar")?)
            }
        })
    }
}

/// Re-evaluates the graph whenever it changes and stores each node's value
/// in its user data, which rebuilds the node's body.
fn evaluate_graph(
    mut responses: MessageReader<NodeGraphResponse<MathGraph>>,
    mut editors: Query<&mut NodeGraphEditor<MathGraph>>,
    mut evaluated_once: Local<bool>,
) {
    let changed = responses.read().any(|message| {
        !matches!(
            message.response,
            NodeResponse::SelectNode(_)
                | NodeResponse::RaiseNode(_)
                | NodeResponse::MoveNode { .. }
        )
    });
    if !changed && *evaluated_once {
        return;
    }
    *evaluated_once = true;

    for mut editor in &mut editors {
        let graph = editor.graph();
        let mut evaluator = Evaluator::new(graph);
        let results: Vec<(NodeId, String)> = graph
            .iter_nodes()
            .map(|node_id| {
                let text = match evaluator.node(node_id) {
                    Ok(value) => format!("= {value}"),
                    Err(error) => format!("error: {error}"),
                };
                (node_id, text)
            })
            .collect();
        for (node_id, text) in results {
            let node = &mut editor.graph_mut()[node_id].user_data;
            if node.result != text {
                node.result = text;
            }
        }
    }
}

fn log_responses(mut responses: MessageReader<NodeGraphResponse<MathGraph>>) {
    for message in responses.read() {
        if !matches!(message.response, NodeResponse::MoveNode { .. }) {
            info!("{:?}", message.response);
        }
    }
}

// ---------------------------------------------------------------------------
// App
// ---------------------------------------------------------------------------

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "bevy_noodle: math graph".into(),
                resolution: (1280u32, 800u32).into(),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(NodeGraphPlugin::<MathGraph>::default())
        .add_systems(Startup, setup)
        .add_systems(Update, (log_responses, evaluate_graph))
        .run();
}

fn setup(mut commands: Commands) {
    commands.spawn(Camera2d);

    let mut editor = NodeGraphEditor::<MathGraph>::new(MathTemplate::ALL);
    let state = &mut editor.state;
    let a = state.add_node(&MathTemplate::MakeScalar, Vec2::new(40.0, 40.0));
    let b = state.add_node(&MathTemplate::MakeScalar, Vec2::new(40.0, 200.0));
    let sum = state.add_node(&MathTemplate::AddScalar, Vec2::new(320.0, 100.0));
    let vector = state.add_node(&MathTemplate::MakeVector, Vec2::new(320.0, 300.0));
    let scaled = state.add_node(&MathTemplate::VectorTimesScalar, Vec2::new(620.0, 180.0));

    let set_scalar =
        |state: &mut GraphEditorState<MathNode>, node: NodeId, name: &str, value: f32| {
            let input = state.graph[node].get_input(name).unwrap();
            state.graph.inputs[input].value = MathValue::Scalar(value);
        };
    set_scalar(state, a, "value", 2.0);
    set_scalar(state, b, "value", 3.5);
    set_scalar(state, vector, "x", 1.0);
    set_scalar(state, vector, "y", -0.5);

    let wire = |state: &mut GraphEditorState<MathNode>, from: NodeId, to: NodeId, input: &str| {
        let output = state.graph[from].get_output("out").unwrap();
        let input = state.graph[to].get_input(input).unwrap();
        state.try_connect(output, input).unwrap();
    };
    wire(state, a, sum, "A");
    wire(state, b, sum, "B");
    wire(state, sum, scaled, "scalar");
    wire(state, vector, scaled, "vector");

    commands
        .spawn(Node {
            width: percent(100),
            height: percent(100),
            flex_direction: FlexDirection::Column,
            ..default()
        })
        .with_children(|root| {
            root.spawn((
                Node {
                    padding: UiRect::axes(px(12), px(8)),
                    ..default()
                },
                BackgroundColor(Color::srgb_u8(18, 19, 22)),
            ))
            .with_child((
                Text::new(
                    "Right-click: add node  |  Drag ports: connect  |  Del: delete  |  \
                     Middle/Space+drag or scroll: pan  |  Wheel/pinch: zoom  |  Drag a number's label: scrub",
                ),
                TextFont::from_font_size(13.0),
                TextColor(Color::srgb_u8(150, 156, 166)),
            ));
            root.spawn((
                editor,
                Node {
                    width: percent(100),
                    flex_grow: 1.0,
                    ..default()
                },
            ));
        });
}
