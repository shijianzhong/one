use gpui::{
    App, ClipboardItem, Context, CursorStyle, EntityInputHandler, FocusHandle,
    Focusable, InteractiveElement, MouseButton, MouseDownEvent, MouseUpEvent, Pixels,
    ParentElement, Render, SharedString, Styled, Window, actions, div, hsla, white,
};
use std::ops::Range;

actions!(
    text_input,
    [
        Backspace,
        Delete,
        Left,
        Right,
        SelectLeft,
        SelectRight,
        SelectAll,
        Home,
        End,
        Paste,
        Cut,
        Copy,
        Enter,
        Escape,
    ]
);

pub struct TextInput {
    focus_handle: FocusHandle,
    pub value: SharedString,
    placeholder: SharedString,
    selected_range: Range<usize>,
    selection_reversed: bool,
    marked_range: Option<Range<usize>>,
    is_selecting: bool,
}

impl TextInput {
    pub fn new(value: &str, placeholder: &str, cx: &mut Context<Self>) -> Self {
        let value_owned = value.to_string();
        Self {
            focus_handle: cx.focus_handle(),
            value: value_owned.clone().into(),
            placeholder: placeholder.into(),
            selected_range: 0..value_owned.len(),
            selection_reversed: false,
            marked_range: None,
            is_selecting: false,
        }
    }

    pub fn new_mut(value: SharedString, placeholder: &str, cx: &mut Context<Self>) -> Self {
        let value_len = value.len();
        Self {
            focus_handle: cx.focus_handle(),
            value,
            placeholder: placeholder.into(),
            selected_range: 0..value_len,
            selection_reversed: false,
            marked_range: None,
            is_selecting: false,
        }
    }

    pub fn get_value(&self) -> &str {
        &self.value
    }

    pub fn set_value(&mut self, value: SharedString) {
        self.value = value;
        self.selected_range = 0..self.value.len();
    }

    fn left(&mut self, _: &Left, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.move_to(self.previous_boundary(self.cursor_offset()));
        } else {
            self.move_to(self.selected_range.start);
        }
        cx.notify();
    }

    fn right(&mut self, _: &Right, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.move_to(self.next_boundary(self.selected_range.end));
        } else {
            self.move_to(self.selected_range.end);
        }
        cx.notify();
    }

    fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.previous_boundary(self.cursor_offset()), cx);
    }

    fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.next_boundary(self.cursor_offset()), cx);
    }

    fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0);
        self.select_to(self.value.len(), cx);
    }

    fn home(&mut self, _: &Home, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0);
        cx.notify();
    }

    fn end(&mut self, _: &End, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.value.len());
        cx.notify();
    }

    fn enter(&mut self, _: &Enter, _: &mut Window, cx: &mut Context<Self>) {
        cx.notify();
    }

    fn escape(&mut self, _: &Escape, _: &mut Window, cx: &mut Context<Self>) {
        cx.notify();
    }

    fn backspace(&mut self, _: &Backspace, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            let prev = self.previous_boundary(self.cursor_offset());
            if prev < self.cursor_offset() {
                self.selected_range = prev..self.cursor_offset();
            }
        }
        self.delete_selected(cx);
    }

    fn delete(&mut self, _: &Delete, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            let next = self.next_boundary(self.cursor_offset());
            if self.cursor_offset() < next {
                self.selected_range = self.cursor_offset()..next;
            }
        }
        self.delete_selected(cx);
    }

    fn delete_selected(&mut self, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() {
            self.value = (self.value[..self.selected_range.start].to_owned()
                + &self.value[self.selected_range.end..])
                .into();
            self.move_to(self.selected_range.start);
        }
        cx.notify();
    }

    fn paste(&mut self, _: &Paste, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            self.replace_text_in_range_internal(None, &text.replace("\n", " "), cx);
        }
    }

    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.value[self.selected_range.clone()].to_string(),
            ));
        }
    }

    fn cut(&mut self, _: &Cut, _: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.value[self.selected_range.clone()].to_string(),
            ));
        }
        self.delete_selected(cx);
    }

    fn move_to(&mut self, offset: usize) {
        self.selected_range = offset..offset;
        self.selection_reversed = false;
        self.marked_range.take();
    }

    fn select_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        if offset < self.cursor_offset() {
            self.selection_reversed = true;
            self.selected_range = offset..self.cursor_offset();
        } else {
            self.selection_reversed = false;
            self.selected_range = self.cursor_offset()..offset;
        }
        cx.notify();
    }

    fn cursor_offset(&self) -> usize {
        if self.selection_reversed {
            self.selected_range.start
        } else {
            self.selected_range.end
        }
    }

    fn previous_boundary(&self, offset: usize) -> usize {
        self.value
            .char_indices()
            .rev()
            .find_map(|(idx, _)| (idx < offset).then_some(idx))
            .unwrap_or(0)
    }

    fn next_boundary(&self, offset: usize) -> usize {
        self.value
            .char_indices()
            .find_map(|(idx, c)| {
                let next_idx = idx + c.len_utf8();
                (next_idx > offset).then_some(next_idx)
            })
            .unwrap_or(self.value.len())
    }

    fn on_mouse_down(&mut self, event: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.is_selecting = true;
        let char_count = self.value.chars().count();
        let char_width = 8.0;
        let offset = ((event.position.x.as_f32() / char_width) as usize)
            .min(char_count)
            .max(0);

        let mut char_idx = 0;
        let mut byte_offset = 0;
        for (i, _c) in self.value.char_indices() {
            if char_idx >= offset {
                break;
            }
            char_idx += 1;
            byte_offset = i;
        }
        let offset_bytes = if char_idx >= offset { self.value.len() } else { byte_offset };

        if event.modifiers.shift {
            self.select_to(offset_bytes, cx);
        } else {
            self.move_to(offset_bytes);
            cx.notify();
        }
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.is_selecting = false;
    }

    fn on_mouse_move(&mut self, event: &gpui::MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.is_selecting {
            let char_count = self.value.chars().count();
            let char_width = 8.0;
            let offset = ((event.position.x.as_f32() / char_width) as usize)
                .min(char_count)
                .max(0);

            let mut char_idx = 0;
            let mut byte_offset = 0;
            for (i, _c) in self.value.char_indices() {
                if char_idx >= offset {
                    break;
                }
                char_idx += 1;
                byte_offset = i;
            }
            let offset_bytes = if char_idx >= offset { self.value.len() } else { byte_offset };

            self.select_to(offset_bytes, cx);
        }
    }

    fn replace_text_in_range_internal(
        &mut self,
        range: Option<Range<usize>>,
        new_text: &str,
        cx: &mut Context<Self>,
    ) {
        let range = range
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());

        self.value =
            (self.value[0..range.start].to_owned() + new_text + &self.value[range.end..]).into();
        self.selected_range = range.start + new_text.len()..range.start + new_text.len();
        self.marked_range.take();
        cx.notify();
    }
}

