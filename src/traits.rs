//! The traits you implement to describe your node graph.
//!
//! | egui_node_graph2       | bevy_noodle                                    |
//! |------------------------|----------------------------------------------------|
//! | `DataTypeTrait`        | [`DataTypeTrait`]                                  |
//! | `WidgetValueTrait`     | [`WidgetValueTrait`] (returns a [`ValueWidget`] description instead of drawing) |
//! | `NodeDataTrait`        | [`NodeDataTrait`] (`bottom_ui` becomes [`NodeDataTrait::spawn_body`]) |
//! | `NodeTemplateTrait`    | [`NodeTemplateTrait`]                              |
//! | `NodeTemplateIter`     | the `templates` list passed to [`NodeGraphEditor::new`](crate::NodeGraphEditor::new) |
//! | five generic params    | one [`NodeGraphSchema`] marker type                |

use std::borrow::Cow;

use bevy::ecs::hierarchy::ChildSpawnerCommands;
use bevy::prelude::*;

use crate::graph::{Graph, NodeId};
use crate::style::NodeGraphStyle;

/// The type of data that flows over a wire.
///
/// Its color is used for ports and wires, and [`is_compatible_with`](Self::is_compatible_with)
/// decides which outputs may connect to which inputs.
pub trait DataTypeTrait: PartialEq + Clone + Send + Sync + 'static {
    /// Color of ports and wires carrying this type.
    fn color(&self) -> Color;

    /// Human-readable name, shown when hovering a port.
    fn name(&self) -> Cow<'_, str>;

    /// Whether an output of type `self` may connect to an input of type `input`.
    /// Defaults to equality; override it to allow implicit conversions.
    fn is_compatible_with(&self, input: &Self) -> bool {
        self == input
    }
}

/// A constant value stored in an input parameter.
///
/// Bevy UI is retained-mode, so instead of drawing a widget every frame you
/// *describe* it with [`value_widget`](Self::value_widget). The editor builds
/// and keeps the widget in sync, and hands edits back through
/// [`apply_edit`](Self::apply_edit).
pub trait WidgetValueTrait: Clone + Send + Sync + 'static {
    /// Describes the inline editor for an input named `param_name`.
    fn value_widget(&self, param_name: &str) -> ValueWidget;

    /// Applies an edit made through the widget described by
    /// [`value_widget`](Self::value_widget).
    fn apply_edit(&mut self, edit: ValueEdit);
}

/// Per-node user data. Usually holds the template the node was built from.
pub trait NodeDataTrait: Clone + Send + Sync + 'static {
    /// The [`DataTypeTrait`] of this graph.
    type DataType: DataTypeTrait;
    /// The [`WidgetValueTrait`] of this graph.
    type ValueType: WidgetValueTrait;

    /// Overrides the title bar color for this node.
    fn titlebar_color(&self) -> Option<Color> {
        None
    }

    /// Whether the user may delete this node from the UI.
    fn can_delete(&self) -> bool {
        true
    }

    /// Spawns custom UI at the bottom of the node (the analogue of egui's
    /// `bottom_ui`). Whatever you spawn here is yours: attach observers or
    /// marker components and drive it from your own systems.
    ///
    /// The body is rebuilt whenever the node's structure changes or
    /// [`body_revision`](Self::body_revision) returns a new value.
    #[allow(unused_variables)]
    fn spawn_body(&self, ctx: NodeBodyContext<'_, Self>, body: &mut ChildSpawnerCommands) {}

    /// Change this value to request a rebuild of [`spawn_body`](Self::spawn_body)
    /// (for example, a hash of what the body displays).
    fn body_revision(&self) -> u64 {
        0
    }
}

/// Context handed to [`NodeDataTrait::spawn_body`].
pub struct NodeBodyContext<'a, N: NodeDataTrait> {
    /// The editor entity (the one holding the `NodeGraphEditor` component).
    pub editor: Entity,
    pub node_id: NodeId,
    pub graph: &'a Graph<N, N::DataType, N::ValueType>,
    pub style: &'a NodeGraphStyle,
}

impl<N: NodeDataTrait> NodeBodyContext<'_, N> {
    /// A [`TextFont`] matching the editor's style.
    pub fn text_font(&self) -> TextFont {
        self.style.text_font(self.style.font_size)
    }
}

/// A kind of node the user can create from the node finder.
pub trait NodeTemplateTrait: Clone + Send + Sync + 'static {
    type NodeData: NodeDataTrait;

    /// Label shown in the node finder.
    fn node_finder_label(&self) -> Cow<'_, str>;

    /// Categories shown as section headers in the node finder.
    fn node_finder_categories(&self) -> Vec<&'static str> {
        Vec::new()
    }

    /// Initial label of created nodes. Defaults to the finder label.
    fn node_graph_label(&self) -> String {
        self.node_finder_label().into_owned()
    }

    /// The user data stored in created nodes.
    fn user_data(&self) -> Self::NodeData;

    /// Adds this template's parameters to the freshly created `node_id`.
    fn build_node(&self, graph: &mut GraphOf<Self::NodeData>, node_id: NodeId);
}

