# bevy_noodle 0.2 — design

Status: implemented on branch `redesign-0.2` (see "Implementation notes" at the end for where it differs from this proposal). Reference sources are in `references/`
(gitignored); citations are `crate/path:line` into those copies.

## Goal

> Super minimal. Each node stylable. No forced background. Edges stylable.
> Doesn't take over control or force any bindings. Super customizable.

So 0.2 is a **headless** node-graph library, in the style of Bevy's own
`bevy_ui_widgets`. The core owns graph structure and interaction *behaviour*
and draws nothing. You build the visuals, or opt in to a default kit.

This is a rewrite of the public API, not a patch on 0.1. The 0.1 design
(trait schema, slotmap graph, library-built node UI) fights almost every item
of the goal.

---

## 1. Core decision: the graph is entities

The canvas, the nodes, the ports and the edges are all ECS entities, and they
are the source of truth. 0.1 kept a slotmap inside one component.

| Goal | Why entities deliver it |
|---|---|
| Each node stylable | A node is *your* UI entity. Style it with `BackgroundColor`, `BorderColor`, `BoxShadow`, feathers, anything. |
| Edges stylable | An edge is an entity. Add style components, or render it however you want. |
| Customizable | Attach your own components to nodes, ports and edges, and query them like anything else. |
| Composes | `Changed`/`Added`/`Hovered` come for free. Picking events bubble node → canvas → window. Several canvases are just several entities. The inspector shows the graph as a hierarchy. |

This mirrors `bevy_ui_widgets`. Users spawn the visual hierarchy and tag the
parts with markers, and the library finds them by marker: "makes no
assumptions about the hierarchical structure… expects that the thumb will be
marked with a `SliderThumb`" (`bevy_ui_widgets-0.19.1/src/slider.rs:81-86`).

**Costs and mitigations:**
- **Traversal is slower than a slotmap.** The library maintains a computed
  `GraphIndex` (adjacency) on the canvas, updated incrementally, like
  `ComputedNode`.
- **Entity ids aren't stable across save/load.** Add an optional
  `NodeKey(u64)`, plus Reflect-based subtree export/import (serde feature).
- **Code can break invariants**, e.g. by despawning a port. Hooks repair it:
  removing a port removes its edges.

The generic `NodeGraphSchema` and its four traits leave the core. Port typing
becomes a plain component, and anything fancier is a veto observer (§3).

---

## 2. Components (core)

All public types derive `Reflect` with `#[reflect(Component, Default)]` and are
registered by the plugin, so they show up in the inspector
(`bevy-inspector-egui-0.37.0/src/restricted_world_view.rs:461-466`).

**User-authored** (you insert these):

| Component | Meaning |
|---|---|
| `NodeCanvas` | Marks a UI node as a graph canvas. Requires `CanvasView` and `GraphIndex`. Adds no background and no children. |
| `CanvasView { pan, zoom }` | The camera. The library applies it to the canvas content (see below). |
| `CanvasContent` | Marks the child that holds nodes and edges. The library writes only this entity's `UiTransform` from `CanvasView`. |
| `GraphNode` | Marks a node's root UI entity. Requires `NodePosition`. |
| `NodePosition(Vec2)` | Position in graph space. The library writes the node's `Node.left/top` from it, and nothing else on your entity. |
| `Port { direction, data_type, max_connections }` | Any UI entity inside a node. `data_type` is a user-chosen `PortType(u32 or &'static str)`. |
| `NodeDragHandle` | Optional. If present in a node, only it starts node drags; otherwise the whole node does. |

**Library-managed:**

