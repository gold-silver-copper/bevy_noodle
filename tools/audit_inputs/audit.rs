//! Text-field audit harness. `run.sh` copies an example, swaps `App::new()`
//! for `audit::app()`, and runs it in a real window. The harness presses each
//! text field (and the padding and label around it) with a synthetic pointer,
//! at several zoom levels, then checks the result and saves screenshots.
#![allow(dead_code)]

use bevy::asset::uuid::Uuid;
use bevy::camera::RenderTarget;
use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input_focus::InputFocus;
use bevy::picking::pointer::{Location, PointerAction, PointerButton, PointerId, PointerInput};
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use bevy::text::{EditableText, TextCursorStyle, TextLayoutInfo};
use bevy::ui::widget::TextScroll;
use bevy::ui::{ComputedNode, Selected, UiGlobalTransform};
use bevy::window::{PrimaryWindow, WindowRef};
use bevy_noodle::prelude::*;
use std::io::Write as _;

const POINTER: PointerId = PointerId::Custom(Uuid::from_u128(0xa0d17));

pub fn app() -> App {
    let mut app = App::new();
    app.add_systems(Startup, |mut commands: Commands| {
        commands.spawn(POINTER);
    })
    .add_systems(Last, drive);
    app
}

#[derive(Clone, Copy, Debug)]
enum Kind {
    /// Inside the field, at `x` (0..1 of its content width).
    Text(f32),
    /// Inside the field's parent but outside the field (feathers container padding).
    Padding,
    /// A sibling of the field without `EditableText` (a feathers number label).
    Label,
}

#[derive(Clone, Copy)]
struct Probe {
    field: Entity,
    kind: Kind,
    zoom: f32,
}

#[derive(Default)]
struct State {
    frame: u32,
    probes: Vec<Probe>,
    index: usize,
    step: u32,
    at: Option<Vec2>,
    before: Option<(Option<Vec2>, bool, String)>,
    fails: u32,
    log: Vec<String>,
}

fn name() -> String {
    std::env::var("AUDIT_NAME").unwrap_or_else(|_| "example".into())
}

fn out_dir() -> String {
    std::env::var("AUDIT_OUT").unwrap_or_else(|_| ".".into())
}

fn drive(world: &mut World, mut state: Local<State>) {
    state.frame += 1;
    // Let assets load and layout settle.
    if state.frame < 90 {
        return;
    }
    if state.frame == 90 {
        let mut q = world.query::<(Entity, &EditableText, &InheritedVisibility, &ComputedNode)>();
        let fields: Vec<Entity> = q
            .iter(world)
            .filter(|(_, _, v, n)| v.get() && !n.is_empty())
            .map(|(e, ..)| e)
            .collect();
        let line = format!("{}: {} visible text fields", name(), fields.len());
        state.log.push(line);
        for &field in &fields {
            for kind in [Kind::Text(0.3), Kind::Text(1.5), Kind::Padding, Kind::Label] {
                state.probes.push(Probe {
                    field,
                    kind,
                    zoom: 1.0,
                });
            }
            for (zoom, x) in [(0.6, 0.5), (1.7, 0.2), (2.5, 0.6)] {
                state.probes.push(Probe {
                    field,
                    kind: Kind::Text(x),
                    zoom,
                });
            }
        }
        if fields.is_empty() {
            finish(world, &mut state);
        }
        return;
    }
    if state.index >= state.probes.len() {
        if state.step == 0 {
            finish(world, &mut state);
        }
        state.step += 1;
        if state.step > 30 {
            world.write_message(AppExit::Success);
        }
        return;
    }
    let probe = state.probes[state.index];
    if world.get_entity(probe.field).is_err() {
        state.index += 1;
        return;
    }
    let step = state.step;
    state.step += 1;
    match step {
        0 => {
            set_zoom(world, probe.zoom);
            world.resource_mut::<InputFocus>().clear();
            world.entity_mut(probe.field).remove::<Selected>();
            let node = node_of(world, probe.field);
            if let Some(node) = node {
                world.entity_mut(node).remove::<Selected>();
            }
        }
        6 => {
            let at = probe_point(world, probe);
            state.at = at;
            let Some(at) = at else {
                let line = format!(
                    "  skip  {} {:?} zoom {}: no point to press",
                    label(world, probe.field),
                    probe.kind,
                    probe.zoom
                );
                state.log.push(line);
                state.index += 1;
                state.step = 0;
                return;
            };
            let node = node_of(world, probe.field);
            let pos = node.and_then(|n| world.get::<NodePosition>(n).map(|p| p.0));
            let sel = node.is_some_and(|n| world.get::<Selected>(n).is_some());
            let text = text_of(world, probe.field);
            state.before = Some((pos, sel, text));
            pointer(world, at, PointerAction::Move { delta: Vec2::ZERO });
        }
        9 => pointer(
            world,
            state.at.unwrap(),
            PointerAction::Press(PointerButton::Primary),
        ),
        10 => pointer(
            world,
            state.at.unwrap(),
            PointerAction::Release(PointerButton::Primary),
        ),
        16 => {
            check_press(world, &mut state, probe);
            let c = digit(&state.before.as_ref().unwrap().2);
            key(world, c, ButtonState::Pressed);
        }
        17 => {
            let c = digit(&state.before.as_ref().unwrap().2);
            key(world, c, ButtonState::Released);
        }
        22 => {
            let (_, _, before) = state.before.clone().unwrap();
            let after = text_of(world, probe.field);
            let focused = world.resource::<InputFocus>().get() == Some(probe.field);
            if focused && after == before {
                let line = format!(
                    "  FAIL  {} {:?} zoom {}: typed a digit but text stayed {before:?}",
                    label(world, probe.field),
                    probe.kind,
                    probe.zoom
                );
                state.log.push(line);
                state.fails += 1;
            }
            // Undo the typing so later probes see the same text.
            if after != before {
                if let Some(mut text) = world.get_mut::<EditableText>(probe.field) {
                    text.queue_edit(bevy::text::TextEdit::SelectAll);
                    text.queue_edit(bevy::text::TextEdit::Insert(before.into()));
                }
            }
            state.index += 1;
            state.step = 0;
        }
        _ => {}
    }
}

