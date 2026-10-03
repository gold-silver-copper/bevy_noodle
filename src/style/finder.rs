//! A searchable "add node" popup.
//!
//! Insert a [`NodeFinder`] on a canvas. It opens on a right click on empty
//! canvas (configurable) and when a wire is dropped on empty canvas; in the
//! second case only templates that can take the wire are listed, and the new
//! node gets connected.

use std::sync::Arc;

use bevy::input::ButtonState;
use bevy::input::keyboard::KeyboardInput;
use bevy::input::mouse::MouseScrollUnit;
use bevy::input_focus::{FocusCause, FocusedInput, InputFocus};
use bevy::picking::pointer::PointerButton;
use bevy::prelude::*;
use bevy::text::{EditableText, LineBreak, TextCursorStyle};
use bevy::ui::{ComputedNode, ui_transform::UiGlobalTransform};

use crate::components::{CanvasContent, CanvasView, NodeCanvas, PortDirection};
use crate::edit::{EditOrigin, GraphEdit, GraphWorldExt};
use crate::interaction::{WireDropped, canvas_local};
use crate::query::{GraphQuery, PortInfo, check_connection};

pub(super) fn plugin(app: &mut App) {
    app.add_observer(open_on_click)
        .add_observer(open_on_wire_drop)
        .add_observer(close_on_canvas_press)
        .add_observer(on_search_key)
        .add_observer(on_item_click)
        .add_systems(
            PostUpdate,
            rebuild_lists.in_set(crate::NoodleSystems::Render),
        );
}

/// Spawns a node as a child of `content` at `position` (graph space) and
/// returns it.
pub type SpawnNodeFn = Arc<dyn Fn(&mut Commands, Entity, Vec2) -> Entity + Send + Sync>;

/// One entry in the finder: a label and how to spawn the node.
#[derive(Clone)]
pub struct NodeTemplate {
    pub label: String,
    pub category: Option<String>,
    /// Spawns the node as a child of `content` at `position` (graph space)
    /// and returns it.
    pub spawn: SpawnNodeFn,
}

impl NodeTemplate {
    pub fn new(
        label: impl Into<String>,
        spawn: impl Fn(&mut Commands, Entity, Vec2) -> Entity + Send + Sync + 'static,
    ) -> Self {
        Self {
            label: label.into(),
            category: None,
            spawn: Arc::new(spawn),
        }
    }

    pub fn in_category(mut self, category: impl Into<String>) -> Self {
        self.category = Some(category.into());
        self
    }
}

/// Enables the finder popup on a canvas.
#[derive(Component, Clone)]
pub struct NodeFinder {
    pub templates: Vec<NodeTemplate>,
    /// Clicking empty canvas with this button opens the finder.
    pub open_button: Option<PointerButton>,
    /// Dropping a wire on empty canvas opens the finder.
    pub open_on_wire_drop: bool,
    pub background: Color,
    pub border: Color,
    pub text: Color,
    pub muted: Color,
    pub highlight: Color,
}

impl NodeFinder {
    pub fn new(templates: impl IntoIterator<Item = NodeTemplate>) -> Self {
        Self {
            templates: templates.into_iter().collect(),
            open_button: Some(PointerButton::Secondary),
            open_on_wire_drop: true,
            background: Color::srgb_u8(32, 33, 38),
            border: Color::srgb_u8(72, 75, 84),
            text: Color::srgb_u8(222, 225, 230),
            muted: Color::srgb_u8(146, 152, 162),
            highlight: Color::srgb_u8(78, 82, 92),
        }
    }
}

/// The open popup (one per canvas).
#[derive(Component)]
struct FinderPopup {
    canvas: Entity,
    position: Vec2,
    pending: Option<Entity>,
    search: Entity,
    list: Entity,
    shown: Option<String>,
}

#[derive(Component)]
struct FinderSearch {
    popup: Entity,
}

#[derive(Component)]
struct FinderItem {
    popup: Entity,
    template: usize,
}

fn open_on_click(
    click: On<Pointer<Click>>,
    graph: GraphQuery,
    canvases: Query<
        (&NodeFinder, &CanvasView, &ComputedNode, &UiGlobalTransform),
        With<NodeCanvas>,
    >,
    popups: Query<(Entity, &FinderPopup)>,
    focus: Option<ResMut<InputFocus>>,
    mut commands: Commands,
) {
    let canvas = click.event_target();
    let Ok((finder, view, computed, transform)) = canvases.get(canvas) else {
        return;
    };
    if finder.open_button != Some(click.button)
        || graph.node_of(click.original_event_target()).is_some()
    {
        return;
    }
    let Some(local) = canvas_local(click.pointer_location.position, computed, transform) else {
        return;
    };
    open(
        &mut commands,
        canvas,
        finder,
        local,
        view.canvas_to_graph(local),
        None,
        &popups,
        focus,
    );
}