| Component | Meaning |
|---|---|
| `Edge { from: Entity, to: Entity }` | Spawned on connect. Port entities, output → input. Requires `EdgeGeometry`. |
| `EdgeGeometry { start, end, start_dir, end_dir }` | Computed every time a port moves (port center from `UiGlobalTransform`, mapped into canvas space). Any renderer reads it: the default shader, gizmos, your own material. This follows the scrollbar precedent of "core computes, stylist draws" (`bevy_ui_widgets-0.19.1/src/scrollbar.rs:282-458`). |
| `GraphIndex` | Computed adjacency on the canvas, used for evaluation helpers (`inputs_of(node)`, `upstream(port)`). |
| State markers | `Selected` (reuse `bevy_ui::Selected`), `Pressed`, `InteractionDisabled` (reuse the bevy_ui ones, `bevy_ui-0.19.1/src/interaction_states.rs`), plus `DraggingNode`, `WireSource`, `WireCandidate` and `WireInvalid` on ports during a wire drag. Style by reacting to these, the way feathers does (`bevy_feathers-0.19.1/src/controls/button.rs:199-318`). |

**Change-detection rule.** User-authored components change only when something
meaningful changes. Library writes are skipped when the value is equal (ui
layout does this, `bevy_ui-0.19.1/src/layout/mod.rs:271-279`). Transient
interaction state lives in separate components, never in user-facing ones.
This fixes 0.1 item 2.

---

## 3. Edits: one path, observable, vetoable

Every mutation goes through one pipeline, whether it comes from the built-in
interaction, your code, an undo stack or the network.

```rust
pub enum GraphEdit {
    Connect { from: Entity, to: Entity },
    Disconnect { edge: Entity },
    MoveNodes { nodes: Vec<Entity>, delta: Vec2, is_final: bool },
    DeleteNodes(Vec<Entity>),
    Select { nodes: Vec<Entity>, mode: SelectMode },
}

// Entry points: commands, world, and the interaction layer all use these.
commands.entity(canvas).graph_edit(GraphEdit::Connect { from, to });
world.graph_edit(canvas, edit);
```

Pipeline:
1. **Validate** (built-in): ports exist, output → input, not the same node,
   `data_type` equal, `max_connections`.
