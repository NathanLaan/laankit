//! The diagram edit commands: what a shortcut, a button, a menu or a palette
//! entry can ask the editor to do, and their registrations, so BlockLiner
//! and NoteLiner list the same commands with the same keys.
//!
//! [`commands`] returns them without an app's types: the app wraps each
//! [`EditAct`] in its own action and maps each [`When`] to a predicate over
//! its own view, then registers them with laankit-egui's registry.

use blockliner_core::style::{Dash, Weight, PRESETS, WEIGHTS};
use blockliner_core::ShapeKind;
use egui::{Key, Modifiers};
use laankit_egui::commands::{Binding, Command};

use crate::canvas::{Swatch, Tool, STROKE_STYLES};
use crate::tools::{tool_digit, tool_key};

/// An edit of the open diagram, or of what's selected in it.
#[derive(Clone, Debug, PartialEq)]
pub enum EditAct {
    Tool(Tool),
    /// The More Shapes button's shape: the last of [`Tool::MORE`] used.
    MoreShapes,
    Undo,
    Redo,
    SelectAll,
    /// Every shape but the text boxes…
    SelectShapes,
    /// …every connector…
    SelectLines,
    /// …or every text box.
    SelectText,
    /// The selection to the clipboard, as Mermaid…
    Copy,
    /// …and deleted…
    Cut,
    /// …and back: asks for the clipboard, which arrives as egui's paste
    /// event.
    Paste,
    Delete,
    /// Edit the single selected shape's or connector's label.
    EditLabel,
    /// The selected connectors become right-angle…
    RouteElbow,
    /// …or straight.
    RouteStraight,
    /// The selected shapes become this kind (Change Shape).
    ChangeShape(ShapeKind),
    /// The selected lines become what this line tool draws (Change Line).
    ChangeLine(Tool),
    /// The selected lines get an arrowhead…
    AddArrowheads,
    /// …or lose it.
    RemoveArrowheads,
    /// The selected shapes' fill…
    Fill(Swatch),
    /// …or stroke.
    Stroke(Swatch),
    /// Solid (`None`), Dashed or Dotted.
    StrokeStyle(Option<Dash>),
    /// A weight (`None`: the default, 1px).
    StrokeWeight(Option<Weight>),
    /// Bold (`true`) or Normal…
    TextWeight(bool),
    /// …Italic or Normal…
    TextStyle(bool),
    /// …and Ctrl+B: Normal when all are Bold, else all Bold.
    ToggleBold,
    /// Fill, stroke and text back to the theme's.
    ResetStyle,
    /// Move the selection by this many grid steps (the arrow keys go
    /// through `DiagramEditor::keys`, which also reads Shift and Alt).
    Nudge(i8, i8),
    /// Cancel the drag in progress, else clear the selection.
    Escape,
    ZoomIn,
    ZoomOut,
    ZoomReset,
}

/// What the edit commands' [`When`]s read: the open diagram's selection and
/// undo history.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EditView {
    pub has_selection: bool,
    /// Selected connectors include a straight one…
    pub straight_selected: bool,
    /// …or a right-angle one.
    pub elbow_selected: bool,
    /// Selected shapes or lines can take a stroke…
    pub stylable: bool,
    /// …and selected shapes a fill.
    pub fillable: bool,
    /// The selection is one shape or Mermaid edge whose label can be edited.
    pub labellable: bool,
    /// A selected shape can change kind (one BlockLiner declares).
    pub changeable: bool,
    /// Selected lines include one without an arrowhead…
    pub plain_line_selected: bool,
    /// …or one with.
    pub arrow_selected: bool,
    pub can_undo: bool,
    pub can_redo: bool,
}

/// When an edit command applies. The app adds its own conditions (no dialog
/// open, a diagram on screen).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum When {
    /// Whenever the app takes commands.
    Ready,
    /// A diagram is open.
    Editing,
    Selected,
    HasStraight,
    HasElbow,
    Stylable,
    Fillable,
    Labellable,
    Changeable,
    HasLine,
    HasPlainLine,
    HasArrow,
    Undoable,
    Redoable,
}

