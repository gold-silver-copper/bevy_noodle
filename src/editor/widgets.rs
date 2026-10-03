//! Inline value widgets and small buttons inside nodes.

use bevy::ecs::hierarchy::ChildSpawnerCommands;
use bevy::input_focus::InputFocus;
use bevy::prelude::*;
use bevy::text::{EditableText, EditableTextFilter, LineBreak, TextCursorStyle};

use super::NodeGraphEditor;
use super::input::delete_node;
use super::view::DELETE_ICON_SIZE;
use crate::graph::{InputId, NodeId};
use crate::render::WireMaterial;
use crate::state::{NodeGraphResponse, NodeResponse};
use crate::style::NodeGraphStyle;
use crate::traits::{
    GraphOf, NodeDataTrait, NodeGraphSchema, NumberField, ValueEdit, ValueWidget, WidgetValueTrait,
};

/// A text or number field bound to an input value.
#[derive(Component, Clone, Copy, Debug)]
pub(crate) struct ValueField {
    pub editor: Entity,
    pub input: InputId,
    pub component: usize,
}

/// A label that scrubs a number value when dragged sideways.
#[derive(Component, Clone, Copy, Debug)]
pub(crate) struct ValueScrub {
    pub editor: Entity,
    pub input: InputId,
    pub component: usize,
    accumulated: f64,
}

#[derive(Component, Clone, Copy, Debug)]
pub(crate) struct ValueToggle {
    pub editor: Entity,
    pub input: InputId,
}

#[derive(Component, Clone, Copy, Debug)]
pub(crate) struct ValueToggleMark {
    pub editor: Entity,
    pub input: InputId,
}

#[derive(Component, Clone, Copy, Debug)]
pub(crate) struct ValueChoiceStep {
    pub editor: Entity,
    pub input: InputId,
    pub step: i32,
}

#[derive(Component, Clone, Copy, Debug)]
pub(crate) struct ValueChoiceLabel {
    pub editor: Entity,
    pub input: InputId,
}

#[derive(Component, Clone, Copy, Debug)]
pub(crate) struct ValueLabel {
    pub editor: Entity,
    pub input: InputId,
}

#[derive(Component, Clone, Copy, Debug)]
pub(crate) struct DeleteNodeButton {
    pub editor: Entity,
    pub node: NodeId,
}

/// Swaps the background color while hovered. Requires [`Interaction`]
/// (e.g. via [`Button`]).
#[derive(Component, Clone, Copy, Debug)]
pub(crate) struct HoverHighlight {
    pub normal: Color,
    pub hovered: Color,
}

pub(crate) fn update_hover_highlights(
    mut buttons: Query<(&Interaction, &HoverHighlight, &mut BackgroundColor), Changed<Interaction>>,
) {
    for (interaction, highlight, mut background) in &mut buttons {
        background.0 = match interaction {
            Interaction::None => highlight.normal,
            Interaction::Hovered | Interaction::Pressed => highlight.hovered,
        };
    }
}

/// The widget currently describing `input`'s value.
pub(crate) fn input_widget<N: NodeDataTrait>(
    graph: &GraphOf<N>,
    input: InputId,
) -> Option<ValueWidget> {
    let param = graph.try_get_input(input)?;
    let name = graph.nodes.get(param.node)?.input_name(input)?;
    Some(param.value.value_widget(name))
}

fn number_field(widget: &ValueWidget, component: usize) -> Option<&NumberField> {
    match widget {
        ValueWidget::Number(field) => Some(field),
        ValueWidget::Numbers(fields) => fields.get(component),
        _ => None,
    }
}

/// Text shown by a field when it is not being edited.
fn field_display(widget: &ValueWidget, component: usize) -> Option<String> {
    match widget {
        ValueWidget::Text { value, .. } => Some(value.clone()),
        _ => number_field(widget, component).map(NumberField::format),
    }
}

/// Applies an edit to an input's value and reports it.
pub(crate) fn apply_value_edit<S: NodeGraphSchema>(
    editor_entity: Entity,
    editor: &mut NodeGraphEditor<S>,
    input: InputId,
    edit: ValueEdit,
    responses: &mut MessageWriter<NodeGraphResponse<S>>,
) {
    let Some(param) = editor.state.graph.inputs.get_mut(input) else {
        return;
    };
    param.value.apply_edit(edit);
    let node_id = param.node;
    responses.write(NodeGraphResponse {
        editor: editor_entity,
        response: NodeResponse::ValueChanged {
            node_id,
            input_id: input,
        },
    });
}

