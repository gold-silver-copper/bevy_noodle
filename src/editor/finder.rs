//! The node finder: a searchable popup listing the node templates.
//!
//! Opened by right-clicking the canvas, or by dropping a wire on empty canvas
//! (then only templates that can take the wire are listed, and the new node
//! gets connected).

use bevy::input::ButtonState;
use bevy::input::keyboard::KeyboardInput;
use bevy::input::mouse::MouseScrollUnit;
use bevy::input_focus::{FocusCause, InputFocus};
use bevy::prelude::*;
use bevy::text::{EditableText, LineBreak, TextCursorStyle};

use super::input::connect;
use super::widgets::HoverHighlight;
use super::{FinderView, NodeGraphEditor};
use crate::graph::{AnyParameterId, InputParamKind, NodeId};
use crate::state::{NodeGraphResponse, NodeResponse};
use crate::style::NodeGraphStyle;
use crate::traits::{DataTypeTrait, NodeGraphSchema, NodeTemplateTrait, SchemaGraph};

const FINDER_WIDTH: f32 = 230.0;
const FINDER_LIST_HEIGHT: f32 = 300.0;

#[derive(Component, Clone, Copy, Debug)]
pub(crate) struct FinderSearchField;

#[derive(Component, Clone, Copy, Debug)]
pub(crate) struct FinderItem {
    pub editor: Entity,
    pub template: usize,
}

enum FinderEntry {
    Header(String),
    Item(usize),
}

/// Whether a node built from `template` has a port that accepts the wire `pending`.
fn template_accepts<S: NodeGraphSchema>(
    graph: &SchemaGraph<S>,
    template: &S::NodeTemplate,
    pending: AnyParameterId,
) -> bool {
    let mut scratch = SchemaGraph::<S>::default();
    let node = scratch.add_node(String::new(), template.user_data(), |graph, node| {
        template.build_node(graph, node)
    });
    match pending {
        AnyParameterId::Output(output) => {
            let Some(output) = graph.try_get_output(output) else {
                return false;
            };
            scratch[node].input_ids().any(|input| {
                let input = scratch.get_input(input);
                input.kind != InputParamKind::ConstantOnly
                    && output.typ.is_compatible_with(&input.typ)
            })
        }
        AnyParameterId::Input(input) => {
            let Some(input) = graph.try_get_input(input) else {
                return false;
            };
            scratch[node].output_ids().any(|output| {
                scratch
                    .get_output(output)
                    .typ
                    .is_compatible_with(&input.typ)
            })
        }
    }
}

/// The finder rows: matching templates, grouped by category when not searching.
fn finder_entries<S: NodeGraphSchema>(editor: &NodeGraphEditor<S>) -> Vec<FinderEntry> {
    let Some(finder) = &editor.ui.finder else {
        return Vec::new();
    };
    let search = finder.search.trim().to_lowercase();
    let matches: Vec<usize> = editor
        .templates
        .iter()
        .enumerate()
        .filter(|(_, template)| {
            search.is_empty()
                || template
                    .node_finder_label()
                    .to_lowercase()
                    .contains(&search)
                || template
                    .node_finder_categories()
                    .iter()
                    .any(|category| category.to_lowercase().contains(&search))
        })
        .filter(|(_, template)| {
            finder
                .pending
                .is_none_or(|pending| template_accepts::<S>(&editor.state.graph, template, pending))
        })
        .map(|(index, _)| index)
        .collect();

    let categorized = editor
        .templates
        .iter()
        .any(|template| !template.node_finder_categories().is_empty());
    if !search.is_empty() || !categorized {
        return matches.into_iter().map(FinderEntry::Item).collect();
    }

    let mut groups: Vec<(String, Vec<usize>)> = Vec::new();
    for index in matches {
        let category = editor.templates[index]
            .node_finder_categories()
            .first()
            .map(|category| category.to_string())
            .unwrap_or_else(|| "Other".into());
        match groups.iter_mut().find(|(name, _)| *name == category) {
            Some((_, items)) => items.push(index),
            None => groups.push((category, vec![index])),
        }
    }
    groups
        .into_iter()
        .flat_map(|(category, items)| {
            std::iter::once(FinderEntry::Header(category))
                .chain(items.into_iter().map(FinderEntry::Item))
        })
        .collect()
}

