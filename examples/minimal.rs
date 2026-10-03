//! No default style at all: you build the nodes with plain Bevy UI and draw
//! the edges yourself (here with gizmos) from `EdgeGeometry`.
//!
//! ```sh
//! cargo run --example minimal
//! ```

use bevy::prelude::*;
use bevy::ui::Selected;
use bevy_noodle::prelude::*;

const NUMBER: PortType = PortType::named("number");

fn main() {
    App::new()
        .add_plugins((DefaultPlugins, NoodlePlugins))
        .add_systems(Startup, setup)
        .add_systems(Update, (draw_edges, show_selection))
        .run();
}

fn setup(mut commands: Commands) {
    commands.spawn(Camera2d);

    // The canvas has no background: whatever is behind it shows through.
    let canvas = commands
        .spawn((
            NodeCanvas,
            Node {
                width: percent(100),
                height: percent(100),
                ..default()
            },
        ))
        .id();
    let content = commands.spawn((CanvasContent, ChildOf(canvas))).id();

    spawn_node(
        &mut commands,
        content,
        "Source",
        Vec2::new(80.0, 120.0),
        &[Port::output(NUMBER)],
    );
    spawn_node(
        &mut commands,
        content,
        "Source",
        Vec2::new(80.0, 300.0),
        &[Port::output(NUMBER)],
    );
    spawn_node(
        &mut commands,
        content,
        "Sink",
        Vec2::new(420.0, 200.0),
        &[Port::input(NUMBER).with_max_connections(None)],
    );
}

/// A node is whatever UI you like, marked with `GraphNode`; ports are any UI
/// entities marked with `Port`.
fn spawn_node(
    commands: &mut Commands,
    content: Entity,
    title: &str,
    position: Vec2,
    ports: &[Port],
) {
    commands
        .spawn((
            GraphNode,
            NodePosition(position),
            ChildOf(content),
            Node {
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(10)),
                row_gap: px(8),
                min_width: px(120),
                border: UiRect::all(px(2)),
                border_radius: BorderRadius::all(px(6)),
                ..default()
            },
            BackgroundColor(Color::srgb(0.16, 0.18, 0.22)),
            BorderColor::all(Color::srgb(0.3, 0.32, 0.38)),
        ))
        .with_children(|node| {
            node.spawn((Text::new(title), Pickable::IGNORE));
            for port in ports {
                let side = match port.direction {
                    PortDirection::Input => AlignSelf::FlexStart,
                    PortDirection::Output => AlignSelf::FlexEnd,
                };
                node.spawn((
                    *port,
                    Node {
                        width: px(14),
                        height: px(14),
                        align_self: side,
                        border_radius: BorderRadius::all(px(7)),
                        ..default()
                    },
                    BackgroundColor(Color::srgb(0.4, 0.7, 1.0)),
                ));
            }
        });
}

/// There is no built-in edge renderer. `EdgeGeometry` (graph space) is all
/// you need; here it becomes a gizmo curve behind the transparent canvas.
fn draw_edges(
    mut gizmos: Gizmos,
    edges: Query<(&Edge, &EdgeGeometry)>,
    pending: Query<(&PendingWire, &CanvasView)>,
    views: Query<&CanvasView>,
    window: Single<&Window>,
) {
    let size = window.size();
    // Canvas-local pixels → 2D world (origin at the window center, y up).
    let to_world = |view: &CanvasView, point: Vec2| {
        let local = view.graph_to_canvas(point);
        Vec2::new(local.x - size.x / 2.0, size.y / 2.0 - local.y)
    };
    let mut draw = |view: &CanvasView, geometry: &EdgeGeometry, color: Color| {
        if !geometry.valid {
            return;
        }
        let points = geometry.bezier(0.5).map(|p| to_world(view, p));
        let curve = CubicBezier::new([points]).to_curve().unwrap();
        gizmos.linestrip_2d(curve.iter_positions(32), color);
    };
    for (edge, geometry) in &edges {
        if let Ok(view) = views.get(edge.canvas) {
            draw(view, geometry, Color::srgb(0.4, 0.7, 1.0));
        }
    }
    for (wire, view) in &pending {
        draw(view, &wire.geometry, Color::srgba(1.0, 1.0, 1.0, 0.6));
    }
}

/// Style reacts to plain components: `Selected` is set by the library.
fn show_selection(mut nodes: Query<(Has<Selected>, &mut BorderColor), With<GraphNode>>) {
    for (selected, mut border) in &mut nodes {
        let color = if selected {
            Color::srgb(1.0, 0.8, 0.3)
        } else {
            Color::srgb(0.3, 0.32, 0.38)
        };
        if border.top != color {
            *border = BorderColor::all(color);
        }
    }
}