fn open_on_wire_drop(
    dropped: On<WireDropped>,
    canvases: Query<(&NodeFinder, &CanvasView), With<NodeCanvas>>,
    popups: Query<(Entity, &FinderPopup)>,
    focus: Option<ResMut<InputFocus>>,
    mut commands: Commands,
) {
    let Ok((finder, view)) = canvases.get(dropped.canvas) else {
        return;
    };
    if !finder.open_on_wire_drop {
        return;
    }
    let local = view.graph_to_canvas(dropped.position);
    open(
        &mut commands,
        dropped.canvas,
        finder,
        local,
        dropped.position,
        Some(dropped.from),
        &popups,
        focus,
    );
}

fn open(
    commands: &mut Commands,
    canvas: Entity,
    finder: &NodeFinder,
    local: Vec2,
    position: Vec2,
    pending: Option<Entity>,
    popups: &Query<(Entity, &FinderPopup)>,
    focus: Option<ResMut<InputFocus>>,
) {
    close(commands, canvas, popups);
    let popup = commands.spawn_empty().id();
    let mut text = EditableText::new("");
    text.cursor_width = 0.15;
    let search = commands
        .spawn((
            FinderSearch { popup },
            Node {
                padding: UiRect::axes(Val::Px(6.0), Val::Px(4.0)),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(4.0)),
                ..default()
            },
            text,
            TextLayout::linebreak(LineBreak::NoWrap),
            TextFont::from_font_size(13.0),
            TextColor(finder.text),
            TextCursorStyle {
                color: finder.text,
                selection_color: finder.highlight,
                ..default()
            },
            BackgroundColor(finder.background.darker(0.05)),
            BorderColor::all(finder.border),
        ))
        .id();
    let list = commands
        .spawn((
            Node {
                flex_direction: FlexDirection::Column,
                max_height: Val::Px(300.0),
                overflow: Overflow::scroll_y(),
                ..default()
            },
            ScrollPosition::default(),
        ))
        .observe(scroll_list)
        .id();
    commands
        .entity(popup)
        .insert((
            FinderPopup {
                canvas,
                position,
                pending,
                search,
                list,
                shown: None,
            },
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(local.x),
                top: Val::Px(local.y),
                width: Val::Px(220.0),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(Val::Px(6.0)),
                row_gap: Val::Px(6.0),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(8.0)),
                ..default()
            },
            BackgroundColor(finder.background),
            BorderColor::all(finder.border),
            GlobalZIndex(1000),
            ChildOf(canvas),
        ))
        .add_children(&[search, list])
        // Clicks and drags inside the popup are not canvas interaction.
        .observe(|mut e: On<Pointer<Press>>| e.propagate(false))
        .observe(|mut e: On<Pointer<Click>>| e.propagate(false))
        .observe(|mut e: On<Pointer<DragStart>>| e.propagate(false))
        .observe(|mut e: On<Pointer<Scroll>>| e.propagate(false));
    if let Some(mut focus) = focus {
        focus.set(search, FocusCause::Navigated);
    }
}

fn close(commands: &mut Commands, canvas: Entity, popups: &Query<(Entity, &FinderPopup)>) {
    for (entity, popup) in popups {
        if popup.canvas == canvas {
            commands.entity(entity).try_despawn();
        }
    }
}

fn close_on_canvas_press(
    press: On<Pointer<Press>>,
    canvases: Query<(), With<NodeFinder>>,
    popups: Query<(Entity, &FinderPopup)>,
    mut commands: Commands,
) {
    let target = press.event_target();
    if canvases.contains(target) {
        close(&mut commands, target, &popups);
    }
}

/// Template indices to show, grouped by category when not searching.
fn entries(
    finder: &NodeFinder,
    search: &str,
    pending: Option<&PortInfo>,
    accepts: impl Fn(usize) -> bool,
) -> Vec<(Option<String>, usize)> {
    let search = search.trim().to_lowercase();
    let mut matches: Vec<(Option<String>, usize)> = finder
        .templates
        .iter()
        .enumerate()
        .filter(|(index, template)| {
            let text = format!(
                "{} {}",
                template.label,
                template.category.as_deref().unwrap_or_default()
            )
            .to_lowercase();
            (search.is_empty() || text.contains(&search)) && (pending.is_none() || accepts(*index))
        })
        .map(|(index, template)| (template.category.clone(), index))
        .collect();
    if search.is_empty() {
        // Stable grouping by first appearance of each category.
        let order: Vec<Option<String>> =
            matches.iter().fold(Vec::new(), |mut seen, (category, _)| {
                if !seen.contains(category) {
                    seen.push(category.clone());
                }
                seen
            });
        matches.sort_by_key(|(category, _)| order.iter().position(|c| c == category));
    }
    matches
}