impl EntityInputHandler for TextInput {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        adjusted_range: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        *adjusted_range = Some(range.clone());
        Some(self.value[range].to_string())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<gpui::UTF16Selection> {
        Some(gpui::UTF16Selection {
            range: self.selected_range.clone(),
            reversed: self.selection_reversed,
        })
    }

    fn marked_text_range(
        &self,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        self.marked_range.clone()
    }

    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.marked_range.take();
        cx.notify();
    }

    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.replace_text_in_range_internal(range, text, cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());

        self.value =
            (self.value[0..range.start].to_owned() + new_text + &self.value[range.end..]).into();
        if !new_text.is_empty() {
            self.marked_range = Some(range.start..range.start + new_text.len());
        } else {
            self.marked_range.take();
        }
        self.selected_range = new_selected_range_utf16
            .map(|r| r.start + range.start..r.end + range.end)
            .unwrap_or_else(|| range.start + new_text.len()..range.start + new_text.len());
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        _: Range<usize>,
        _: gpui::Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<gpui::Bounds<Pixels>> {
        None
    }

    fn character_index_for_point(
        &mut self,
        _: gpui::Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        None
    }
}

impl Render for TextInput {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl gpui::IntoElement {
        let value = self.value.clone();
        let placeholder = self.placeholder.clone();

        div()
            .key_context("TextInput")
            .track_focus(&self.focus_handle(cx))
            .cursor(CursorStyle::IBeam)
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::left))
            .on_action(cx.listener(Self::right))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::home))
            .on_action(cx.listener(Self::end))
            .on_action(cx.listener(Self::enter))
            .on_action(cx.listener(Self::escape))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::cut))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .flex_1()
            .h_full()
            .px_3()
            .text_sm()
            .bg(white())
            .child(
                if value.is_empty() {
                    div().text_sm().text_color(hsla(0., 0., 0., 0.4)).child(placeholder)
                } else {
                    div().text_sm().text_color(hsla(0., 0., 0., 1.)).child(value)
                }
            )
    }
}

impl Focusable for TextInput {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}
