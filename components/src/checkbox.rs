//! Checkbox widget implementation.

use gpui::{
    actions, App, Context, CursorStyle, FocusHandle, Focusable, InteractiveElement,
    MouseButton, MouseDownEvent, MouseUpEvent, ParentElement, Render, Styled, Window, px,
};

use crate::traits::state::ComponentState;

actions!(
    checkbox,
    [
        Toggle,
    ]
);

pub struct Checkbox {
    label: gpui::SharedString,
    state: ComponentState,
    checked: bool,
    indeterminate: bool,
    disabled: bool,
    focus_handle: FocusHandle,
    on_toggle: Option<Box<dyn Fn(bool, &mut Window, &mut Context<Checkbox>) + 'static>>,
}

impl Checkbox {
    pub fn new(label: impl Into<gpui::SharedString>, cx: &mut Context<Self>) -> Self {
        Self {
            label: label.into(),
            state: ComponentState::new(),
            checked: false,
            indeterminate: false,
            disabled: false,
            focus_handle: cx.focus_handle(),
            on_toggle: None,
        }
    }

    pub fn checked(mut self, checked: bool) -> Self {
        self.checked = checked;
        if checked {
            self.indeterminate = false;
        }
        self
    }

    pub fn indeterminate(mut self, indeterminate: bool) -> Self {
        self.indeterminate = indeterminate;
        if indeterminate {
            self.checked = false;
        }
        self
    }

    pub fn on_toggle<F>(mut self, callback: F) -> Self
    where
        F: Fn(bool, &mut Window, &mut Context<Checkbox>) + 'static,
    {
        self.on_toggle = Some(Box::new(callback));
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self.state.set_disabled(disabled);
        self
    }

    pub fn is_checked(&self) -> bool {
        self.checked
    }

    pub fn is_indeterminate(&self) -> bool {
        self.indeterminate
    }
}

impl Checkbox {
    fn handle_mouse_down(&mut self, _: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        if !self.disabled {
            self.state.press();
            cx.notify();
        }
    }

    fn handle_mouse_up(&mut self, _: &MouseUpEvent, window: &mut Window, cx: &mut Context<Self>) {
        if !self.disabled {
            self.state.release();
            if !self.indeterminate {
                self.checked = !self.checked;
            } else {
                self.indeterminate = false;
                self.checked = true;
            }
            if let Some(ref callback) = self.on_toggle {
                callback(self.checked, window, cx);
            }
            cx.notify();
        }
    }
}

impl Render for Checkbox {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl gpui::IntoElement {
        let box_bg = if self.disabled {
            gpui::hsla(0., 0., 0.9, 1.)
        } else if self.checked || self.indeterminate {
            gpui::hsla(0.6, 1., 0.5, 1.)
        } else {
            gpui::hsla(0., 0., 1., 1.)
        };

        let mark_color = if self.checked || self.indeterminate {
            gpui::hsla(0., 0., 1., 1.)
        } else {
            gpui::hsla(0., 0., 0., 0.)
        };

        let cursor = if self.disabled {
            CursorStyle::OperationNotAllowed
        } else {
            CursorStyle::PointingHand
        };

        gpui::div()
            .id("checkbox")
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .cursor(cursor)
            .on_mouse_down(MouseButton::Left, cx.listener(Self::handle_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::handle_mouse_up))
            .child(
                gpui::div()
                    .size(gpui::px(18.0))
                    .bg(box_bg)
                    .rounded_sm()
                    .border(px(1.0))
                    .border_color(gpui::hsla(0., 0., 0.6, 1.))
                    .child(
                        if self.indeterminate {
                            gpui::div().w_1().h_0p5().bg(mark_color)
                        } else if self.checked {
                            gpui::div().w_full().h_0p5().bg(mark_color)
                        } else {
                            gpui::div()
                        },
                    ),
            )
            .child(
                gpui::div()
                    .text_sm()
                    .text_color(if self.disabled {
                        gpui::hsla(0., 0., 0.5, 1.)
                    } else {
                        gpui::hsla(0., 0., 0., 1.)
                    })
                    .child(self.label.clone()),
            )
    }
}

impl Focusable for Checkbox {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}