/// Whether a template's node would have a port accepting `pending`. Builds
/// the node in a scratch world.
fn template_accepts(template: &NodeTemplate, pending: &PortInfo) -> bool {
    let mut world = World::new();
    let canvas = world.spawn(NodeCanvas).id();
    let content = world.spawn((CanvasContent, ChildOf(canvas))).id();
    let node = {
        let mut queue = bevy::ecs::world::CommandQueue::default();
        let mut commands = Commands::new(&mut queue, &world);
        let node = (template.spawn)(&mut commands, content, Vec2::ZERO);
        queue.apply(&mut world);
        node
    };
    let mut probe = pending.clone();
    // Pretend the pending port lives in this scratch canvas, outside the node.
    probe.canvas = Some(canvas);
    probe.node = Some(Entity::PLACEHOLDER);
    probe.edges.clear();
    probe.peers.clear();
    descendants(&world, node)
        .into_iter()
        .filter_map(|e| PortInfo::from_world(&world, e))
        .any(|info| check_connection(&probe, &info, canvas).is_ok())
}

fn descendants(world: &World, root: Entity) -> Vec<Entity> {
    let mut out = Vec::new();
    let mut stack = vec![root];
    while let Some(entity) = stack.pop() {
        out.push(entity);
        if let Some(children) = world.get::<Children>(entity) {
            stack.extend(children.iter());
        }
    }
    out
}

fn rebuild_lists(
    mut commands: Commands,
    mut popups: Query<(Entity, &mut FinderPopup)>,
    finders: Query<&NodeFinder>,
    texts: Query<&EditableText, With<FinderSearch>>,
    graph: GraphQuery,
) {
    for (entity, mut popup) in &mut popups {
        let Ok(finder) = finders.get(popup.canvas) else {
            commands.entity(entity).try_despawn();
            continue;
        };
        let search = texts
            .get(popup.search)
            .map(|t| t.value().to_string())
            .unwrap_or_default();
        if popup.shown.as_deref() == Some(search.as_str()) {
            continue;
        }
        popup.shown = Some(search.clone());
        let pending = popup.pending.and_then(|port| graph.port_info(port));
        let rows = entries(finder, &search, pending.as_ref(), |index| {
            pending
                .as_ref()
                .is_some_and(|p| template_accepts(&finder.templates[index], p))
        });

        commands.entity(popup.list).despawn_children();
        let list = popup.list;
        let mut previous_category: Option<Option<String>> = None;
        for (position, (category, index)) in rows.iter().enumerate() {
            if search.trim().is_empty() && previous_category.as_ref() != Some(category) {
                if let Some(name) = category {
                    commands.spawn((
                        Text::new(name.clone()),
                        TextFont::from_font_size(11.0),
                        TextColor(finder.muted),
                        Node {
                            padding: UiRect::new(
                                Val::Px(6.0),
                                Val::Px(6.0),
                                Val::Px(6.0),
                                Val::Px(2.0),
                            ),
                            ..default()
                        },
                        bevy::picking::Pickable::IGNORE,
                        ChildOf(list),
                    ));
                }
                previous_category = Some(category.clone());
            }
            commands.spawn((
                FinderItem {
                    popup: entity,
                    template: *index,
                },
                Button,
                Node {
                    padding: UiRect::axes(Val::Px(8.0), Val::Px(4.0)),
                    border: UiRect::all(Val::Px(1.0)),
                    border_radius: BorderRadius::all(Val::Px(4.0)),
                    ..default()
                },
                BorderColor::all(if position == 0 {
                    finder.highlight
                } else {
                    Color::NONE
                }),
                ChildOf(list),
                children![(
                    Text::new(finder.templates[*index].label.clone()),
                    TextFont::from_font_size(13.0),
                    TextColor(finder.text),
                    bevy::picking::Pickable::IGNORE,
                )],
            ));
        }
        if rows.is_empty() {
            commands.spawn((
                Text::new("No matching nodes"),
                TextFont::from_font_size(13.0),
                TextColor(finder.muted),
                Node {
                    padding: UiRect::axes(Val::Px(8.0), Val::Px(4.0)),
                    ..default()
                },
                ChildOf(list),
            ));
        }
    }
}

