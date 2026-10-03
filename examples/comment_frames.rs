//! Comment frames: a frame is just a node drawn under the others, and an
//! `EditApplied` observer moves the nodes inside it along with it. The extra
//! move carries its own `EditOrigin`, so it does not trigger itself and an
//! undo stack can tell the two apart.
//!
//! ```sh
//! cargo run --example comment_frames --features default_style
//! ```

use bevy::prelude::*;
use bevy_noodle::prelude::*;
use bevy_noodle::style::kit;

const NUMBER: PortType = PortType::named("number");
const BLUE: Color = Color::srgb(0.25, 0.52, 0.9);
/// Marks the moves frames make.
const CARRIED: EditOrigin = EditOrigin::Custom(1);

fn main() {
    App::new()
        .add_plugins((DefaultPlugins, NoodlePlugins, NoodleDefaultStylePlugin))
        .insert_resource(ClearColor(Color::srgb_u8(24, 25, 29)))
        .add_systems(Startup, setup)
        .add_observer(carry_contents)
        .run();
}

#[derive(Component)]
struct Frame;

fn setup(mut commands: Commands) {
    commands.spawn(Camera2d);
    let canvas = commands.spawn(kit::canvas()).id();
    let content = commands.spawn((CanvasContent, ChildOf(canvas))).id();
    for (title, at, size, color) in [
        (
            "Inputs",
            Vec2::new(40.0, 60.0),
            Vec2::new(260.0, 330.0),
            (0.35, 0.55, 0.95),
        ),
        (
            "Math",
            Vec2::new(360.0, 120.0),
            Vec2::new(260.0, 200.0),
            (0.95, 0.7, 0.3),
        ),
    ] {
        let (r, g, b) = color;
        commands.spawn((
            Frame,
            GraphNode,
            NodePosition(at),
            // Under the nodes, whatever order they are raised in.
            ZIndex(-1),
            Node {
                width: px(size.x),
                height: px(size.y),
                padding: UiRect::all(px(8)),
                border: UiRect::all(px(1.5)),
                border_radius: BorderRadius::all(px(10)),
                ..default()
            },
            BackgroundColor(Color::srgba(r, g, b, 0.08)),
            BorderColor::all(Color::srgba(r, g, b, 0.5)),
            ChildOf(content),
            children![(
                Text::new(title),
                TextFont::from_font_size(16.0),
                TextColor(Color::srgb(r, g, b)),
                bevy::picking::Pickable::IGNORE,
            )],
        ));
    }
    for (name, y) in [("First", 110.0), ("Second", 250.0)] {
        commands.spawn((
            kit::node(Vec2::new(80.0, y)),
            ChildOf(content),
            children![kit::title(name), kit::output("value", NUMBER, BLUE)],
        ));
    }
    commands.spawn((
        kit::node(Vec2::new(410.0, 170.0)),
        ChildOf(content),
        children![
            kit::title("Add"),
            kit::input("a", NUMBER, BLUE),
            kit::input("b", NUMBER, BLUE),
            kit::output("sum", NUMBER, BLUE),
        ],
    ));
    commands.spawn((
        kit::node(Vec2::new(720.0, 200.0)),
        ChildOf(content),
        children![kit::title("Outside"), kit::input("value", NUMBER, BLUE)],
    ));
}

/// When a frame moves, the nodes that were inside it move too.
fn carry_contents(
    applied: On<EditApplied>,
    frames: Query<(&NodePosition, &ComputedNode), With<Frame>>,
    nodes: Query<(Entity, &NodePosition, &ComputedNode), Without<Frame>>,
    graph: GraphQuery,
    mut commands: Commands,
) {
    let GraphEdit::MoveNodes {
        nodes: moved,
        delta,
        total,
        is_final,
    } = &applied.edit
    else {
        return;
    };
    if applied.origin == CARRIED {
        return;
    }
    let rect = |position: Vec2, computed: &ComputedNode| {
        Rect::from_corners(
            position,
            position + computed.size() * computed.inverse_scale_factor(),
        )
    };
    let mut carried = Vec::new();
    for (frame, computed) in moved.iter().filter_map(|f| frames.get(*f).ok()) {
        // Where the frame was before this step.
        let before = rect(frame.0 - *delta, computed);
        for (node, position, computed) in &nodes {
            let inside = before.contains(rect(position.0, computed).min)
                && before.contains(rect(position.0, computed).max);
            let here = graph.canvas_of(node) == Some(applied.canvas);
            if inside && here && !moved.contains(&node) && !carried.contains(&node) {
                carried.push(node);
            }
        }
    }
    if !carried.is_empty() {
        let edit = GraphEdit::MoveNodes {
            nodes: carried,
            delta: *delta,
            total: *total,
            is_final: *is_final,
        };
        commands.graph_edit_with_origin(applied.canvas, edit, CARRIED);
    }
}
