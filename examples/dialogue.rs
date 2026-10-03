//! A dialogue tree editor. Shows text, choice and checkbox widgets,
//! constant-only parameters, inputs accepting many wires, a node that cannot
//! be deleted, and walking the graph to preview the script.
//!
//! ```sh
//! cargo run --example dialogue
//! ```

use std::borrow::Cow;

use bevy::prelude::*;
use bevy_noodle::prelude::*;

const SPEAKERS: [&str; 3] = ["Narrator", "Innkeeper", "Hero"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Wire {
    Flow,
    Flag,
}

impl DataTypeTrait for Wire {
    fn color(&self) -> Color {
        match self {
            Wire::Flow => Color::srgb_u8(220, 222, 228),
            Wire::Flag => Color::srgb_u8(226, 104, 104),
        }
    }

    fn name(&self) -> Cow<'_, str> {
        match self {
            Wire::Flow => "flow".into(),
            Wire::Flag => "flag".into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Value {
    None,
    Text { text: String, multiline: bool },
    Speaker(usize),
    Flag(bool),
}

impl WidgetValueTrait for Value {
    fn value_widget(&self, _param_name: &str) -> ValueWidget {
        match self {
            Value::None => ValueWidget::None,
            Value::Text { text, multiline } => ValueWidget::Text {
                value: text.clone(),
                multiline: *multiline,
            },
            Value::Speaker(selected) => ValueWidget::Choice {
                options: SPEAKERS.iter().map(|s| s.to_string()).collect(),
                selected: *selected,
            },
            Value::Flag(value) => ValueWidget::Bool(*value),
        }
    }

    fn apply_edit(&mut self, edit: ValueEdit) {
        match (self, edit) {
            (Value::Text { text, .. }, ValueEdit::Text(new)) => *text = new,
            (Value::Speaker(selected), ValueEdit::Choice(new)) => *selected = new,
            (Value::Flag(value), ValueEdit::Bool(new)) => *value = new,
            _ => {}
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Template {
    Start,
    Line,
    Branch,
    Flag,
    End,
    Note,
}

impl Template {
    const ALL: [Template; 5] = [
        Template::Line,
        Template::Branch,
        Template::Flag,
        Template::End,
        Template::Note,
    ];
}

impl NodeTemplateTrait for Template {
    type NodeData = DialogueNode;

    fn node_finder_label(&self) -> Cow<'_, str> {
        match self {
            Template::Start => "Start",
            Template::Line => "Line",
            Template::Branch => "Branch",
            Template::Flag => "Flag",
            Template::End => "End",
            Template::Note => "Note",
        }
        .into()
    }

    fn node_finder_categories(&self) -> Vec<&'static str> {
        match self {
            Template::Start | Template::Line | Template::End => vec!["Dialogue"],
            Template::Branch | Template::Flag => vec!["Logic"],
            Template::Note => vec!["Misc"],
        }
    }

    fn user_data(&self) -> DialogueNode {
        DialogueNode { template: *self }
    }

    fn build_node(&self, graph: &mut GraphOf<DialogueNode>, node: NodeId) {
        // Many lines may lead into the same node.
        let flow_in = |graph: &mut GraphOf<DialogueNode>| {
            graph.add_wide_input_param(
                node,
                "from",
                Wire::Flow,
                Value::None,
                InputParamKind::ConnectionOnly,
                None,
                false,
            );
        };
        let text = |graph: &mut GraphOf<DialogueNode>, name: &str, text: &str, multiline: bool| {
            let value = Value::Text {
                text: text.into(),
                multiline,
            };
            graph.add_input_param(
                node,
                name,
                Wire::Flow,
                value,
                InputParamKind::ConstantOnly,
                true,
            );
        };

        match self {
            Template::Start => {
                graph.add_output_param(node, "next", Wire::Flow);
            }
            Template::Line => {
                flow_in(graph);
                graph.add_input_param(
                    node,
                    "speaker",
                    Wire::Flow,
                    Value::Speaker(0),
                    InputParamKind::ConstantOnly,
                    true,
                );
                text(graph, "text", "...", true);
                graph.add_output_param(node, "next", Wire::Flow);
            }
            Template::Branch => {
                flow_in(graph);
                graph.add_input_param(
                    node,
                    "condition",
                    Wire::Flag,
                    Value::Flag(false),
                    InputParamKind::ConnectionOrConstant,
                    true,
                );
                graph.add_output_param(node, "true", Wire::Flow);
                graph.add_output_param(node, "false", Wire::Flow);
            }
            Template::Flag => {
                text(graph, "name", "flag", false);
                graph.add_input_param(
                    node,
                    "set",
                    Wire::Flag,
                    Value::Flag(true),
                    InputParamKind::ConstantOnly,
                    true,
                );
                graph.add_output_param(node, "value", Wire::Flag);
            }
            Template::End => flow_in(graph),
            Template::Note => text(graph, "note", "Write anything here.", true),
        }
    }
}

#[derive(Clone, Debug)]
struct DialogueNode {
    template: Template,
}

impl NodeDataTrait for DialogueNode {
    type DataType = Wire;
    type ValueType = Value;

    fn titlebar_color(&self) -> Option<Color> {
        Some(match self.template {
            Template::Start | Template::End => Color::srgb_u8(46, 92, 70),
            Template::Line => return None,
            Template::Branch | Template::Flag => Color::srgb_u8(96, 58, 64),
            Template::Note => Color::srgb_u8(84, 78, 48),
        })
    }

    /// There is always exactly one start.
    fn can_delete(&self) -> bool {
        self.template != Template::Start
    }
}

struct DialogueGraph;

impl NodeGraphSchema for DialogueGraph {
    type NodeData = DialogueNode;
    type NodeTemplate = Template;
}

// ---------------------------------------------------------------------------
// Walking the graph
// ---------------------------------------------------------------------------

fn text_of(graph: &GraphOf<DialogueNode>, node: NodeId, name: &str) -> String {
    match graph[node]
        .get_input(name)
        .map(|input| &graph.get_input(input).value)
    {
        Ok(Value::Text { text, .. }) => text.clone(),
        _ => String::new(),
    }
}

/// The node the `output` named `name` of `node` leads to.
fn follow(graph: &GraphOf<DialogueNode>, node: NodeId, name: &str) -> Option<NodeId> {
    let output = graph[node].get_output(name).ok()?;
    let input = graph.output_targets(output).next()?;
    Some(graph.get_input(input).node)
}

fn flag(graph: &GraphOf<DialogueNode>, node: NodeId) -> bool {
    let Ok(input) = graph[node].get_input("condition") else {
        return false;
    };
    let param = match graph.connection(input) {
        Some(output) => {
            let source = graph.get_output(output).node;
            graph[source]
                .get_input("set")
                .map(|input| graph.get_input(input))
        }
        None => Ok(graph.get_input(input)),
    };
    matches!(param.map(|param| &param.value), Ok(Value::Flag(true)))
}

fn preview(graph: &GraphOf<DialogueNode>) -> String {
    let Some(mut node) = graph
        .iter_nodes()
        .find(|node| graph[*node].user_data.template == Template::Start)
    else {
        return "(no start node)".into();
    };
    let mut lines = Vec::new();
    for _ in 0..64 {
        let next = match graph[node].user_data.template {
            Template::Start => follow(graph, node, "next"),
            Template::Line => {
                let speaker = match graph[node]
                    .get_input("speaker")
                    .map(|input| &graph.get_input(input).value)
                {
                    Ok(Value::Speaker(index)) => SPEAKERS[*index],
                    _ => "?",
                };
                lines.push(format!("{speaker}: {}", text_of(graph, node, "text")));
                follow(graph, node, "next")
            }
            Template::Branch => {
                let branch = if flag(graph, node) { "true" } else { "false" };
                lines.push(format!("[branch -> {branch}]"));
                follow(graph, node, branch)
            }
            Template::End => {
                lines.push("[end]".into());
                None
            }
            Template::Flag | Template::Note => None,
        };
        match next {
            Some(next) => node = next,
            None => break,
        }
    }
    lines.join("\n\n")
}

#[derive(Component)]
struct PreviewText;

fn update_preview(
    mut responses: MessageReader<NodeGraphResponse<DialogueGraph>>,
    editors: Query<&NodeGraphEditor<DialogueGraph>>,
    mut texts: Query<&mut Text, With<PreviewText>>,
    mut shown_once: Local<bool>,
) {
    let changed = responses.read().count() > 0;
    if !changed && *shown_once {
        return;
    }
    *shown_once = true;
    let (Ok(editor), Ok(mut text)) = (editors.single(), texts.single_mut()) else {
        return;
    };
    text.0 = preview(editor.graph());
}

/// Fits the whole graph in view once the nodes have been laid out.
fn frame_once(mut editors: Query<&mut NodeGraphEditor<DialogueGraph>>, mut done: Local<bool>) {
    if *done {
        return;
    }
    for mut editor in &mut editors {
        let laid_out = editor
            .graph()
            .iter_nodes()
            .all(|node| editor.node_screen_rect(node).is_some());
        if laid_out && editor.canvas_size() != Vec2::ZERO {
            editor.frame_all();
            *done = true;
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
                title: "bevy_noodle: dialogue".into(),
                resolution: (1280u32, 800u32).into(),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(NodeGraphPlugin::<DialogueGraph>::default())
        .add_systems(Startup, setup)
        .add_systems(Update, (update_preview, frame_once))
        .run();
}

fn setup(mut commands: Commands) {
    commands.spawn(Camera2d);

    let mut editor = NodeGraphEditor::<DialogueGraph>::new(Template::ALL);
    let state = &mut editor.state;
    let start = state.add_node(&Template::Start, Vec2::new(0.0, 150.0));
    let greet = state.add_node(&Template::Line, Vec2::new(260.0, 60.0));
    let flag = state.add_node(&Template::Flag, Vec2::new(260.0, 360.0));
    let branch = state.add_node(&Template::Branch, Vec2::new(560.0, 170.0));
    let yes = state.add_node(&Template::Line, Vec2::new(840.0, 30.0));
    let no = state.add_node(&Template::Line, Vec2::new(840.0, 300.0));
    let end = state.add_node(&Template::End, Vec2::new(1120.0, 200.0));
    let note = state.add_node(&Template::Note, Vec2::new(560.0, 430.0));

    let set =
        |state: &mut GraphEditorState<DialogueNode>, node: NodeId, name: &str, value: Value| {
            let input = state.graph[node].get_input(name).unwrap();
            state.graph.inputs[input].value = value;
        };
    let line = |text: &str| Value::Text {
        text: text.into(),
        multiline: true,
    };
    set(state, greet, "speaker", Value::Speaker(1));
    set(
        state,
        greet,
        "text",
        line("Welcome, traveler! Have you a room booked?"),
    );
    set(
        state,
        flag,
        "name",
        Value::Text {
            text: "has_booking".into(),
            multiline: false,
        },
    );
    set(state, yes, "speaker", Value::Speaker(2));
    set(state, yes, "text", line("I do. Under the name of Ash."));
    set(state, no, "speaker", Value::Speaker(2));
    set(state, no, "text", line("Not yet. Is there space left?"));
    set(
        state,
        note,
        "note",
        line("Toggle 'set' on the flag to change the branch."),
    );

    let wire = |state: &mut GraphEditorState<DialogueNode>,
                from: NodeId,
                output: &str,
                to: NodeId,
                input: &str| {
        let output = state.graph[from].get_output(output).unwrap();
        let input = state.graph[to].get_input(input).unwrap();
        state.try_connect(output, input).unwrap();
    };
    wire(state, start, "next", greet, "from");
    wire(state, greet, "next", branch, "from");
    wire(state, flag, "value", branch, "condition");
    wire(state, branch, "true", yes, "from");
    wire(state, branch, "false", no, "from");
    wire(state, yes, "next", end, "from");
    wire(state, no, "next", end, "from");

    commands
        .spawn(Node {
            width: percent(100),
            height: percent(100),
            ..default()
        })
        .with_children(|root| {
            root.spawn((
                editor,
                Node {
                    flex_grow: 1.0,
                    height: percent(100),
                    ..default()
                },
            ));
            root.spawn((
                Node {
                    width: px(300),
                    flex_direction: FlexDirection::Column,
                    row_gap: px(10),
                    padding: UiRect::all(px(14)),
                    ..default()
                },
                BackgroundColor(Color::srgb_u8(18, 19, 22)),
            ))
            .with_children(|panel| {
                panel.spawn((
                    Text::new("Script preview"),
                    TextFont::from_font_size(16.0),
                    TextColor(Color::srgb_u8(236, 238, 241)),
                ));
                panel.spawn((
                    PreviewText,
                    Text::default(),
                    TextFont::from_font_size(13.0),
                    TextColor(Color::srgb_u8(180, 186, 196)),
                ));
            });
        });
}
