//! Base Component trait for reusable UI elements.
//!
//! The `Component` trait is the foundation for building stateful widgets
//! in the one_gpui component library. It follows the Elm architecture
//! pattern adapted for GPUI's Entity/Context system.
//!
//! # Lifecycle
//!
//! 1. **Creation**: Component is created with initial state
//! 2. **Event Handling**: User interactions dispatch Messages
//! 3. **Update**: `update()` processes messages and mutates state
//! 4. **Render**: `Render` trait produces UI elements
//! 5. **Re-render**: `cx.notify()` triggers re-render when state changes
//!
//! # Implementing a Component
//!
//! ```ignore
//! use gpui::{actions, Context, Render, Styled};
//! use crate::traits::{Component, ComponentState};
//!
//! actions!(counter, [Increment, Decrement]);
//!
//! pub struct Counter {
//!     state: ComponentState,
//!     value: i32,
//! }
//!
//! impl Counter {
//!     pub fn new(initial: i32, cx: &mut Context<Self>) -> Self {
//!         Self {
//!             state: ComponentState::default(),
//!             value: initial,
//!         }
//!     }
//! }
//!
//! impl Component for Counter {
//!     type Message = Message;
//!
//!     fn update(&mut self, message: Self::Message, cx: &mut Context<Self>) {
//!         match message {
//!             Message::Increment => self.value += 1,
//!             Message::Decrement => self.value -= 1,
////!         }
//!         cx.notify();
//!     }
//! }
//!
//! impl Render for Counter {
//!     fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
//!         div()
//!             .child(text(format!("Count: {}", self.value)))
//!             .on_click(cx.listener(|this, _, _, cx| {
//!                 this.state = ComponentState::Pressed;
//!                 cx.notify();
//!             }))
//!     }
//! }
//! ```

use gpui::Context;

/// Message type that represents all possible user interactions with a component.
///
/// Components define their own Message type using the `actions!` macro,
/// then implement `update()` to handle each message variant.
///
/// # Example
///
/// ```ignore
/// actions!(my_component, [
///     Click,
///     Hover(bool),      // true = entered, false = left
///     ValueChanged(String),
/// ]);
/// ```
pub trait Message: Sized + 'static {
    /// Returns the component to its default state.
    fn reset(&mut self);
}

/// Component state tracking for visual feedback.
///
/// Tracks the current interaction state of a component:
/// - Default: Normal idle state
/// - Hovered: Mouse is over the component
/// - Pressed: Mouse button is held down
/// - Focused: Component has keyboard focus
#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub enum InteractionState {
    /// Component is in its normal idle state
    #[default]
    Default,
    /// Mouse is hovering over the component
    Hovered,
    /// Mouse button is pressed on the component
    Pressed,
    /// Component has keyboard focus
    Focused,
    /// Both hovered and focused
    HoveredFocused,
    /// Component is disabled and cannot be interacted with
    Disabled,
}

impl InteractionState {
    /// Returns true if the component can receive input.
    pub fn is_enabled(&self) -> bool {
        !matches!(self, Self::Disabled)
    }

    /// Returns true if the component is currently being interacted with.
    pub fn is_interacting(&self) -> bool {
        matches!(self, Self::Pressed | Self::Hovered | Self::HoveredFocused)
    }

    /// Returns true if the component should show a focus indicator.
    pub fn should_show_focus(&self) -> bool {
        matches!(self, Self::Focused | Self::HoveredFocused)
    }
}

/// Base trait for all components in the one_gpui component library.
///
/// Components are self-contained UI elements that manage their own state
/// and respond to user interactions through a message-based system.
///
/// # Type Parameters
///
/// - `M`: The Message type defining all possible interactions
///
/// # Associated Types
///
/// - `Message`: All user interactions this component can produce
///
/// # Lifecycle Methods
///
/// - `update()`: Process a message and update component state
/// - `view()`: (via Render trait) Produce the UI representation
///
/// # Default Implementations
///
/// - `interaction_state()`: Returns the current interaction state
/// - `set_interaction_state()`: Updates the interaction state
pub trait Component: Sized {
    /// The message type for all user interactions.
    type Message: Message;

    /// Creates a new component with the given initial state.
    fn new(cx: &mut Context<Self>) -> Self;

    /// Processes a message and updates the component's internal state.
    ///
    /// This is called by the framework when a user interaction occurs.
    /// Subclasses should implement this to handle their specific messages.
    fn update(&mut self, message: Self::Message, cx: &mut Context<Self>);

    /// Returns the current interaction state of the component.
    fn interaction_state(&self) -> InteractionState {
        InteractionState::Default
    }

    /// Sets the interaction state of the component.
    ///
    /// This is called by the framework to update the visual state
    /// based on mouse/keyboard activity.
    fn set_interaction_state(&mut self, _state: InteractionState) {}
}
