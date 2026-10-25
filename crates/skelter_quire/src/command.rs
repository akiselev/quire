//! What a key binding resolves to: an editor command (pure, in `quire-core`) or a widget action
//! (view chrome the widget owns, such as opening the find bar).

use quire_core::EditorCommand;

/// Actions the widget performs itself (no document change).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WidgetAction {
    /// `actions.find`: open the find bar and focus its input.
    OpenFind,
    /// `editor.action.startFindReplaceAction`: open the find bar with the replace row.
    OpenReplace,
    /// `closeFindWidget`.
    CloseFind,
}

impl WidgetAction {
    /// The VS Code id.
    pub fn id(self) -> &'static str {
        match self {
            Self::OpenFind => "actions.find",
            Self::OpenReplace => "editor.action.startFindReplaceAction",
            Self::CloseFind => "closeFindWidget",
        }
    }

    fn from_id(id: &str) -> Option<Self> {
        [Self::OpenFind, Self::OpenReplace, Self::CloseFind]
            .into_iter()
            .find(|a| a.id() == id)
    }
}

/// A resolved key binding.
#[derive(Clone, Debug)]
pub enum KeyAction {
    Command(EditorCommand),
    Widget(WidgetAction),
}

impl KeyAction {
    /// Resolve a `keybindings.json` command id and optional args. Editor commands deserialize
    /// from `{"id": command, ...args}` (quire-core's serde form); the widget's own ids follow.
    pub fn resolve(command: &str, args: Option<&serde_json::Value>) -> Result<Self, String> {
        // VS Code's argument-free ids (cursorLeftSelect, deleteWordLeft, ...). Page motions get
        // their line count from the viewport when dispatched.
        if args.is_none()
            && let Some(c) = EditorCommand::from_vscode_id(command, 0)
        {
            return Ok(Self::Command(c));
        }
        let mut obj = match args {
            Some(serde_json::Value::Object(m)) => m.clone(),
            Some(other) => {
                return Err(format!(
                    "args of {command:?} must be an object, got {other}"
                ));
            }
            None => serde_json::Map::new(),
        };
        obj.insert("id".into(), serde_json::Value::String(command.into()));
        match serde_json::from_value::<EditorCommand>(serde_json::Value::Object(obj)) {
            Ok(c) => Ok(Self::Command(c)),
            Err(e) => WidgetAction::from_id(command)
                .map(Self::Widget)
                .ok_or_else(|| e.to_string()),
        }
    }
}