impl When {
    /// Whether it holds for `v`. [`When::Ready`] and [`When::Editing`] are
    /// the app's to decide, and hold here.
    pub fn holds(self, v: &EditView) -> bool {
        match self {
            When::Ready | When::Editing => true,
            When::Selected => v.has_selection,
            When::HasStraight => v.straight_selected,
            When::HasElbow => v.elbow_selected,
            When::Stylable => v.stylable,
            When::Fillable => v.fillable,
            When::Labellable => v.labellable,
            When::Changeable => v.changeable,
            When::HasLine => v.straight_selected || v.elbow_selected,
            When::HasPlainLine => v.plain_line_selected,
            When::HasArrow => v.arrow_selected,
            When::Undoable => v.can_undo,
            When::Redoable => v.can_redo,
        }
    }
}

/// An edit command for an app to register: its id, label, section, keys and
/// shortcut text (in `command`), and when it applies.
#[derive(Clone)]
pub struct EditCommand {
    pub command: Command<EditAct>,
    pub when: When,
}

impl EditCommand {
    fn key(mut self, binding: Binding) -> Self {
        self.command = self.command.key(binding);
        self
    }

    fn shortcut(mut self, text: impl Into<String>) -> Self {
        self.command = self.command.shortcut(text);
        self
    }

    fn when(mut self, when: When) -> Self {
        self.when = when;
        self
    }

    /// The command for an app's registry: `wrap` makes the app's action and
    /// `when` its predicate.
    pub fn into_command<A, C>(
        self,
        wrap: impl FnOnce(EditAct) -> A,
        when: impl FnOnce(When) -> fn(&C) -> bool,
    ) -> Command<A, C> {
        let c = self.command;
        Command {
            id: c.id,
            label: c.label,
            section: c.section,
            shortcut: c.shortcut,
            bindings: c.bindings,
            when: Some(when(self.when)),
            action: wrap(c.action),
        }
    }
}

/// The commands, in the order they're registered.
struct Commands(Vec<EditCommand>);

impl Commands {
    fn register(&mut self, command: EditCommand) {
        self.0.push(command);
    }
}

