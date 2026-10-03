# Changelog

## Unreleased

### Added
- Controls inside nodes: a press or drag that starts in a focusable control
  (anything with a `TabIndex`) belongs to the control, so sliders, text
  fields, color pickers and menus no longer select, raise or move the node.
- `scene::Transient`: snapshots leave out entities marked with it (and their
  descendants), for UI the app rebuilds from its own data.
- Connection rules observers can override: `EditRequested::refused` carries
  the built-in verdict (types, already connected, full port), and observers
  may `allow()` or `reject()` it. `EditRequested::preview` and
  `GraphWorldExt::preview_edit` ask observers without applying anything;
  dragged wires and keyboard connections snap to ports observers would allow.
  `type_conversion` now uses real int and float ports.
- Keyboard use, opt-in per canvas: `NoodleKeyboardPlugin` and
  `CanvasKeyboard` (Tab focus, Enter, arrows, Space to connect, Escape), built
  on `bevy_input_focus`; a `FocusOutline` in the default style.
  Ctrl/Cmd+arrows pan the view and +/- zoom it. Example: `keyboard`.
- Examples: `comment_frames`, `reroute`, `type_conversion`, `minimap`,
  `auto_layout`.
- `kit::input_dot`/`kit::output_dot`: port rows without labels, about a
  fifth fewer UI entities per node and no text to lay out. The `stress`
  example switches to them with T.

### Fixed
- Edges of an outer graph are pickable where they pass over a nested canvas.
- New wires and selection boxes show in the frame they appear, not the next.
- Restoring or inserting a snapshot no longer leaves children it left out
  listed in their parent's `Children`.

### Changed
- `save_load` also saves and opens the graph model alone (a few hundred
  bytes instead of a full snapshot).

### Examples
- styled, editor, save_load, subgraph, scene_builder_3d, type_conversion,
  keyboard and comment_frames edit every value in the node, with Bevy's
  feathers controls (number fields, sliders, color pickers, a dropdown) or
  `EditableText`, and show results live.

## 0.3.0

A smaller, Bevy-native core: you build and style the nodes and edges, the
library handles the graph. See the README's "Migrating from 0.2" for every
breaking change.

### Added
- Pickable edges: `EdgeHitbox` gives edges `Pointer` events; clicking selects
  them, and `GraphEdit::Delete` removes selected nodes and edges together.
- `scene` feature: `DynamicWorld` snapshots of a graph or of selected nodes
  (`snapshot`, `snapshot_nodes`, `insert`, `restore`) for undo, copy and
  paste, and saving.
- `EdgeStyle`: gradients, dashes, animated flow, layer (above or below
  nodes), hover and selection highlights, trimming at port rims.
- `kit::canvas()`, and flatter kit nodes (two fewer entities each).
- `EditApplied::ports`, `GraphQuery::edges_in`, `EditResult`.
- Examples: `edge_styles`, `subgraph`, `scene_builder_3d`, `editor`,
  `save_load`, `stress`.

### Changed
- Interaction is opt-in per canvas (`CanvasInteraction`), with configurable
  buttons and keys; no built-in key bindings.
- `GraphEdit::DeleteNodes { nodes }` → `Delete { items }`;
  `Select { nodes, .. }` → `Select { items, .. }`; `nodes_of` → `nodes_in`.
- Edges are children of the canvas content and follow their ports into other
  graphs; `Edge` is a marker.
- `NodePosition` is optional: without it, your layout places the node.
- Wire snapping and pinch zoom use Bevy's `HoverMap`; edge hit testing uses
  `bevy_math` curves.

### Removed
- `NoodleKeyBindingsPlugin`, `CanvasKeymap`, the action events, the node
  finder popup, `CanvasWantsInput`, `KitTheme`, `kit::body`.

### Fixed
- `NodeDragHandle` is enforced for the whole drag.
- The interaction plugin no longer panics without a picking backend.
- Edits no longer scan the whole canvas, so large graphs stay fast.

## 0.2.0
Headless, entity-based redesign.

## 0.1.0
First release.