/// Creates a node from the finder, connecting it to the pending wire if any.
fn create_from_finder<S: NodeGraphSchema>(
    editor_entity: Entity,
    editor: &mut NodeGraphEditor<S>,
    template: usize,
    responses: &mut MessageWriter<NodeGraphResponse<S>>,
) -> Option<NodeId> {
    let finder = editor.ui.finder.take()?;
    let template = editor.templates.get(template)?.clone();
    let mut position = finder.world;
    if matches!(finder.pending, Some(AnyParameterId::Input(_))) {
        // The new node feeds the wire: put it to the left of the pointer.
        position.x -= 200.0;
    }
    let node_id = editor.state.add_node(&template, position);
    editor.state.select_only(node_id);
    responses.write(NodeGraphResponse {
        editor: editor_entity,
        response: NodeResponse::CreatedNode(node_id),
    });

    match finder.pending {
        Some(AnyParameterId::Output(output)) => {
            let inputs: Vec<_> = editor.state.graph[node_id].input_ids().collect();
            if let Some(input) = inputs
                .into_iter()
                .find(|input| editor.state.can_connect(output, *input))
            {
                connect(editor_entity, editor, output, input, responses);
            }
        }
        Some(AnyParameterId::Input(input)) => {
            let outputs: Vec<_> = editor.state.graph[node_id].output_ids().collect();
            if let Some(output) = outputs
                .into_iter()
                .find(|output| editor.state.can_connect(*output, input))
            {
                connect(editor_entity, editor, output, input, responses);
            }
        }
        None => {}
    }
    Some(node_id)
}

/// Reads the search field and handles Enter (create the first match) and
/// Escape (close).
pub(crate) fn handle_finder_input<S: NodeGraphSchema>(
    mut key_events: MessageReader<KeyboardInput>,
    mut focus: ResMut<InputFocus>,
    search_fields: Query<&EditableText, With<FinderSearchField>>,
    mut editors: Query<(Entity, &mut NodeGraphEditor<S>)>,
    mut responses: MessageWriter<NodeGraphResponse<S>>,
) {
    let pressed: Vec<KeyCode> = key_events
        .read()
        .filter(|event| event.state == ButtonState::Pressed)
        .map(|event| event.key_code)
        .collect();

    for (editor_entity, mut editor) in &mut editors {
        let editor = &mut *editor;
        let Some(view) = &editor.ui.finder_view else {
            continue;
        };
        let search_entity = view.search;
        if let (Some(finder), Ok(text)) = (&mut editor.ui.finder, search_fields.get(search_entity))
        {
            let search = text.value().to_string();
            if finder.search != search {
                finder.search = search;
            }
        }
        if focus.get() != Some(search_entity) {
            continue;
        }
        for key in &pressed {
            match key {
                KeyCode::Escape => {
                    editor.ui.finder = None;
                    focus.clear();
                }
                KeyCode::Enter | KeyCode::NumpadEnter => {
                    let first = finder_entries(editor)
                        .into_iter()
                        .find_map(|entry| match entry {
                            FinderEntry::Item(index) => Some(index),
                            FinderEntry::Header(_) => None,
                        });
                    if let Some(index) = first {
                        create_from_finder(editor_entity, editor, index, &mut responses);
                        focus.clear();
                    }
                }
                _ => {}
            }
        }
    }
}

/// Spawns, refreshes and removes the finder popup.
pub(crate) fn sync_finder<S: NodeGraphSchema>(
    mut commands: Commands,
    mut editors: Query<(Entity, &mut NodeGraphEditor<S>, &NodeGraphStyle)>,
    mut focus: ResMut<InputFocus>,
) {
    for (editor_entity, mut editor, style) in &mut editors {
        let editor = &mut *editor;

        let close = editor.ui.finder.is_none() || std::mem::take(&mut editor.ui.finder_view_reset);
        if close && let Some(view) = editor.ui.finder_view.take() {
            commands.entity(view.root).despawn();
            if focus.get() == Some(view.search) {
                focus.clear();
            }
        }
        let Some(finder) = &editor.ui.finder else {
            continue;
        };

        if editor.ui.finder_view.is_none() {
            let canvas = editor.ui.canvas_size;
            let left = finder.screen.x.min(canvas.x - FINDER_WIDTH - 8.0).max(4.0);
            let top = finder
                .screen
                .y
                .min(canvas.y - FINDER_LIST_HEIGHT - 60.0)
                .max(4.0);
            let view = spawn_finder(&mut commands, editor_entity, Vec2::new(left, top), style);
            focus.set(view.search, FocusCause::Navigated);
            editor.ui.finder_view = Some(view);
        }

        let search = finder.search.clone();
        let needs_rebuild = editor
            .ui
            .finder_view
            .as_ref()
            .is_some_and(|view| view.built_search.as_deref() != Some(search.as_str()));
        if !needs_rebuild {
            continue;
        }
        let entries = finder_entries(editor);
        let view = editor.ui.finder_view.as_mut().expect("spawned above");
        view.built_search = Some(search);
        commands.entity(view.list).despawn_children();
        let templates = &editor.templates;
        commands.entity(view.list).with_children(|list| {
            if entries.is_empty() {
                list.spawn((
                    Text::new("No matching nodes"),
                    style.text_font(style.font_size),
                    TextColor(style.text_muted),
                    Node {
                        padding: UiRect::axes(px(8), px(4)),
                        ..default()
                    },
                    Pickable::IGNORE,
                ));
            }
            let mut first = true;
            for entry in entries {
                match entry {
                    FinderEntry::Header(category) => {
                        list.spawn((
                            Text::new(category),
                            style.text_font(style.font_size - 2.0),
                            TextColor(style.text_muted),
                            Node {
                                padding: UiRect::new(px(6), px(6), px(6), px(2)),
                                ..default()
                            },
                            Pickable::IGNORE,
                        ));
                    }
                    FinderEntry::Item(index) => {
                        list.spawn((
                            FinderItem {
                                editor: editor_entity,
                                template: index,
                            },
                            Button,
                            Node {
                                padding: UiRect::axes(px(8), px(4)),
                                border: UiRect::all(px(1)),
                                border_radius: BorderRadius::all(px(4)),
                                ..default()
                            },
                            BackgroundColor(Color::NONE),
                            BorderColor::all(if first {
                                style.accent.with_alpha(0.6)
                            } else {
                                Color::NONE
                            }),
                            HoverHighlight {
                                normal: Color::NONE,
                                hovered: style.button_hovered,
                            },
                        ))
                        .observe(on_finder_item_click::<S>)
                        .with_child((
                            Text::new(templates[index].node_finder_label().into_owned()),
                            style.text_font(style.font_size),
                            TextColor(style.text),
                            Pickable::IGNORE,
                        ));
                        first = false;
                    }
                }
            }
        });
    }
}