fn check_press(world: &mut World, state: &mut State, probe: Probe) {
    let at = state.at.unwrap();
    let field = probe.field;
    let tag = format!(
        "{} {:?} zoom {} at ({:.0},{:.0})",
        label(world, field),
        probe.kind,
        probe.zoom,
        at.x,
        at.y
    );
    let mut problems = Vec::new();
    let focus = world.resource::<InputFocus>().get();
    if focus != Some(field) {
        let other = focus.map_or("nothing".to_string(), |e| describe(world, e));
        let hits = world
            .resource::<bevy::picking::hover::HoverMap>()
            .get(&POINTER)
            .map(|m| m.iter().map(|(e, h)| (h.depth, *e)).collect::<Vec<_>>())
            .unwrap_or_default();
        let mut hits = hits;
        hits.sort_by(|a, b| a.0.total_cmp(&b.0));
        let hit = hits
            .first()
            .map_or("nothing".into(), |&(_, e)| describe(world, e));
        problems.push(format!("focus went to {other}; pointer was over {hit}"));
    }
    let style = world.get::<TextCursorStyle>(field).copied();
    match style {
        None => problems.push("no TextCursorStyle: cursor and selection are never drawn".into()),
        Some(style) => {
            if let Some(bg) = background(world, field) {
                let ratio = contrast(style.color, bg);
                if ratio < 3.0 {
                    problems.push(format!(
                        "cursor contrast {ratio:.2}:1 ({:?} on {:?})",
                        hex(style.color),
                        hex(bg)
                    ));
                }
            }
        }
    }
    let local = to_text_local(world, field, at);
    if let Some(info) = world.get::<TextLayoutInfo>(field) {
        if focus == Some(field) && info.cursor.is_none() && info.selection_rects.is_empty() {
            problems.push("focused, but no cursor geometry".into());
        }
        // Where the cursor shows, against where the pointer pressed.
        if let (Kind::Text(_), Some((_, rect)), true) =
            (probe.kind, info.cursor, info.selection_rects.is_empty())
            && let Some(local) = local
        {
            let end = info
                .glyphs
                .iter()
                .map(|g| g.position.x + g.atlas_info.rect.size().x / 2.0)
                .fold(0.0f32, f32::max);
            let expect = local.x.min(end).max(0.0);
            let tolerance = rect.height().max(8.0) * 0.6;
            if (rect.center().x - expect).abs() > tolerance {
                problems.push(format!(
                    "cursor at x {:.1}, pressed at {:.1} (text local)",
                    rect.center().x,
                    local.x
                ));
            }
        }
    }
    if let Some((pos, sel, _)) = &state.before {
        let node = node_of(world, field);
        let now = node.and_then(|n| world.get::<NodePosition>(n).map(|p| p.0));
        if now != *pos {
            problems.push(format!("node moved {pos:?} -> {now:?}"));
        }
        if !sel && node.is_some_and(|n| world.get::<Selected>(n).is_some()) {
            problems.push("press selected the node".into());
        }
    }
    let shot = format!("{}/{}-{:02}.png", out_dir(), name(), state.index);
    let line = if problems.is_empty() {
        format!("  ok    {tag}")
    } else {
        state.fails += 1;
        format!("  FAIL  {tag}: {}  [{shot}]", problems.join("; "))
    };
    state.log.push(line);
    if std::env::var("AUDIT_SHOTS").is_ok() || !problems.is_empty() {
        world
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(shot));
    }
}