/// The edit commands, in registration order: the tools, then Edit and Style.
/// The zoom commands are left to the app, which chooses their keys.
pub fn commands() -> Vec<EditCommand> {
    let mut r = Commands(Vec::new());
    let cmd = Modifiers::COMMAND;
    let shift = cmd | Modifiers::SHIFT;
    let add = |id: &str, label: &str, section: &str, act: EditAct| EditCommand {
        command: Command::new(id, label, section, act),
        when: When::Ready,
    };
    let ready = When::Ready;
    let editing = When::Editing;
    let selected = When::Selected;
    let has_straight = When::HasStraight;
    let has_elbow = When::HasElbow;
    let stylable = When::Stylable;
    let fillable = When::Fillable;
    let labellable = When::Labellable;
    let changeable = When::Changeable;
    let has_line = When::HasLine;
    let has_plain_line = When::HasPlainLine;
    let has_arrow = When::HasArrow;
    let undoable = When::Undoable;
    let redoable = When::Redoable;
    for tool in Tool::ALL {
        let letter = tool_key(tool);
        let (digit, number) = tool_digit(tool);
        let id: String = tool
            .name()
            .to_lowercase()
            .chars()
            .filter(char::is_ascii_alphanumeric)
            .collect();
        let id = format!("tool.{id}");
        let mut command = add(
            &id,
            &format!("{} Tool", tool.name()),
            "Tools",
            EditAct::Tool(tool),
        )
        .when(ready);
        if let Some((key, _)) = letter {
            command = command
                .key(Binding::logical(Modifiers::NONE, key))
                // With Shift the tool stays picked after each shape.
                .key(Binding::logical(Modifiers::SHIFT, key));
        }
        // The more shapes' number is More Shapes'.
        r.register(match (tool.is_more(), letter) {
            (true, Some((_, letter))) => command.shortcut(letter),
            (true, None) => command,
            (false, letter) => command
                .key(Binding::physical(Modifiers::NONE, digit))
                .key(Binding::physical(Modifiers::SHIFT, digit))
                .shortcut(match letter {
                    Some((_, letter)) => format!("{letter}, {number}"),
                    None => number.to_owned(),
                }),
        });
    }
    let (digit, number) = tool_digit(Tool::MORE[0]);
    r.register(
        add(
            "tool.moreShapes",
            "More Shapes Tool",
            "Tools",
            EditAct::MoreShapes,
        )
        .key(Binding::physical(Modifiers::NONE, digit))
        .key(Binding::physical(Modifiers::SHIFT, digit))
        .shortcut(number)
        .when(ready),
    );
    r.register(
        add("edit.undo", "Undo", "Edit", EditAct::Undo)
            .key(Binding::logical(cmd, Key::Z))
            .shortcut("Ctrl+Z")
            .when(undoable),
    );
    r.register(
        add("edit.redo", "Redo", "Edit", EditAct::Redo)
            .key(Binding::logical(shift, Key::Z))
            .key(Binding::logical(cmd, Key::Y))
            .shortcut("Ctrl+Shift+Z")
            .when(redoable),
    );
    r.register(
        add("edit.selectAll", "Select All", "Edit", EditAct::SelectAll)
            .key(Binding::logical(cmd, Key::A))
            .shortcut("Ctrl+A")
            .when(ready),
    );
    // Physical keys, as for the other Shift and Alt shortcuts.
    let alt = cmd | Modifiers::ALT;
    for (id, label, binding, keys, act) in [
        (
            "edit.selectShapes",
            "Select All Shapes",
            Binding::physical(shift, Key::A),
            "Ctrl+Shift+A",
            EditAct::SelectShapes,
        ),
        (
            "edit.selectLines",
            "Select All Lines",
            Binding::physical(alt, Key::A),
            "Ctrl+Alt+A",
            EditAct::SelectLines,
        ),
        (
            "edit.selectText",
            "Select All Text",
            Binding::physical(shift, Key::T),
            "Ctrl+Shift+T",
            EditAct::SelectText,
        ),
    ] {
        r.register(
            add(id, label, "Edit", act)
                .key(binding)
                .shortcut(keys)
                .when(ready),
        );
    }
    // No key bindings: egui turns the platform's copy, cut and paste keys
    // into its clipboard events, which `DiagramEditor::keys` reads.
    for (id, label, keys, act, when) in [
        ("edit.cut", "Cut", "Ctrl+X", EditAct::Cut, selected),
        ("edit.copy", "Copy", "Ctrl+C", EditAct::Copy, selected),
        ("edit.paste", "Paste", "Ctrl+V", EditAct::Paste, editing),
    ] {
        r.register(add(id, label, "Edit", act).shortcut(keys).when(when));
    }
    // No key bindings: the arrow keys are read in `DiagramEditor::keys`, which
    // also handles Shift (5 steps) and Alt (1 px). These entries put them in
    // the palette and the shortcuts list.
    for (id, label, keys, (dx, dy)) in [
        ("edit.nudgeLeft", "Nudge Left", "←", (-1, 0)),
        ("edit.nudgeRight", "Nudge Right", "→", (1, 0)),
        ("edit.nudgeUp", "Nudge Up", "↑", (0, -1)),
        ("edit.nudgeDown", "Nudge Down", "↓", (0, 1)),
    ] {
        r.register(
            add(id, label, "Edit", EditAct::Nudge(dx, dy))
                .shortcut(keys)
                .when(selected),
        );
    }
    r.register(
        add(
            "edit.routeElbow",
            "Make Connectors Right-Angle",
            "Edit",
            EditAct::RouteElbow,
        )
        .when(has_straight),
    );
    r.register(
        add(
            "edit.routeStraight",
            "Make Connectors Straight",
            "Edit",
            EditAct::RouteStraight,
        )
        .when(has_elbow),
    );
    // Change Shape and arrowheads: palette-only (and the right-click menu
    // and the Properties pane). The letters already pick drawing tools.
    for kind in ShapeKind::CHANGEABLE {
        let id: String = kind
            .name()
            .to_lowercase()
            .chars()
            .filter(char::is_ascii_alphanumeric)
            .collect();
        let label = format!("Change Shape: {}", kind.name());
        r.register(
            add(
                &format!("edit.changeShape.{id}"),
                &label,
                "Edit",
                EditAct::ChangeShape(kind),
            )
            .when(changeable),
        );
    }
    for tool in Tool::LINES {
        let id: String = tool
            .name()
            .to_lowercase()
            .chars()
            .filter(char::is_ascii_alphanumeric)
            .collect();
        r.register(
            add(
                &format!("edit.changeLine.{id}"),
                &format!("Change Line: {}", tool.name()),
                "Edit",
                EditAct::ChangeLine(tool),
            )
            .when(has_line),
        );
    }
    r.register(
        add(
            "edit.addArrowheads",
            "Add Arrowheads",
            "Edit",
            EditAct::AddArrowheads,
        )
        .when(has_plain_line),
    );
    r.register(
        add(
            "edit.removeArrowheads",
            "Remove Arrowheads",
            "Edit",
            EditAct::RemoveArrowheads,
        )
        .when(has_arrow),
    );
    // Colours: palette-only (and the right-click menu).
    for (part, make, when) in [
        ("Fill", EditAct::Fill as fn(Swatch) -> EditAct, fillable),
        ("Stroke", EditAct::Stroke, stylable),
    ] {
        let mut swatch = |id: String, name: &str, s: Swatch| {
            let label = format!("{part}: {name}");
            r.register(add(&id, &label, "Style", make(s)).when(when));
        };
        let key = part.to_lowercase();
        swatch(format!("style.{key}.default"), "Default", Swatch::Default);
        for (i, preset) in PRESETS.iter().enumerate() {
            let id = format!("style.{key}.{}", preset.name.to_lowercase());
            swatch(id, preset.name, Swatch::Preset(i));
        }
        swatch(format!("style.{key}.custom"), "Custom…", Swatch::Custom);
    }
    for dash in STROKE_STYLES {
        let name = Dash::name(dash.as_ref());
        r.register(
            add(
                &format!("style.strokeStyle.{}", name.to_lowercase()),
                &format!("Stroke Style: {name}"),
                "Style",
                EditAct::StrokeStyle(dash),
            )
            .when(stylable),
        );
    }
    for weight in WEIGHTS {
        let id = match &weight {
            Some(w) => w.field().replace('.', "_"),
            None => Weight::number_text(Weight::DEFAULT).replace('.', "_"),
        };
        r.register(
            add(
                &format!("style.strokeWeight.{id}"),
                &format!("Stroke Weight: {}", Weight::label(weight.as_ref())),
                "Style",
                EditAct::StrokeWeight(weight),
            )
            .when(stylable),
        );
    }
    for (id, label, act) in [
        (
            "style.textWeight.normal",
            "Text Weight: Normal",
            EditAct::TextWeight(false),
        ),
        (
            "style.textWeight.bold",
            "Text Weight: Bold",
            EditAct::TextWeight(true),
        ),
        (
            "style.textStyle.normal",
            "Text Style: Normal",
            EditAct::TextStyle(false),
        ),
        (
            "style.textStyle.italic",
            "Text Style: Italic",
            EditAct::TextStyle(true),
        ),
    ] {
        r.register(add(id, label, "Style", act).when(stylable));
    }
    r.register(
        add(
            "style.toggleBold",
            "Toggle Bold",
            "Style",
            EditAct::ToggleBold,
        )
        .key(Binding::logical(cmd, Key::B))
        .shortcut("Ctrl+B")
        .when(stylable),
    );
    r.register(add("style.reset", "Reset Style", "Style", EditAct::ResetStyle).when(stylable));
    r.register(
        add("edit.label", "Edit Label", "Edit", EditAct::EditLabel)
            .key(Binding::logical(Modifiers::NONE, Key::Enter))
            .shortcut("Enter")
            .when(labellable),
    );
    r.register(
        add("edit.delete", "Delete", "Edit", EditAct::Delete)
            .key(Binding::logical(Modifiers::NONE, Key::Delete))
            .key(Binding::logical(Modifiers::NONE, Key::Backspace))
            .shortcut("Delete")
            .when(selected),
    );
    r.register(
        add("edit.escape", "Clear Selection", "Edit", EditAct::Escape)
            .key(Binding::logical(Modifiers::NONE, Key::Escape))
            .shortcut("Esc")
            .when(ready),
    );
    r.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_tool_has_a_command_and_ids_are_unique() {
        let all = commands();
        for tool in Tool::ALL {
            assert!(
                all.iter().any(|c| c.command.action == EditAct::Tool(tool)),
                "{tool:?}"
            );
        }
        for (i, c) in all.iter().enumerate() {
            assert!(
                all[i + 1..].iter().all(|o| o.command.id != c.command.id),
                "{} is registered twice",
                c.command.id
            );
        }
    }

    #[test]
    fn whens_read_the_view() {
        let none = EditView::default();
        assert!(When::Ready.holds(&none) && When::Editing.holds(&none));
        assert!(!When::Selected.holds(&none) && !When::HasLine.holds(&none));
        let v = EditView {
            elbow_selected: true,
            can_undo: true,
            ..Default::default()
        };
        assert!(When::HasLine.holds(&v) && When::HasElbow.holds(&v));
        assert!(!When::HasStraight.holds(&v));
        assert!(When::Undoable.holds(&v) && !When::Redoable.holds(&v));
    }
}