fn spawn_finder(
    commands: &mut Commands,
    editor: Entity,
    position: Vec2,
    style: &NodeGraphStyle,
) -> FinderView {
    let mut search_text = EditableText::new("");
    search_text.cursor_width = 0.15;
    let search = commands
        .spawn((
            FinderSearchField,
            Node {
                padding: UiRect::axes(px(6), px(4)),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(4)),
                ..default()
            },
            search_text,
            TextLayout::linebreak(LineBreak::NoWrap),
            style.text_font(style.font_size),
            TextColor(style.text),
            TextCursorStyle {
                color: style.text,
                selection_color: style.accent.with_alpha(0.45),
                ..default()
            },
            BackgroundColor(style.widget_background),
            BorderColor::all(style.widget_border_focused),
        ))
        .id();
    let list = commands
        .spawn((
            Node {
                flex_direction: FlexDirection::Column,
                max_height: px(FINDER_LIST_HEIGHT),
                overflow: Overflow::scroll_y(),
                row_gap: px(1),
                ..default()
            },
            ScrollPosition::default(),
        ))
        .observe(on_finder_list_scroll)
        .id();
    let root = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(position.x),
                top: px(position.y),
                width: px(FINDER_WIDTH),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(6)),
                row_gap: px(6),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(8)),
                ..default()
            },
            BackgroundColor(style.popup_background),
            BorderColor::all(style.popup_border),
            BoxShadow::new(style.node_shadow, px(0), px(6), px(0), px(18)),
            ChildOf(editor),
        ))
        .add_children(&[search, list])
        .observe(|mut press: On<Pointer<Press>>| press.propagate(false))
        .observe(|mut click: On<Pointer<Click>>| click.propagate(false))
        .observe(|mut drag: On<Pointer<DragStart>>| drag.propagate(false))
        .observe(|mut drag: On<Pointer<Drag>>| drag.propagate(false))
        .observe(|mut scroll: On<Pointer<Scroll>>| scroll.propagate(false))
        .id();
    FinderView {
        root,
        search,
        list,
        built_search: None,
    }
}

fn on_finder_list_scroll(
    mut scroll: On<Pointer<Scroll>>,
    mut positions: Query<&mut ScrollPosition>,
) {
    scroll.propagate(false);
    let Ok(mut position) = positions.get_mut(scroll.event_target()) else {
        return;
    };
    let step = match scroll.unit {
        MouseScrollUnit::Line => 24.0,
        MouseScrollUnit::Pixel => 1.0,
    };
    position.y = (position.y - scroll.y * step).max(0.0);
}

fn on_finder_item_click<S: NodeGraphSchema>(
    mut click: On<Pointer<Click>>,
    items: Query<&FinderItem>,
    mut editors: Query<&mut NodeGraphEditor<S>>,
    mut focus: ResMut<InputFocus>,
    mut responses: MessageWriter<NodeGraphResponse<S>>,
) {
    if click.button != PointerButton::Primary {
        return;
    }
    click.propagate(false);
    let Ok(item) = items.get(click.event_target()) else {
        return;
    };
    let Ok(mut editor) = editors.get_mut(item.editor) else {
        return;
    };
    if create_from_finder(item.editor, &mut editor, item.template, &mut responses).is_some() {
        focus.clear();
    }
}