fn finish(world: &mut World, state: &mut State) {
    let summary = format!(
        "{}: {} probes, {} failed",
        name(),
        state.probes.len(),
        state.fails
    );
    state.log.push(summary);
    let path = format!("{}/{}.log", out_dir(), name());
    let mut file = std::fs::File::create(path).unwrap();
    for line in &state.log {
        println!("{line}");
        writeln!(file, "{line}").unwrap();
    }
    world.write_message(AppExit::Success);
}

fn set_zoom(world: &mut World, zoom: f32) {
    let mut q = world.query::<&mut CanvasView>();
    for mut view in q.iter_mut(world) {
        if view.zoom != zoom {
            view.zoom = zoom;
            view.pan = Vec2::ZERO;
        }
    }
}

/// The graph node a field sits in.
fn node_of(world: &World, mut e: Entity) -> Option<Entity> {
    loop {
        if world.get::<GraphNode>(e).is_some() {
            return Some(e);
        }
        e = world.get::<ChildOf>(e)?.parent();
    }
}

fn text_of(world: &World, field: Entity) -> String {
    world
        .get::<EditableText>(field)
        .map(|t| t.value().to_string())
        .unwrap_or_default()
}

fn label(world: &World, field: Entity) -> String {
    let mut text = text_of(world, field);
    text.truncate(12);
    format!("[{field} {text:?}]")
}

fn describe(world: &World, e: Entity) -> String {
    let mut kinds = Vec::new();
    if world.get::<GraphNode>(e).is_some() {
        kinds.push("GraphNode");
    }
    if world.get::<Port>(e).is_some() {
        kinds.push("Port");
    }
    if world.get::<EditableText>(e).is_some() {
        kinds.push("another EditableText");
    }
    if let Some(h) = world.get::<EdgeHitbox>(e) {
        return format!(
            "{e} Edge radius={} below_nodes={} points={:?}",
            h.radius, h.below_nodes, h.points
        );
    }
    if world.get::<Window>(e).is_some() {
        kinds.push("Window");
    }
    let name = world
        .get::<Name>(e)
        .map(|n| n.to_string())
        .unwrap_or_default();
    let parent = world.get::<ChildOf>(e).map(|c| c.parent());
    let text = world.get::<Text>(e).map(|t| t.0.clone());
    let node = world.get::<ComputedNode>(e).map(|n| n.size());
    format!("{e} {kinds:?} {name} text={text:?} size={node:?} parent={parent:?}")
}

/// Screen rectangle (physical pixels) of a node, ignoring rotation.
fn rect(world: &World, e: Entity) -> Option<Rect> {
    let node = world.get::<ComputedNode>(e)?;
    let gt = world.get::<UiGlobalTransform>(e)?;
    let half = node.size() / 2.0;
    let a = gt.transform_point2(-half);
    let b = gt.transform_point2(half);
    Some(Rect::from_corners(a, b))
}

fn scale_factor(world: &mut World) -> f32 {
    let mut q = world.query_filtered::<&Window, With<PrimaryWindow>>();
    q.single(world).map_or(1.0, |w| w.scale_factor())
}

