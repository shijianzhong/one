//! Button widget implementation.

use gpui::{
    actions, App, Context, CursorStyle, FocusHandle, Focusable, InteractiveElement, MouseButton,
    MouseDownEvent, MouseUpEvent, ParentElement, Render, Styled, Window,
};

use crate::traits::state::ComponentState;

actions!(button, [Press, Release, Click,]);

pub struct Button {
    label: gpui::SharedString,
    state: ComponentState,
    disabled: bool,
    focus_handle: FocusHandle,
    on_press: Option<Box<dyn Fn(&mut Window, &mut Context<Button>) + 'static>>,
}

impl Button {
    pub fn new(label: impl Into<gpui::SharedString>, cx: &mut Context<Self>) -> Self {
        Self {
            label: label.into(),
            state: ComponentState::new(),
            disabled: false,
            focus_handle: cx.focus_handle(),
            on_press: None,
        }
    }

    pub fn on_press<F>(mut self, callback: F) -> Self
    where
        F: Fn(&mut Window, &mut Context<Button>) + 'static,
    {
        self.on_press = Some(Box::new(callback));
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self.state.set_disabled(disabled);
        self
    }

    pub fn is_disabled(&self) -> bool {
        self.disabled
    }
}

impl Button {
    fn handle_mouse_down(&mut self, _: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        if !self.disabled {
            self.state.press();
            cx.notify();
        }
    }

    fn handle_mouse_up(&mut self, _: &MouseUpEvent, window: &mut Window, cx: &mut Context<Self>) {
        if !self.disabled {
            self.state.release();
            if let Some(ref callback) = self.on_press {
                callback(window, cx);
            }
            cx.notify();
        }
    }
}

impl Render for Button {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl gpui::IntoElement {
        let interaction = self.state.interaction;

        let bg_color = if self.disabled {
            gpui::hsla(0., 0., 0.85, 1.)
        } else {
            match interaction {
                crate::traits::InteractionState::Pressed => gpui::hsla(0.6, 1., 0.5, 1.),
                crate::traits::InteractionState::Hovered
                | crate::traits::InteractionState::HoveredFocused => gpui::hsla(0.6, 1., 0.7, 1.),
                _ => gpui::hsla(0.6, 1., 0.8, 1.),
            }
        };

        let cursor = if self.disabled {
            CursorStyle::OperationNotAllowed
        } else {
            CursorStyle::PointingHand
        };

        gpui::div()
            .id("button")
            .flex()
            .items_center()
            .justify_center()
            .px_4()
            .py_2()
            .gap_2()
            .bg(bg_color)
            .rounded_md()
            .cursor(cursor)
            .on_mouse_down(MouseButton::Left, cx.listener(Self::handle_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::handle_mouse_up))
            .child(self.label.clone())
    }
}

impl Focusable for Button {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}