2. **`EditRequested { canvas, edit, origin, rejected }`** is triggered with
   `World::trigger_ref` (bevy_ecs 0.19 `observer/mod.rs:84`, "check or use the
   event after it has been modified by observers"). Your observers set
   `rejected` to veto. This covers cycles, "only one Start", type conversions
   and permissions. It fixes 0.1 item 6.
3. **Apply.**
4. **`EditApplied { canvas, edit, origin }`** is triggered as an `EntityEvent`
   on the canvas *and* written as a `Message`, the same push+pull pair picking
   uses (`bevy_picking-0.19.1/src/events.rs:1083-1084`). This fixes 0.1 items
   4 and 5.

`origin: EditOrigin { Interaction, Code, Custom(u64) }` lets netcode and undo
skip echoes. It mirrors replicon's `client_id` on `FromClient`
(`bevy_replicon-0.44.2/src/shared/message/client_event.rs:117-129`).

**Node spawning and despawning are not edits.** You spawn nodes with ordinary
`commands.spawn`, and the library reacts with `On<Add, GraphNode>` /
`On<Remove, GraphNode>` (index update, edge cleanup). `DeleteNodes` exists so
interactive deletion can be vetoed.

A `MoveNodes` drag streams with `is_final: false` and ends with `true`, like
`ValueChange::is_final` (`bevy_ui_widgets-0.19.1/src/lib.rs:87-100`). Undo
systems record only final moves.

---

## 4. Input: behaviour only, zero bindings in core

### Interaction plugin: picking only
- **Pointer events only.** All drags use `Pointer<DragStart/Drag/DragEnd/
  Cancel/DragDrop>` observers registered globally with `app.add_observer`,
  filtered by marker queries. The library attaches no per-entity observers to
  your entities (`bevy_ui_widgets-0.19.1/src/button.rs:142-150`).
- **Any pointer works.** In-flight drag state is keyed by `PointerId`
  (`bevy_picking-0.19.1/src/pointer.rs:34-44`), so touch, pen, custom pointers
  and render-to-texture targets work. It never reads `ButtonInput<MouseButton>`.
  This fixes 0.1 item 9.
- **Connecting.** Dropping onto a port uses `Pointer<DragDrop>` (`dropped` is
  the source port), plus a snap radius for near misses.
- **Configurable per canvas.** `CanvasInteraction` lets every behaviour be set
  to `None`:
  `{ pan: Option<PointerButton>, box_select: Option<PointerButton>, drag_nodes: bool, connect: bool, scroll: ScrollMode, drag_threshold: f32 }`.
- **Turning interaction off.** `InteractionDisabled` on a canvas, node or port
  turns its interaction off, like avian's `RigidBodyDisabled`
  (`avian2d-0.7.0/src/dynamics/rigid_body/mod.rs:331-380`).
- **Input ownership.** A per-canvas `CanvasWantsInput { pointer, dragging }`
  plus the run conditions `canvas_wants_pointer_input()` tell the rest of the
  app the graph is busy. App input is never absorbed; egui's absorbing is also
  off by default (`bevy_egui-0.42.0/src/lib.rs:292`).

### Actions are events; no bindings in core
```rust
commands.trigger(DeleteSelection { canvas });
commands.trigger(SelectAll { canvas });
commands.trigger(FrameAll { canvas });
commands.trigger(CancelInteraction { canvas });
commands.trigger(PanBy { canvas, delta });
commands.trigger(ZoomBy { canvas, factor, anchor });
commands.trigger(RequestNodeFinder { canvas, at, pending_wire }); // headless; see §5
```
Any input crate can drive these. With bevy_enhanced_input:
`On<Start<MyDelete>>` → `commands.trigger(DeleteSelection{..})`. With
leafwing: `just_pressed` → trigger. Neither becomes a dependency.

### Optional `NoodleKeyBindingsPlugin`
- **Not in the default group.** It's added separately, like avian's debug
  plugin (`avian2d-0.7.0/src/lib.rs:62-65`).
- **Bindings are data.** They live in a per-canvas `CanvasKeymap`
  (Reflect/serde, `Default` = 0.1's shortcuts), which you clear, merge or
  replace like leafwing's `InputMap`
  (`leafwing-input-manager-0.21.0/src/input_map.rs:103-107`). No keymap means
  no bindings.
- **Focus-based.** It reads `FocusedInput<KeyboardInput>` on the canvas
  (`TabIndex`), not global keys, and consumes only keys it handles
  (`bevy_input_focus-0.19.1/src/lib.rs:186-247`). This fixes 0.1 item 1.
- **Never panics on missing focus support.** Every focus resource is
  `Option<Res…>` (`bevy_ui_widgets-0.19.1/src/checkbox.rs:93-106`). This fixes
  0.1 item 3.

---

## 5. Visuals: all optional

The core draws nothing: no background, no grid, no node chrome, no wire
pixels. Visuals live behind the cargo feature `default_style` in
`NoodleDefaultStylePlugin`. Each piece is opt-in **per entity**:

| Piece | Opt in with | Style with |
|---|---|---|
| Wire renderer (0.1's anti-aliased Bézier `UiMaterial`) | `DefaultEdgeRenderer` on the canvas | `EdgeStyle { color, width, dash?, curvature }` on the canvas (default) or on each `Edge` (override). Port-type colors come via an `EdgeColorByType` map. |
| Wire preview while dragging | the same renderer | `PreviewEdgeStyle` |
| Grid background (0.1's shader) | `CanvasGrid { spacing, major_every, colors }` | — |
| Box-selection rectangle | `SelectionBoxStyle` | — |
| Node kit | `node_kit::title_bar(..)`, `node_kit::port_row(..)` | These are **functions returning bundles**, not systems. Use them, copy them, or ignore them. |
| Node finder popup | `DefaultNodeFinder { templates }` | Listens to `RequestNodeFinder`. Without it, the request event is yours to handle. |

Inline value widgets (0.1's `ValueWidget`) leave the library. Nodes are your
UI, so use `bevy_ui_widgets` or feathers sliders, checkboxes and text inputs
directly. The examples show this.

This mirrors the split between `bevy_ui_widgets` and feathers: feathers is
added separately and never pulls in the headless widgets
(`bevy_feathers-0.19.1/src/lib.rs:117-124`).

---

## 6. Plugins and scheduling

```rust
NoodlePlugins            // PluginGroup: .disable::<…>() / .set(…)
├── NoodleCorePlugin         components, edit pipeline, GraphIndex, EdgeGeometry,
│                            CanvasView → CanvasContent transform, Reflect registration
└── NoodleInteractionPlugin  picking-driven drag / connect / select / pan / zoom
NoodleKeyBindingsPlugin  // opt-in, separate
NoodleDefaultStylePlugin // feature = "default_style", separate
```
- **Structure follows avian.** `PhysicsPlugins` is a configurable group
  (`avian2d-0.7.0/src/lib.rs:757-790`). Internal shared plugins are added only
  if missing (`collider_tree/mod.rs:69`). External plugins are never assumed.
- **Mostly event-driven.** Behaviour runs in observers, so it doesn't depend
  on a schedule. The only systems are:
  - `EdgeGeometry` and `NodePosition` → `Node` sync, before `UiSystems::Layout`
  - port measurement, after `UiSystems::Layout`

  Both are filtered on `Changed<…>`, so idle or hidden canvases cost almost
  nothing (0.1 item 8). Dirty work uses a SparseSet marker, like avian's
  `RecomputeMassProperties`
  (`mass_properties/components/mod.rs:1028-1030`).
- **Public sets with hook points:**
  `NoodleSystems::{First, Index, Geometry, Measure, Last}`. `First`/`Last`
  are empty for users to hook into (`avian2d-0.7.0/src/schedule/mod.rs:162-176`).
- **No takeover.** The library never rewrites your `Node` (0.1 set overflow
  and size). It only writes `left/top` on `GraphNode`s and the `UiTransform`
  of `CanvasContent`. Cleanup runs in `On<Remove>` observers (0.1 item 7).

---

## 7. What a user writes (sketch, not compiled)

```rust
App::new().add_plugins((DefaultPlugins, NoodlePlugins));

let canvas = commands.spawn((NodeCanvas, Node { width: percent(100), height: percent(100), ..default() })).id();
let content = commands.spawn((CanvasContent, ChildOf(canvas))).id();

let add = commands.spawn((
    GraphNode, NodePosition(vec2(300., 80.)), ChildOf(content),
    Node { flex_direction: FlexDirection::Column, ..default() },
    BackgroundColor(MY_NODE_BG),                // styled like any UI
)).with_children(|n| {
    n.spawn((Text::new("Add"), NodeDragHandle));
    n.spawn((Port::input(NUMBER), Node { width: px(10), height: px(10), ..default() }, BackgroundColor(BLUE)));
    n.spawn((Port::output(NUMBER), /* … */));
}).id();

// Veto: no cycles.
app.add_observer(|mut req: On<EditRequested>, index: Query<&GraphIndex>| {
    if let GraphEdit::Connect { from, to } = req.edit { if creates_cycle(&index, from, to) { req.rejected = true; } }
});

// React: re-evaluate.
app.add_observer(|_: On<EditApplied>, /* … */| { /* evaluate */ });
```

---

## 8. Coverage of the earlier list

| # | Problem in 0.1 | Fixed by |
|---|---|---|
| 1 | Global, hard-coded shortcuts | No bindings in core; action events; opt-in focus-based keymap (§4) |
| 2 | Changed every frame | Entities, authored vs computed split, write only on difference (§2) |
| 3 | Panics or silent breakage without plugins | `Option<Res>`, no external plugin assumptions (§4, §6) |
| 4 | Code edits bypass events | One edit pipeline (§3) |
| 5 | Only a global message | `EditApplied` as an entity event and a message (§3) |
| 6 | No veto | `EditRequested` + `trigger_ref` (§3) |
| 7 | Takes over the user's entity | Writes only `left/top` + content transform; `On<Remove>` cleanup (§6) |
| 8 | Per-frame work | Observers, `Changed` filters, dirty markers (§6) |
| 9 | Mouse/window only | Picking events keyed by `PointerId` (§4) |
| 10 | No Reflect, schedule control | Reflect everywhere; plugin group and public sets (§2, §6) |
| new | Forced background, chrome, wires, widgets | Core draws nothing; per-entity opt-in style kit (§5) |
| new | Per-node and per-edge styling | Nodes and edges are your entities; `EdgeStyle` overrides per edge (§1, §5) |

---

## 9. Bevy 0.20

0.20 RCs are out. The changes that touch us:
- picking events renamed (`Pointer<Press>` → `PointerPress`, …)
- lifecycle observers become `On<Insert<T>>`
- pointer capture via `PointerCaptureMap`
- new `PointerFocusPlugin`

(`references/bevy-0.20.0-rc.2/bevy_ui_widgets-0.20.0-rc.2/src/slider.rs:22-26, 387-398`)

Build 0.2 on 0.19, with picking and lifecycle-observer usage kept in one
internal `compat` module, so 0.3 is a small port. Adopt pointer capture once
on 0.20.

---

## 10. Implementation order

1. **Core.** Components, `GraphIndex`, edit pipeline with veto, `On<Add/Remove>`
   integrity, Reflect registration.
   - *Test:* headless `World` tests only. No rendering is needed, which 0.1
     could not do.
2. **Geometry.** `CanvasView` → content transform, `NodePosition` → `left/top`,
   port measurement → `EdgeGeometry`.
   - *Test:* offscreen harness (the 0.1 approach).
3. **Interaction.** Picking drags, connect, box select, pan/zoom,
   `CanvasInteraction`, `CanvasWantsInput`, action events.
   - *Test:* scripted `PointerInput` harness.
4. **Default style.** Port the 0.1 wire and grid shaders, `EdgeStyle`,
   selection box, node kit, default finder.
5. **Keybindings plugin**, serde/Reflect export with `NodeKey`.
6. **Examples.**
   - `minimal`: hand-built nodes, no default style, proves "nothing forced"
   - `styled`: default kit
   - `math_graph`: ported, with feathers widgets inside nodes
   - `dialogue`: veto rules
   - `enhanced_input`: bindings recipe, as a dev-dependency only

## 11. Open questions

1. **Entities as the source of truth?** Recommended. The alternative keeps the
   0.1 slotmap and only adds the headless and binding changes, which is less
   work but leaves per-node styling and `Changed` awkward.
2. **Port typing.** A plain `PortType` equality check plus veto, as proposed?
   Or keep an optional typed `DataTypeTrait` layer for compatibility rules?
3. **Bevy version.** Ship 0.2 on 0.19 now, or wait for 0.20 final?
4. **Default style packaging.** A feature in this crate (proposed, simplest to
   publish), or a separate `bevy_noodle_style` crate like feathers?

---

## Implementation notes

Implemented on `redesign-0.2`, against Bevy 0.19.

**Deviations from the proposal:**
- **No `GraphIndex` cache.** Edges are Bevy relationships instead:
  `EdgeSource`/`EdgeTarget` on the edge, `OutgoingEdges`/`IncomingEdges` on the
  ports, with `linked_spawn`. Bevy keeps adjacency correct, and despawning a
  port or node removes its edges for free. `GraphQuery` (a `SystemParam`)
  provides the traversal helpers.
- **Simpler system sets.** `NoodleSystems` is `{Sync, Render, Measure}`, not
  `{First, Index, Geometry, Measure, Last}`.
- **Pre-layout edge geometry.** It's computed from `NodePosition` plus the
  last measured port offset, so edges follow dragged nodes with no frame of
  lag. Offsets are re-measured after layout.
- **Raising nodes.** Done by re-adding the node to its parent, which moves it
  last in `Children`; this is pinned by a test. The user's `ZIndex` is never
  written.
- **No inline value widgets.** Use `bevy_ui_widgets`/`EditableText` inside
  nodes; `math_graph` shows how.
- **Not yet done:** serde/Reflect export with `NodeKey` (step 5 of the
  implementation order), and an undo example built on `EditApplied`.

**Verification:**
- 15 unit/integration tests, all headless.
- Offscreen scripted runs of all three examples:
  - wire drag, then the filtered finder, then create and connect
  - box select, keymap delete, wheel zoom
  - drag-to-connect without the style feature
  - live evaluation, cycle veto, typing into a node's text field