/// Ties together the types of one kind of graph. Implement it on a marker
/// type and register [`NodeGraphPlugin::<YourSchema>`](crate::NodeGraphPlugin).
///
/// ```ignore
/// struct MathGraph;
/// impl NodeGraphSchema for MathGraph {
///     type NodeData = MyNodeData;
///     type NodeTemplate = MyNodeTemplate;
/// }
/// ```
pub trait NodeGraphSchema: Send + Sync + 'static {
    type NodeData: NodeDataTrait;
    type NodeTemplate: NodeTemplateTrait<NodeData = Self::NodeData>;
}

/// The [`Graph`] type for a given node data type.
pub type GraphOf<N> = Graph<N, <N as NodeDataTrait>::DataType, <N as NodeDataTrait>::ValueType>;
/// The [`DataTypeTrait`] of a schema.
pub type DataTypeOf<S> = <<S as NodeGraphSchema>::NodeData as NodeDataTrait>::DataType;
/// The [`WidgetValueTrait`] of a schema.
pub type ValueTypeOf<S> = <<S as NodeGraphSchema>::NodeData as NodeDataTrait>::ValueType;
/// The [`Graph`] type of a schema.
pub type SchemaGraph<S> = GraphOf<<S as NodeGraphSchema>::NodeData>;

/// Describes the inline editor of an input value. See [`WidgetValueTrait`].
#[derive(Clone, Debug, PartialEq)]
pub enum ValueWidget {
    /// No widget: only the parameter name is shown.
    None,
    /// Read-only text next to the parameter name.
    Label(String),
    /// An editable text field.
    Text { value: String, multiline: bool },
    /// A numeric field. Drag the parameter name sideways to scrub the value.
    Number(NumberField),
    /// Several numeric fields on one row, e.g. the components of a vector.
    Numbers(Vec<NumberField>),
    /// A checkbox.
    Bool(bool),
    /// Cycles through `options` with ‹ › buttons.
    Choice {
        options: Vec<String>,
        selected: usize,
    },
}

/// An edit produced by a [`ValueWidget`].
#[derive(Clone, Debug, PartialEq)]
pub enum ValueEdit {
    Text(String),
    /// `component` is always 0 for [`ValueWidget::Number`], and the index of the
    /// field for [`ValueWidget::Numbers`].
    Number {
        component: usize,
        value: f64,
    },
    Bool(bool),
    Choice(usize),
}

/// A numeric field inside a [`ValueWidget`].
#[derive(Clone, Debug, PartialEq)]
pub struct NumberField {
    pub value: f64,
    /// Short label drawn before the field (e.g. `"x"`).
    pub label: Option<String>,
    pub min: f64,
    pub max: f64,
    /// Value change per logical pixel when scrubbing.
    pub speed: f64,
    /// Digits after the decimal point. `0` turns this into an integer field.
    pub decimals: usize,
}

impl NumberField {
    pub fn new(value: f64) -> Self {
        Self {
            value,
            label: None,
            min: f64::NEG_INFINITY,
            max: f64::INFINITY,
            speed: 0.01,
            decimals: 3,
        }
    }

    /// An integer field (no decimals, whole-number scrubbing).
    pub fn integer(value: i64) -> Self {
        Self {
            speed: 0.1,
            decimals: 0,
            ..Self::new(value as f64)
        }
    }

    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn range(mut self, min: f64, max: f64) -> Self {
        self.min = min;
        self.max = max;
        self
    }

    pub fn speed(mut self, speed: f64) -> Self {
        self.speed = speed;
        self
    }

    pub fn decimals(mut self, decimals: usize) -> Self {
        self.decimals = decimals;
        self
    }

    /// Whether `value` differs from the current value by more than the
    /// precision of the underlying storage (values often round-trip through `f32`).
    pub fn differs_from(&self, value: f64) -> bool {
        (value - self.value).abs() > 1e-6 * value.abs().max(self.value.abs()).max(1.0)
    }

    /// Clamps (and for integer fields, rounds) a candidate value.
    pub fn sanitize(&self, value: f64) -> f64 {
        let value = if self.decimals == 0 {
            value.round()
        } else {
            value
        };
        value.clamp(self.min, self.max)
    }

    /// The value as displayed in the field.
    pub fn format(&self) -> String {
        format_number(self.value, self.decimals)
    }
}

pub(crate) fn format_number(value: f64, decimals: usize) -> String {
    if decimals == 0 {
        return format!("{value:.0}");
    }
    let text = format!("{value:.decimals$}");
    // Trim trailing zeros but keep one decimal so floats still read as floats.
    let trimmed = text.trim_end_matches('0');
    if trimmed.ends_with('.') {
        format!("{trimmed}0")
    } else {
        trimmed.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn number_formatting_trims_trailing_zeros() {
        assert_eq!(format_number(1.5, 3), "1.5");
        assert_eq!(format_number(2.0, 3), "2.0");
        assert_eq!(format_number(-0.126, 2), "-0.13");
        assert_eq!(format_number(7.6, 0), "8");
    }

    #[test]
    fn number_field_sanitizes() {
        let field = NumberField::integer(3).range(0.0, 10.0);
        assert_eq!(field.sanitize(4.4), 4.0);
        assert_eq!(field.sanitize(42.0), 10.0);
        let float = NumberField::new(0.0).range(-1.0, 1.0);
        assert_eq!(float.sanitize(0.25), 0.25);
    }

    #[test]
    fn f32_round_trips_do_not_count_as_changes() {
        let field = NumberField::new(2.09_f32 as f64);
        assert!(!field.differs_from(2.09));
        assert!(field.differs_from(2.1));
    }
}
