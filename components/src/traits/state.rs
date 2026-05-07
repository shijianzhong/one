//! Component state management utilities.

use gpui::FocusHandle;

/// State tracking for visual/interaction feedback on components.
#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub enum InteractionState {
    #[default]
    Default,
    Hovered,
    Pressed,
    Focused,
    HoveredFocused,
    Disabled,
}

impl InteractionState {
    pub fn is_enabled(&self) -> bool {
        !matches!(self, Self::Disabled)
    }

    pub fn is_interacting(&self) -> bool {
        matches!(self, Self::Pressed | Self::Hovered | Self::HoveredFocused)
    }

    pub fn should_show_focus(&self) -> bool {
        matches!(self, Self::Focused | Self::HoveredFocused)
    }
}

/// State information for a component.
#[derive(Default, Clone)]
pub struct ComponentState {
    pub interaction: InteractionState,
    pub dirty: bool,
    focus_handle: Option<FocusHandle>,
}

impl ComponentState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn focus_handle(&self) -> &Option<FocusHandle> {
        &self.focus_handle
    }

    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    pub fn clear_dirty(&mut self) {
        self.dirty = false;
    }

    pub fn set_interaction(&mut self, new_state: InteractionState) {
        self.interaction = new_state;
        self.mark_dirty();
    }

    pub fn hover(&mut self) {
        if self.interaction.is_enabled() {
            self.interaction = match self.interaction {
                InteractionState::Focused => InteractionState::HoveredFocused,
                _ => InteractionState::Hovered,
            };
            self.mark_dirty();
        }
    }

    pub fn unhover(&mut self) {
        self.interaction = match self.interaction {
            InteractionState::HoveredFocused => InteractionState::Focused,
            InteractionState::Hovered => InteractionState::Default,
            other => other,
        };
        self.mark_dirty();
    }

    pub fn press(&mut self) {
        if self.interaction.is_enabled() {
            self.interaction = match self.interaction {
                InteractionState::HoveredFocused
                | InteractionState::Hovered
                | InteractionState::Focused
                | InteractionState::Default => InteractionState::Pressed,
                other => other,
            };
            self.mark_dirty();
        }
    }

    pub fn release(&mut self) {
        self.interaction = match self.interaction {
            InteractionState::Pressed => InteractionState::Default,
            other => other,
        };
        self.mark_dirty();
    }

    pub fn set_focused(&mut self, focused: bool) {
        self.interaction = if focused {
            match self.interaction {
                InteractionState::Hovered => InteractionState::HoveredFocused,
                InteractionState::Pressed | InteractionState::Default => InteractionState::Focused,
                other => other,
            }
        } else {
            match self.interaction {
                InteractionState::HoveredFocused => InteractionState::Hovered,
                InteractionState::Focused => InteractionState::Default,
                other => other,
            }
        };
        self.mark_dirty();
    }

    pub fn set_disabled(&mut self, disabled: bool) {
        self.interaction = if disabled {
            InteractionState::Disabled
        } else {
            InteractionState::Default
        };
        self.mark_dirty();
    }
}

impl std::fmt::Debug for ComponentState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ComponentState")
            .field("interaction", &self.interaction)
            .field("dirty", &self.dirty)
            .finish()
    }
}