/// Logical window position to press for a probe.
fn probe_point(world: &mut World, probe: Probe) -> Option<Vec2> {
    let field = probe.field;
    let phys = match probe.kind {
        Kind::Text(x) => {
            let node = world.get::<ComputedNode>(field)?;
            let gt = world.get::<UiGlobalTransform>(field)?;
            let content = node.content_box();
            let local = Vec2::new(
                content.min.x + content.width() * x.min(0.98),
                content.center().y,
            );
            gt.transform_point2(local)
        }
        Kind::Padding => {
            let parent = world.get::<ChildOf>(field)?.parent();
            if world.get::<GraphNode>(parent).is_some() {
                return None;
            }
            let outer = rect(world, parent)?;
            let inner = rect(world, field)?;
            let siblings: Vec<Rect> = world
                .get::<Children>(parent)?
                .iter()
                .filter_map(|c| rect(world, c))
                .collect();
            let candidates = [
                Vec2::new(outer.center().x, outer.min.y + 1.5),
                Vec2::new(outer.center().x, outer.max.y - 1.5),
                Vec2::new(outer.min.x + 1.5, outer.center().y),
                Vec2::new(outer.max.x - 1.5, outer.center().y),
                Vec2::new(inner.max.x + 1.0, outer.center().y),
            ];
            candidates
                .into_iter()
                .find(|p| outer.contains(*p) && !siblings.iter().any(|s| s.contains(*p)))?
        }
        Kind::Label => {
            let parent = world.get::<ChildOf>(field)?.parent();
            if world.get::<GraphNode>(parent).is_some() {
                return None;
            }
            let sibling = world
                .get::<Children>(parent)?
                .iter()
                .find(|&c| c != field && world.get::<EditableText>(c).is_none())?;
            rect(world, sibling)?.center()
        }
    };
    if let Some(clip) = world.get::<bevy::ui::CalculatedClip>(field)
        && !clip.clip.contains(phys)
    {
        return None;
    }
    let logical = phys / scale_factor(world);
    let size = world
        .query_filtered::<&Window, With<PrimaryWindow>>()
        .single(world)
        .ok()?
        .size();
    (logical.x > 1.0 && logical.y > 1.0 && logical.x < size.x - 1.0 && logical.y < size.y - 1.0)
        .then_some(logical)
}

/// Logical window position → the field's text layout space.
fn to_text_local(world: &mut World, field: Entity, at: Vec2) -> Option<Vec2> {
    let phys = at * scale_factor(world);
    let node = world.get::<ComputedNode>(field)?;
    let gt = world.get::<UiGlobalTransform>(field)?;
    let scroll = world.get::<TextScroll>(field).map_or(Vec2::ZERO, |s| s.0);
    Some(gt.try_inverse()?.transform_point2(phys) - node.content_box().min + scroll)
}

/// The first mostly opaque background at or behind a field.
fn background(world: &World, mut e: Entity) -> Option<Color> {
    loop {
        if let Some(bg) = world.get::<BackgroundColor>(e)
            && bg.0.alpha() > 0.5
        {
            return Some(bg.0);
        }
        match world.get::<ChildOf>(e) {
            Some(c) => e = c.parent(),
            None => return world.get_resource::<ClearColor>().map(|c| c.0),
        }
    }
}

fn luminance(c: Color) -> f32 {
    let l = c.to_linear();
    0.2126 * l.red + 0.7152 * l.green + 0.0722 * l.blue
}

fn contrast(a: Color, b: Color) -> f32 {
    let (x, y) = (luminance(a) + 0.05, luminance(b) + 0.05);
    x.max(y) / x.min(y)
}

fn hex(c: Color) -> String {
    c.to_srgba().to_hex()
}

fn pointer(world: &mut World, at: Vec2, action: PointerAction) {
    let window = world
        .query_filtered::<Entity, With<PrimaryWindow>>()
        .single(world)
        .unwrap();
    let target = RenderTarget::Window(WindowRef::Primary)
        .normalize(Some(window))
        .unwrap();
    world.write_message(PointerInput::new(
        POINTER,
        Location {
            target,
            position: at,
        },
        action,
    ));
}

fn digit(text: &str) -> &'static str {
    ["5", "6", "7", "8", "9", "0", "1"]
        .into_iter()
        .find(|d| !text.contains(d))
        .unwrap_or("5")
}

fn key(world: &mut World, c: &str, state: ButtonState) {
    let window = world
        .query_filtered::<Entity, With<PrimaryWindow>>()
        .single(world)
        .unwrap();
    world.write_message(KeyboardInput {
        key_code: KeyCode::Digit5,
        logical_key: Key::Character(c.into()),
        state,
        text: (state == ButtonState::Pressed).then(|| c.into()),
        repeat: false,
        window,
    });
}