fn on_item_click(
    mut click: On<Pointer<Click>>,
    items: Query<&FinderItem>,
    popups: Query<&FinderPopup>,
    finders: Query<&NodeFinder>,
    mut commands: Commands,
) {
    let Ok(item) = items.get(click.event_target()) else {
        return;
    };
    click.propagate(false);
    if click.button == PointerButton::Primary {
        create(&mut commands, item.popup, item.template, &popups, &finders);
    }
}

fn on_search_key(
    mut input: On<FocusedInput<KeyboardInput>>,
    searches: Query<&FinderSearch>,
    popups: Query<&FinderPopup>,
    finders: Query<&NodeFinder>,
    texts: Query<&EditableText>,
    graph: GraphQuery,
    mut commands: Commands,
) {
    let Ok(search) = searches.get(input.event_target()) else {
        return;
    };
    if input.input.state != ButtonState::Pressed {
        return;
    }
    match input.input.key_code {
        KeyCode::Escape => {
            input.propagate(false);
            commands.entity(search.popup).try_despawn();
        }
        KeyCode::Enter | KeyCode::NumpadEnter => {
            input.propagate(false);
            let Ok(popup) = popups.get(search.popup) else {
                return;
            };
            let Ok(finder) = finders.get(popup.canvas) else {
                return;
            };
            let text = texts
                .get(input.event_target())
                .map(|t| t.value().to_string())
                .unwrap_or_default();
            let pending = popup.pending.and_then(|port| graph.port_info(port));
            let first = entries(finder, &text, pending.as_ref(), |index| {
                pending
                    .as_ref()
                    .is_some_and(|p| template_accepts(&finder.templates[index], p))
            })
            .first()
            .map(|(_, index)| *index);
            if let Some(index) = first {
                create(&mut commands, search.popup, index, &popups, &finders);
            }
        }
        _ => {}
    }
}

/// Spawns the template's node, connects the pending wire, closes the popup.
fn create(
    commands: &mut Commands,
    popup_entity: Entity,
    template: usize,
    popups: &Query<&FinderPopup>,
    finders: &Query<&NodeFinder>,
) {
    let Ok(popup) = popups.get(popup_entity) else {
        return;
    };
    let Ok(finder) = finders.get(popup.canvas) else {
        return;
    };
    let Some(template) = finder.templates.get(template).cloned() else {
        return;
    };
    let (canvas, mut position, pending) = (popup.canvas, popup.position, popup.pending);
    commands.entity(popup_entity).try_despawn();
    commands.queue(move |world: &mut World| {
        let Some(content) = world.get::<Children>(canvas).and_then(|children| {
            children
                .iter()
                .find(|c| world.get::<CanvasContent>(*c).is_some())
        }) else {
            return;
        };
        // A node feeding an input goes to the left of the drop point.
        if pending.is_some_and(|p| {
            world
                .get::<crate::Port>(p)
                .is_some_and(|p| p.direction == PortDirection::Input)
        }) {
            position.x -= 200.0;
        }
        let node = {
            let mut commands = world.commands();
            (template.spawn)(&mut commands, content, position)
        };
        world.flush();
        if let Some(pending) = pending
            && let Some(pending_info) = PortInfo::from_world(world, pending)
        {
            let target = descendants(world, node).into_iter().find(|port| {
                PortInfo::from_world(world, *port)
                    .is_some_and(|info| check_connection(&pending_info, &info, canvas).is_ok())
            });
            if let Some(target) = target {
                let _ = world.graph_edit_with_origin(
                    canvas,
                    GraphEdit::Connect {
                        from: pending,
                        to: target,
                    },
                    EditOrigin::Interaction,
                );
            }
        }
        let _ = world.graph_edit_with_origin(
            canvas,
            GraphEdit::Select {
                nodes: vec![node],
                mode: crate::SelectMode::Replace,
            },
            EditOrigin::Interaction,
        );
    });
}

fn scroll_list(mut scroll: On<Pointer<Scroll>>, mut positions: Query<&mut ScrollPosition>) {
    scroll.propagate(false);
    if let Ok(mut position) = positions.get_mut(scroll.event_target()) {
        let step = match scroll.unit {
            MouseScrollUnit::Line => 24.0,
            MouseScrollUnit::Pixel => 1.0,
        };
        position.y = (position.y - scroll.y * step).max(0.0);
    }
}