// ---------------------------------------------------------------------------
// Spawning
// ---------------------------------------------------------------------------

/// The parameter name. For a number widget it doubles as a scrub handle.
pub(crate) fn spawn_param_label<S: NodeGraphSchema>(
    row: &mut ChildSpawnerCommands,
    editor: Entity,
    input: InputId,
    name: &str,
    widget: &ValueWidget,
    show_widget: bool,
    style: &NodeGraphStyle,
) {
    let mut label = row.spawn((
        Text::new(name),
        style.text_font(style.font_size),
        TextColor(if show_widget {
            style.text_muted
        } else {
            style.text
        }),
    ));
    if show_widget && matches!(widget, ValueWidget::Number(_)) {
        add_scrub::<S>(&mut label, editor, input, 0);
    } else {
        label.insert(Pickable::IGNORE);
    }
}

fn add_scrub<S: NodeGraphSchema>(
    entity: &mut EntityCommands,
    editor: Entity,
    input: InputId,
    component: usize,
) {
    entity
        .insert(ValueScrub {
            editor,
            input,
            component,
            accumulated: 0.0,
        })
        .observe(stop_primary_press)
        .observe(on_scrub_start::<S>)
        .observe(on_scrub_drag::<S>);
}

pub(crate) fn spawn_value_widget<S: NodeGraphSchema>(
    row: &mut ChildSpawnerCommands,
    editor: Entity,
    input: InputId,
    widget: &ValueWidget,
    style: &NodeGraphStyle,
) {
    match widget {
        ValueWidget::None => {}
        ValueWidget::Label(text) => {
            row.spawn((
                ValueLabel { editor, input },
                Text::new(text.clone()),
                style.text_font(style.font_size),
                TextColor(style.text),
                Pickable::IGNORE,
            ));
        }
        ValueWidget::Text { value, multiline } => {
            spawn_text_field(row, editor, input, 0, value, *multiline, false, style);
        }
        ValueWidget::Number(field) => {
            spawn_text_field(row, editor, input, 0, &field.format(), false, true, style);
        }
        ValueWidget::Numbers(fields) => {
            row.spawn(Node {
                flex_grow: 1.0,
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: px(4),
                ..default()
            })
            .with_children(|group| {
                for (component, field) in fields.iter().enumerate() {
                    if let Some(label) = &field.label {
                        let mut label = group.spawn((
                            Text::new(label.clone()),
                            style.text_font(style.font_size),
                            TextColor(style.text_muted),
                        ));
                        add_scrub::<S>(&mut label, editor, input, component);
                    }
                    spawn_text_field(
                        group,
                        editor,
                        input,
                        component,
                        &field.format(),
                        false,
                        true,
                        style,
                    );
                }
            });
        }
        ValueWidget::Bool(value) => {
            row.spawn((
                ValueToggle { editor, input },
                Button,
                Node {
                    width: px(16),
                    height: px(16),
                    border: UiRect::all(px(1)),
                    border_radius: BorderRadius::all(px(4)),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    ..default()
                },
                BackgroundColor(style.widget_background),
                BorderColor::all(style.widget_border),
                HoverHighlight {
                    normal: style.widget_background,
                    hovered: style.button_hovered,
                },
            ))
            .observe(stop_primary_press)
            .observe(on_toggle_click::<S>)
            .with_child((
                ValueToggleMark { editor, input },
                Node {
                    width: px(8),
                    height: px(8),
                    border_radius: BorderRadius::all(px(2)),
                    ..default()
                },
                BackgroundColor(style.accent),
                if *value {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                },
                Pickable::IGNORE,
            ));
        }
        ValueWidget::Choice { options, selected } => {
            let label = options.get(*selected).cloned().unwrap_or_default();
            row.spawn(Node {
                flex_grow: 1.0,
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: px(4),
                ..default()
            })
            .with_children(|group| {
                spawn_choice_step::<S>(group, editor, input, -1, style);
                group.spawn((
                    ValueChoiceLabel { editor, input },
                    Text::new(label),
                    style.text_font(style.font_size),
                    TextColor(style.text),
                    TextLayout::justify(Justify::Center),
                    Node {
                        flex_grow: 1.0,
                        ..default()
                    },
                    Pickable::IGNORE,
                ));
                spawn_choice_step::<S>(group, editor, input, 1, style);
            });
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn spawn_text_field(
    parent: &mut ChildSpawnerCommands,
    editor: Entity,
    input: InputId,
    component: usize,
    initial: &str,
    multiline: bool,
    numeric: bool,
    style: &NodeGraphStyle,
) {
    let mut text = EditableText::new(initial);
    text.allow_newlines = multiline;
    text.visible_lines = Some(if multiline { 3.0 } else { 1.0 });
    text.visible_width = Some(if numeric { 6.0 } else { 12.0 });
    text.cursor_width = 0.15;

    let mut field = parent.spawn((
        ValueField {
            editor,
            input,
            component,
        },
        Node {
            flex_grow: 1.0,
            min_width: px(40),
            padding: UiRect::axes(px(6), px(3)),
            border: UiRect::all(px(1)),
            border_radius: BorderRadius::all(px(4)),
            ..default()
        },
        text,
        TextLayout::linebreak(if multiline {
            LineBreak::WordBoundary
        } else {
            LineBreak::NoWrap
        }),
        style.text_font(style.font_size),
        TextColor(style.text),
        TextCursorStyle {
            color: style.text,
            selection_color: style.accent.with_alpha(0.45),
            unfocused_selection_color: style.accent.with_alpha(0.2),
            ..default()
        },
        BackgroundColor(style.widget_background),
        BorderColor::all(style.widget_border),
    ));
    if numeric {
        field.insert(EditableTextFilter::new(|c| {
            c.is_ascii_digit() || matches!(c, '.' | '-' | '+' | 'e' | 'E')
        }));
    }
}

fn spawn_choice_step<S: NodeGraphSchema>(
    parent: &mut ChildSpawnerCommands,
    editor: Entity,
    input: InputId,
    step: i32,
    style: &NodeGraphStyle,
) {
    parent
        .spawn((
            ValueChoiceStep {
                editor,
                input,
                step,
            },
            Button,
            Node {
                width: px(18),
                height: px(18),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border_radius: BorderRadius::all(px(4)),
                ..default()
            },
            BackgroundColor(style.widget_background),
            HoverHighlight {
                normal: style.widget_background,
                hovered: style.button_hovered,
            },
        ))
        .observe(stop_primary_press)
        .observe(on_choice_click::<S>)
        .with_child((
            Text::new(if step < 0 { "<" } else { ">" }),
            style.text_font(style.font_size),
            TextColor(style.text),
            Pickable::IGNORE,
        ));
}

/// The "x" button in a node's title bar.
pub(crate) fn spawn_delete_button<S: NodeGraphSchema>(
    bar: &mut ChildSpawnerCommands,
    editor: Entity,
    node: NodeId,
    style: &NodeGraphStyle,
    cross: &Handle<WireMaterial>,
) {
    const SIZE: f32 = 18.0;
    let inset = (SIZE - DELETE_ICON_SIZE) * 0.5;
    bar.spawn((
        DeleteNodeButton { editor, node },
        Button,
        Node {
            width: px(SIZE),
            height: px(SIZE),
            border_radius: BorderRadius::all(px(4)),
            ..default()
        },
        BackgroundColor(Color::NONE),
        HoverHighlight {
            normal: Color::NONE,
            hovered: style.button_hovered,
        },
    ))
    .observe(stop_primary_press)
    .observe(on_delete_click::<S>)
    .with_child((
        Node {
            position_type: PositionType::Absolute,
            left: px(inset),
            top: px(inset),
            width: px(DELETE_ICON_SIZE),
            height: px(DELETE_ICON_SIZE),
            ..default()
        },
        MaterialNode(cross.clone()),
        Pickable::IGNORE,
    ));
}

// ---------------------------------------------------------------------------
// Observers
// ---------------------------------------------------------------------------

/// Keeps a press on a widget from selecting or dragging the node, and takes
/// keyboard focus away from any text field.
fn stop_primary_press(mut press: On<Pointer<Press>>, mut focus: ResMut<InputFocus>) {
    if press.button == PointerButton::Primary {
        press.propagate(false);
        if focus.get().is_some() {
            focus.clear();
        }
    }
}

fn on_delete_click<S: NodeGraphSchema>(
    mut click: On<Pointer<Click>>,
    buttons: Query<&DeleteNodeButton>,
    mut editors: Query<&mut NodeGraphEditor<S>>,
    mut responses: MessageWriter<NodeGraphResponse<S>>,
) {
    if click.button != PointerButton::Primary {
        return;
    }
    click.propagate(false);
    let Ok(button) = buttons.get(click.event_target()) else {
        return;
    };
    if let Ok(mut editor) = editors.get_mut(button.editor) {
        delete_node(button.editor, &mut editor, button.node, &mut responses);
    }
}

fn on_toggle_click<S: NodeGraphSchema>(
    mut click: On<Pointer<Click>>,
    toggles: Query<&ValueToggle>,
    mut editors: Query<&mut NodeGraphEditor<S>>,
    mut responses: MessageWriter<NodeGraphResponse<S>>,
) {
    if click.button != PointerButton::Primary {
        return;
    }
    click.propagate(false);
    let Ok(toggle) = toggles.get(click.event_target()) else {
        return;
    };
    let Ok(mut editor) = editors.get_mut(toggle.editor) else {
        return;
    };
    if let Some(ValueWidget::Bool(value)) = input_widget(&editor.state.graph, toggle.input) {
        apply_value_edit(
            toggle.editor,
            &mut editor,
            toggle.input,
            ValueEdit::Bool(!value),
            &mut responses,
        );
    }
}

fn on_choice_click<S: NodeGraphSchema>(
    mut click: On<Pointer<Click>>,
    steps: Query<&ValueChoiceStep>,
    mut editors: Query<&mut NodeGraphEditor<S>>,
    mut responses: MessageWriter<NodeGraphResponse<S>>,
) {
    if click.button != PointerButton::Primary {
        return;
    }
    click.propagate(false);
    let Ok(step) = steps.get(click.event_target()) else {
        return;
    };
    let Ok(mut editor) = editors.get_mut(step.editor) else {
        return;
    };
    if let Some(ValueWidget::Choice { options, selected }) =
        input_widget(&editor.state.graph, step.input)
        && !options.is_empty()
    {
        let next = (selected as i32 + step.step).rem_euclid(options.len() as i32) as usize;
        apply_value_edit(
            step.editor,
            &mut editor,
            step.input,
            ValueEdit::Choice(next),
            &mut responses,
        );
    }
}

fn on_scrub_start<S: NodeGraphSchema>(
    mut drag: On<Pointer<DragStart>>,
    mut scrubs: Query<&mut ValueScrub>,
    editors: Query<&NodeGraphEditor<S>>,
) {
    if drag.button != PointerButton::Primary {
        return;
    }
    drag.propagate(false);
    let Ok(mut scrub) = scrubs.get_mut(drag.event_target()) else {
        return;
    };
    let Ok(editor) = editors.get(scrub.editor) else {
        return;
    };
    if let Some(widget) = input_widget(&editor.state.graph, scrub.input)
        && let Some(field) = number_field(&widget, scrub.component)
    {
        scrub.accumulated = field.value;
    }
}

fn on_scrub_drag<S: NodeGraphSchema>(
    mut drag: On<Pointer<Drag>>,
    mut scrubs: Query<&mut ValueScrub>,
    mut editors: Query<&mut NodeGraphEditor<S>>,
    mut responses: MessageWriter<NodeGraphResponse<S>>,
) {
    if drag.button != PointerButton::Primary {
        return;
    }
    drag.propagate(false);
    let Ok(mut scrub) = scrubs.get_mut(drag.event_target()) else {
        return;
    };
    let Ok(mut editor) = editors.get_mut(scrub.editor) else {
        return;
    };
    let Some(widget) = input_widget(&editor.state.graph, scrub.input) else {
        return;
    };
    let Some(field) = number_field(&widget, scrub.component) else {
        return;
    };
    scrub.accumulated += drag.delta.x as f64 * field.speed;
    let value = field.sanitize(scrub.accumulated);
    if field.differs_from(value) {
        let edit = ValueEdit::Number {
            component: scrub.component,
            value,
        };
        apply_value_edit(scrub.editor, &mut editor, scrub.input, edit, &mut responses);
    }
}

// ---------------------------------------------------------------------------
// Systems
// ---------------------------------------------------------------------------

/// Pushes what the user types into the focused field into the graph.
pub(crate) fn commit_focused_field<S: NodeGraphSchema>(
    focus: Res<InputFocus>,
    fields: Query<(&ValueField, &EditableText)>,
    mut editors: Query<&mut NodeGraphEditor<S>>,
    mut responses: MessageWriter<NodeGraphResponse<S>>,
) {
    let Some((field, text)) = focus.get().and_then(|entity| fields.get(entity).ok()) else {
        return;
    };
    let Ok(mut editor) = editors.get_mut(field.editor) else {
        return;
    };
    let Some(widget) = input_widget(&editor.state.graph, field.input) else {
        return;
    };
    let typed = text.value().to_string();
    let edit = match &widget {
        ValueWidget::Text { value, .. } => (typed != *value).then_some(ValueEdit::Text(typed)),
        _ => number_field(&widget, field.component).and_then(|number| {
            let parsed = typed
                .trim()
                .parse::<f64>()
                .ok()
                .filter(|value| value.is_finite())?;
            let value = number.sanitize(parsed);
            number.differs_from(value).then_some(ValueEdit::Number {
                component: field.component,
                value,
            })
        }),
    };
    if let Some(edit) = edit {
        apply_value_edit(field.editor, &mut editor, field.input, edit, &mut responses);
    }
}

/// Keeps widgets showing the current values (for changes made by code, by
/// scrubbing, or after a field loses focus).
#[allow(clippy::type_complexity)]
pub(crate) fn sync_widget_values<S: NodeGraphSchema>(
    editors: Query<(&NodeGraphEditor<S>, &NodeGraphStyle)>,
    focus: Res<InputFocus>,
    mut fields: Query<(Entity, &ValueField, &mut EditableText, &mut BorderColor)>,
    mut marks: Query<(&ValueToggleMark, &mut Visibility)>,
    mut choices: Query<(&ValueChoiceLabel, &mut Text), Without<ValueLabel>>,
    mut labels: Query<(&ValueLabel, &mut Text), Without<ValueChoiceLabel>>,
) {
    for (entity, field, mut text, mut border) in &mut fields {
        let Ok((editor, style)) = editors.get(field.editor) else {
            continue;
        };
        let focused = focus.get() == Some(entity);
        let border_color = if focused {
            style.widget_border_focused
        } else {
            style.widget_border
        };
        if border.top != border_color {
            *border = BorderColor::all(border_color);
        }
        if focused {
            continue;
        }
        let Some(display) = input_widget(&editor.state.graph, field.input)
            .and_then(|widget| field_display(&widget, field.component))
        else {
            continue;
        };
        if text.value().to_string() != display {
            text.editor_mut().set_text(&display);
        }
    }

    for (mark, mut visibility) in &mut marks {
        let Ok((editor, _)) = editors.get(mark.editor) else {
            continue;
        };
        if let Some(ValueWidget::Bool(value)) = input_widget(&editor.state.graph, mark.input) {
            let wanted = if value {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            };
            if *visibility != wanted {
                *visibility = wanted;
            }
        }
    }

    for (choice, mut text) in &mut choices {
        let Ok((editor, _)) = editors.get(choice.editor) else {
            continue;
        };
        if let Some(ValueWidget::Choice { options, selected }) =
            input_widget(&editor.state.graph, choice.input)
        {
            let wanted = options.get(selected).cloned().unwrap_or_default();
            if text.0 != wanted {
                text.0 = wanted;
            }
        }
    }

    for (label, mut text) in &mut labels {
        let Ok((editor, _)) = editors.get(label.editor) else {
            continue;
        };
        if let Some(ValueWidget::Label(wanted)) = input_widget(&editor.state.graph, label.input)
            && text.0 != wanted
        {
            text.0 = wanted;
        }
    }
}
