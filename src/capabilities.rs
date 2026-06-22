use editor::Editor;
use gpui::{
    div, prelude::*, px, relative, AnyElement, ClipboardItem, Context, Div, InteractiveElement,
    ParentElement, Styled, Window,
};

use crate::i18n::{t, Translations};
use crate::ui_theme::{
    ACCENT_TEXT, BORDER_LIGHT, BRAND_BLUE, CANVAS_BG, GHOST_SURFACE_BG, MUTED_TEXT, PRIMARY_TEXT,
    SECONDARY_TEXT, SURFACE_PANEL,
};
use crate::workflows::{WorkflowDefinition, WorkflowEdge, WorkflowNode, WorkflowNodeKind};
use crate::{AppState, CapabilitiesTab, ToastLevel, WorkflowEditState};

pub(crate) fn render_capabilities_titlebar(
    app: &AppState,
    _window: &mut Window,
    cx: &mut Context<AppState>,
) -> AnyElement {
    let lang = app.current_lang;
    let active_tab = app.capabilities_tab;
    let library_active = active_tab == CapabilitiesTab::Library;
    let workflows_active = active_tab == CapabilitiesTab::Workflows;

    div()
        .flex()
        .items_center()
        .justify_between()
        .h_full()
        .px_8()
        .child(
            div()
                .flex()
                .items_center()
                .gap_3()
                .child(
                    div()
                        .text_lg()
                        .text_color(PRIMARY_TEXT())
                        .font_weight(gpui::FontWeight::BOLD)
                        .child(t(lang, Translations::CAPABILITIES)),
                )
                .child(
                    div()
                        .px_2()
                        .py_0p5()
                        .rounded_md()
                        .bg(GHOST_SURFACE_BG())
                        .text_size(px(10.0))
                        .text_color(BRAND_BLUE())
                        .font_weight(gpui::FontWeight::BOLD)
                        .child("ALPHA"),
                ),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap_3()
                .px_4()
                .py_0p5()
                .rounded_xl()
                .bg(GHOST_SURFACE_BG())
                .border_1()
                .border_color(BORDER_LIGHT())
                .child(tab_button(
                    app,
                    t(lang, Translations::CAPABILITY_LIBRARY),
                    library_active,
                    CapabilitiesTab::Library,
                    cx,
                ))
                .child(tab_button(
                    app,
                    t(lang, Translations::WORKFLOWS),
                    workflows_active,
                    CapabilitiesTab::Workflows,
                    cx,
                )),
        )
        .into_any_element()
}

pub(crate) fn render_capabilities(
    app: &AppState,
    window: &mut Window,
    cx: &mut Context<AppState>,
) -> AnyElement {
    let lang = app.current_lang;
    let capabilities = crate::workflows::capability_manifests();

    let mut content = div()
        .flex_1()
        .h_full()
        .overflow_hidden()
        .bg(CANVAS_BG())
        .px_10()
        .py_10()
        .flex_col()
        .gap_6();

    match app.capabilities_tab {
        CapabilitiesTab::Library => {
            content = content.child(
                div()
                    .flex()
                    .items_start()
                    .justify_between()
                    .gap_6()
                    .child(
                        div()
                            .flex_col()
                            .gap_2()
                            .child(
                                div()
                                    .text_2xl()
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .text_color(PRIMARY_TEXT())
                                    .child(t(lang, Translations::PUBLISHED_CAPABILITIES)),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .line_height(relative(1.5))
                                    .text_color(SECONDARY_TEXT())
                                    .child(t(lang, Translations::CAPABILITIES_HINT)),
                            ),
                    )
                    .child(import_capability_panel(app, window, cx)),
            );

            if capabilities.is_empty() {
                content = content.child(empty_state(
                    "capabilities",
                    t(lang, Translations::NO_CAPABILITIES),
                    t(lang, Translations::CAPABILITIES_HINT),
                ));
            } else {
                let mut grid = div().flex().flex_wrap().gap_4();
                for capability in capabilities {
                    grid = grid.child(capability_card(app, window, capability, cx));
                }
                content = content.child(grid);
            }

            content = content.child(recent_runs_panel(app, window, cx));
        }
        CapabilitiesTab::Workflows => {
            let workflows = load_workflow_drafts();
            let selected_definition = workflows
                .as_ref()
                .ok()
                .and_then(|items| {
                    app.editing_workflow_id
                        .as_deref()
                        .and_then(|editing_id| {
                            items
                                .iter()
                                .find(|workflow| workflow.id.as_str() == editing_id)
                        })
                        .or_else(|| items.first())
                })
                .and_then(|workflow| load_workflow_definition(&workflow.id).ok());
            let pending_canvas_events = crate::workflow_webview::drain_workflow_canvas_events();
            if !pending_canvas_events.is_empty() {
                let fallback_workflow_id = selected_definition
                    .as_ref()
                    .map(|definition| definition.id.clone());
                cx.defer_in(window, move |this, _window, cx| {
                    for event in pending_canvas_events {
                        match event {
                            crate::workflow_webview::WorkflowCanvasEvent::NodeSelected {
                                workflow_id,
                                node_id,
                            } => {
                                let workflow_id = workflow_id
                                    .or_else(|| fallback_workflow_id.clone())
                                    .unwrap_or_default();
                                if workflow_id.is_empty() {
                                    continue;
                                }
                                this.editing_workflow_id = Some(workflow_id.clone());
                                this.selected_workflow_id = Some(workflow_id);
                                this.selected_workflow_node_id = Some(node_id);
                            }
                            crate::workflow_webview::WorkflowCanvasEvent::EdgeCreated {
                                workflow_id,
                                source_node_id,
                                target_node_id,
                            } => {
                                let workflow_id = workflow_id
                                    .or_else(|| fallback_workflow_id.clone())
                                    .unwrap_or_default();
                                if workflow_id.is_empty() {
                                    continue;
                                }
                                match create_workflow_edge(
                                    &workflow_id,
                                    &source_node_id,
                                    &target_node_id,
                                ) {
                                    Ok(edge_id) => {
                                        mark_workflow_dirty(
                                            &mut this.workflow_edit_states,
                                            &workflow_id,
                                            format!("Created route {}", edge_id),
                                        );
                                        this.push_toast(
                                            ToastLevel::Success,
                                            format!("Created route {}", edge_id),
                                            cx,
                                        );
                                    }
                                    Err(err) => this.push_toast(
                                        ToastLevel::Error,
                                        format!("Failed to create route: {}", err),
                                        cx,
                                    ),
                                }
                            }
                            crate::workflow_webview::WorkflowCanvasEvent::EdgeDeleted {
                                workflow_id,
                                edge_id,
                            } => {
                                let workflow_id = workflow_id
                                    .or_else(|| fallback_workflow_id.clone())
                                    .unwrap_or_default();
                                if workflow_id.is_empty() {
                                    continue;
                                }
                                match delete_workflow_edge(&workflow_id, &edge_id) {
                                    Ok(()) => {
                                        mark_workflow_dirty(
                                            &mut this.workflow_edit_states,
                                            &workflow_id,
                                            format!("Deleted route {}", edge_id),
                                        );
                                        this.push_toast(
                                            ToastLevel::Success,
                                            format!("Deleted route {}", edge_id),
                                            cx,
                                        );
                                    }
                                    Err(err) => this.push_toast(
                                        ToastLevel::Error,
                                        format!("Failed to delete route: {}", err),
                                        cx,
                                    ),
                                }
                            }
                            crate::workflow_webview::WorkflowCanvasEvent::Error {
                                workflow_id,
                                message,
                            } => {
                                let workflow_id = workflow_id
                                    .or_else(|| fallback_workflow_id.clone())
                                    .unwrap_or_default();
                                if !workflow_id.is_empty() {
                                    mark_workflow_error(
                                        &mut this.workflow_edit_states,
                                        &workflow_id,
                                        message.clone(),
                                    );
                                }
                                this.push_toast(
                                    ToastLevel::Error,
                                    format!("Workflow canvas error: {}", message),
                                    cx,
                                );
                            }
                        }
                    }
                    cx.notify();
                });
            }
            let canvas_workflow = selected_definition.as_ref().map(|definition| {
                crate::workflow_webview::CanvasWorkflow::from_definition_with_statuses(
                    definition,
                    app.workflow_node_run_statuses.get(&definition.id),
                )
            });

            content = content.child(
                div()
                    .flex()
                    .items_start()
                    .justify_between()
                    .gap_6()
                    .child(
                        div()
                            .flex_col()
                            .gap_2()
                            .child(
                                div()
                                    .text_2xl()
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .text_color(PRIMARY_TEXT())
                                    .child(t(lang, Translations::WORKFLOW_BUILDER)),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .line_height(relative(1.5))
                                    .text_color(SECONDARY_TEXT())
                                    .child(t(lang, Translations::WORKFLOWS_HINT)),
                            ),
                    )
                    .child(workflow_template_gallery(cx)),
            );

            content = content.child(workflow_builder_shell(
                app,
                selected_definition.as_ref(),
                canvas_workflow,
                window,
                cx,
            ));

            match workflows {
                Ok(workflows) if workflows.is_empty() => {
                    content = content.child(empty_state(
                        "run-panel",
                        t(lang, Translations::NO_WORKFLOWS),
                        t(lang, Translations::WORKFLOWS_HINT),
                    ));
                }
                Ok(workflows) => {
                    let mut list = div().flex_col().gap_3();
                    for workflow in workflows {
                        list = list.child(workflow_card(app, window, workflow, cx));
                    }
                    content = content.child(list);
                }
                Err(err) => {
                    content = content.child(
                        div()
                            .p_5()
                            .rounded_xl()
                            .border_1()
                            .border_color(BORDER_LIGHT())
                            .bg(SURFACE_PANEL())
                            .text_sm()
                            .text_color(crate::ui_theme::ERROR_TEXT())
                            .child(format!("Failed to load workflows: {}", err)),
                    );
                }
            }
        }
    }

    content.into_any_element()
}

fn workflow_builder_shell(
    app: &AppState,
    definition: Option<&WorkflowDefinition>,
    workflow: Option<crate::workflow_webview::CanvasWorkflow>,
    window: &mut Window,
    cx: &mut Context<AppState>,
) -> impl IntoElement {
    let publish_definition = definition.cloned();
    let add_agent_definition = definition.cloned();
    let save_definition = definition.cloned();
    let run_definition = definition.cloned();
    let copilot_definition = definition.cloned();
    let copilot_brief_editor = workflow_copilot_brief_editor(window, cx);
    let weak_copilot_brief = copilot_brief_editor.downgrade();
    let save_status = definition
        .map(|definition| workflow_edit_state_label(app, &definition.id))
        .unwrap_or_else(|| "No workflow selected".to_string());

    div()
        .w_full()
        .min_h(px(410.0))
        .rounded_xl()
        .border_1()
        .border_color(BORDER_LIGHT())
        .bg(SURFACE_PANEL())
        .overflow_hidden()
        .flex_col()
        .child(
            div()
                .h(px(44.0))
                .px_4()
                .flex()
                .items_center()
                .justify_between()
                .border_b_1()
                .border_color(BORDER_LIGHT())
                .child(
                    div()
                        .flex_col()
                        .gap_0p5()
                        .child(
                            div()
                                .text_sm()
                                .font_weight(gpui::FontWeight::BOLD)
                                .text_color(PRIMARY_TEXT())
                                .child("Workflow Builder"),
                        )
                        .child(div().text_xs().text_color(MUTED_TEXT()).child(format!(
                            "{} · {}",
                            crate::workflow_webview::webview_status_label(),
                            save_status
                        ))),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(small_button("Publish", cx, move |this, cx| {
                            if let Some(definition) = publish_definition.as_ref() {
                                match publish_workflow_as_capability(
                                    &definition.id,
                                    &definition.name,
                                    &definition.description,
                                ) {
                                    Ok(capability_id) => this.push_toast(
                                        ToastLevel::Success,
                                        format!("Published capability {}", capability_id),
                                        cx,
                                    ),
                                    Err(err) => this.push_toast(
                                        ToastLevel::Error,
                                        format!("Failed to publish workflow: {}", err),
                                        cx,
                                    ),
                                }
                            } else {
                                this.push_toast(
                                    ToastLevel::Info,
                                    "Create or select a workflow before publishing.".to_string(),
                                    cx,
                                );
                            }
                            cx.notify();
                        }))
                        .child(small_button("Add Agent", cx, move |this, cx| {
                            if let Some(definition) = add_agent_definition.as_ref() {
                                match append_empty_agent_node(&definition.id) {
                                    Ok(node_id) => {
                                        this.editing_workflow_id = Some(definition.id.clone());
                                        mark_workflow_dirty(
                                            &mut this.workflow_edit_states,
                                            &definition.id,
                                            format!("Added local Agent {}", node_id),
                                        );
                                        this.push_toast(
                                            ToastLevel::Success,
                                            format!("Added local Agent {}", node_id),
                                            cx,
                                        );
                                    }
                                    Err(err) => this.push_toast(
                                        ToastLevel::Error,
                                        format!("Failed to add Agent: {}", err),
                                        cx,
                                    ),
                                }
                            } else {
                                this.push_toast(
                                    ToastLevel::Info,
                                    "Create or select a workflow before adding agents.".to_string(),
                                    cx,
                                );
                            }
                            cx.notify();
                        }))
                        .child(small_button("Save", cx, move |this, cx| {
                            if let Some(definition) = save_definition.as_ref() {
                                match validate_and_save_workflow_definition(&definition.id) {
                                    Ok(()) => {
                                        mark_workflow_saved(
                                            &mut this.workflow_edit_states,
                                            &definition.id,
                                        );
                                        this.push_toast(
                                            ToastLevel::Success,
                                            format!("Saved workflow {}", definition.id),
                                            cx,
                                        );
                                    }
                                    Err(err) => {
                                        mark_workflow_save_failed(
                                            &mut this.workflow_edit_states,
                                            &definition.id,
                                            err.to_string(),
                                        );
                                        this.push_toast(
                                            ToastLevel::Error,
                                            format!("Failed to save workflow: {}", err),
                                            cx,
                                        );
                                    }
                                }
                            } else {
                                this.push_toast(
                                    ToastLevel::Info,
                                    "Create or select a workflow before saving.".to_string(),
                                    cx,
                                );
                            }
                            cx.notify();
                        }))
                        .child(small_button("Run", cx, move |this, cx| {
                            if let Some(definition) = run_definition.as_ref() {
                                let workflow_id = definition.id.clone();
                                this.push_toast(
                                    ToastLevel::Info,
                                    format!("Running draft workflow {}...", workflow_id),
                                    cx,
                                );
                                cx.spawn(async move |this, cx| {
                                    let result = run_workflow_draft(workflow_id.clone()).await;
                                    let _ = this.update(cx, |this, cx| match result {
                                        Ok(value) => {
                                            this.workflow_node_run_statuses.insert(
                                                workflow_id.clone(),
                                                workflow_node_statuses_from_run(&value),
                                            );
                                            this.push_toast(
                                                ToastLevel::Success,
                                                format!(
                                                    "Draft workflow finished: {}",
                                                    value["status"]
                                                ),
                                                cx,
                                            );
                                        }
                                        Err(err) => this.push_toast(
                                            ToastLevel::Error,
                                            format!("Draft workflow failed: {}", err),
                                            cx,
                                        ),
                                    });
                                })
                                .detach();
                            } else {
                                this.push_toast(
                                    ToastLevel::Info,
                                    "Create or select a workflow before running.".to_string(),
                                    cx,
                                );
                            }
                            cx.notify();
                        }))
                        .child(small_button("AI Copilot", cx, move |this, cx| {
                            let brief = weak_copilot_brief
                                .upgrade()
                                .map(|editor| editor.read_with(cx, |editor, cx| editor.text(cx)))
                                .unwrap_or_default();
                            if brief.trim().is_empty() {
                                this.push_toast(
                                    ToastLevel::Info,
                                    "Describe the workflow you want Copilot to generate."
                                        .to_string(),
                                    cx,
                                );
                                cx.notify();
                                return;
                            }
                            let source_workflow_id = copilot_definition
                                .as_ref()
                                .map(|definition| definition.id.clone());
                            this.push_toast(
                                ToastLevel::Info,
                                "Generating workflow draft with AI Copilot...".to_string(),
                                cx,
                            );
                            cx.spawn(async move |this, cx| {
                                let result =
                                    create_workflow_from_copilot_brief(&brief, source_workflow_id)
                                        .await;
                                let _ = this.update(cx, |this, cx| match result {
                                    Ok(workflow_id) => {
                                        this.editing_workflow_id = Some(workflow_id.clone());
                                        this.selected_workflow_id = Some(workflow_id.clone());
                                        this.selected_workflow_node_id = None;
                                        mark_workflow_dirty(
                                            &mut this.workflow_edit_states,
                                            &workflow_id,
                                            "Generated by AI Copilot",
                                        );
                                        this.push_toast(
                                            ToastLevel::Success,
                                            format!("Generated workflow draft {}", workflow_id),
                                            cx,
                                        );
                                    }
                                    Err(err) => this.push_toast(
                                        ToastLevel::Error,
                                        format!("AI Copilot failed: {}", err),
                                        cx,
                                    ),
                                });
                            })
                            .detach();
                            cx.notify();
                        })),
                ),
        )
        .child(
            div()
                .px_4()
                .py_1()
                .border_b_1()
                .border_color(BORDER_LIGHT())
                .flex()
                .items_center()
                .gap_3()
                .child(
                    div()
                        .text_xs()
                        .font_weight(gpui::FontWeight::BOLD)
                        .text_color(MUTED_TEXT())
                        .child("Copilot brief"),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.0))
                        .px_3()
                        .py_0p5()
                        .rounded_md()
                        .border_1()
                        .border_color(BORDER_LIGHT())
                        .bg(CANVAS_BG())
                        .text_sm()
                        .text_color(PRIMARY_TEXT())
                        .child(copilot_brief_editor),
                ),
        )
        .child(
            div()
                .flex_1()
                .min_h(px(332.0))
                .flex()
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.0))
                        .overflow_hidden()
                        .bg(CANVAS_BG())
                        .child(
                            crate::workflow_webview::workflow_canvas_poc(workflow)
                                .w_full()
                                .h_full()
                                .bg(CANVAS_BG()),
                        ),
                )
                .child(workflow_inspector_panel(
                    definition,
                    app.selected_workflow_id.as_deref(),
                    app.selected_workflow_node_id.as_deref(),
                    window,
                    cx,
                )),
        )
}

fn workflow_inspector_panel(
    definition: Option<&WorkflowDefinition>,
    selected_workflow_id: Option<&str>,
    selected_node_id: Option<&str>,
    window: &mut Window,
    cx: &mut Context<AppState>,
) -> impl IntoElement {
    let mut panel = div()
        .w(px(300.0))
        .flex_none()
        .p_4()
        .border_l_1()
        .border_color(BORDER_LIGHT())
        .bg(GHOST_SURFACE_BG())
        .flex_col()
        .gap_4();

    if let Some(definition) = definition {
        let selected_node = selected_node_id
            .filter(|_| selected_workflow_id == Some(definition.id.as_str()))
            .and_then(|node_id| {
                definition
                    .nodes
                    .iter()
                    .find(|node| node.id.as_str() == node_id)
            });
        if let Some(node) = selected_node {
            let workflow_id = definition.id.clone();
            let node_id = node.id.clone();
            let name_editor = workflow_agent_field_editor(
                &workflow_id,
                &node_id,
                "name",
                &node.name,
                false,
                window,
                cx,
            );
            let description_editor = workflow_agent_field_editor(
                &workflow_id,
                &node_id,
                "description",
                &workflow_node_config_string(node, &["description"], ""),
                true,
                window,
                cx,
            );
            let category_editor = workflow_agent_field_editor(
                &workflow_id,
                &node_id,
                "category",
                &workflow_node_config_string(node, &["metadata", "category"], ""),
                false,
                window,
                cx,
            );
            let tags_editor = workflow_agent_field_editor(
                &workflow_id,
                &node_id,
                "tags",
                &workflow_node_config_tags(node),
                false,
                window,
                cx,
            );
            let version_editor = workflow_agent_field_editor(
                &workflow_id,
                &node_id,
                "version",
                &workflow_node_config_string(node, &["metadata", "version"], "0.1.0"),
                false,
                window,
                cx,
            );
            let model_provider_editor = workflow_agent_field_editor(
                &workflow_id,
                &node_id,
                "model_provider",
                &workflow_node_config_string(node, &["model", "provider"], "default"),
                false,
                window,
                cx,
            );
            let model_name_editor = workflow_agent_field_editor(
                &workflow_id,
                &node_id,
                "model_name",
                &workflow_node_config_string(node, &["model", "model"], "default"),
                false,
                window,
                cx,
            );
            let temperature_editor = workflow_agent_field_editor(
                &workflow_id,
                &node_id,
                "temperature",
                &workflow_node_config_number(node, &["model", "temperature"], "0.2"),
                false,
                window,
                cx,
            );
            let max_tokens_editor = workflow_agent_field_editor(
                &workflow_id,
                &node_id,
                "max_tokens",
                &workflow_node_config_number(node, &["model", "max_tokens"], "4096"),
                false,
                window,
                cx,
            );
            let timeout_editor = workflow_agent_field_editor(
                &workflow_id,
                &node_id,
                "timeout",
                &workflow_node_config_number(node, &["model", "timeout_seconds"], "120"),
                false,
                window,
                cx,
            );
            let system_prompt_editor = workflow_agent_field_editor(
                &workflow_id,
                &node_id,
                "system_prompt",
                &workflow_node_config_string(node, &["prompt", "system"], ""),
                true,
                window,
                cx,
            );
            let instructions_editor = workflow_agent_field_editor(
                &workflow_id,
                &node_id,
                "instructions",
                &workflow_node_config_string(node, &["prompt", "instructions"], ""),
                true,
                window,
                cx,
            );
            let output_format_editor = workflow_agent_field_editor(
                &workflow_id,
                &node_id,
                "output_format",
                &workflow_node_config_string(node, &["output", "format"], "text"),
                false,
                window,
                cx,
            );
            let output_schema_editor = workflow_agent_field_editor(
                &workflow_id,
                &node_id,
                "output_schema",
                &workflow_node_config_json_text(node, &["output", "schema"], "null"),
                true,
                window,
                cx,
            );
            let summarize_editor = workflow_agent_field_editor(
                &workflow_id,
                &node_id,
                "summarize",
                &workflow_node_config_bool(node, &["output", "summarize_with_mainagent"], true)
                    .to_string(),
                false,
                window,
                cx,
            );
            let skills_editor = workflow_agent_field_editor(
                &workflow_id,
                &node_id,
                "skills",
                &workflow_node_config_json_text(node, &["tools", "skills"], "[]"),
                true,
                window,
                cx,
            );
            let mcp_tools_editor = workflow_agent_field_editor(
                &workflow_id,
                &node_id,
                "mcp_tools",
                &workflow_node_config_json_text(node, &["tools", "mcp_tools"], "[]"),
                true,
                window,
                cx,
            );
            let system_tools_editor = workflow_agent_field_editor(
                &workflow_id,
                &node_id,
                "system_tools",
                &workflow_node_config_json_text(node, &["tools", "system_tools"], "[]"),
                true,
                window,
                cx,
            );
            let coding_runtimes_editor = workflow_agent_field_editor(
                &workflow_id,
                &node_id,
                "coding_runtimes",
                &workflow_node_config_json_text(node, &["tools", "coding_runtimes"], "[]"),
                true,
                window,
                cx,
            );
            let retry_editor = workflow_agent_field_editor(
                &workflow_id,
                &node_id,
                "retry",
                &workflow_node_config_number(node, &["settings", "retry"], "0"),
                false,
                window,
                cx,
            );
            let settings_timeout_editor = workflow_agent_field_editor(
                &workflow_id,
                &node_id,
                "settings_timeout",
                &workflow_node_config_number(node, &["settings", "timeout_seconds"], "120"),
                false,
                window,
                cx,
            );
            let human_confirmation_editor = workflow_agent_field_editor(
                &workflow_id,
                &node_id,
                "human_confirmation",
                &workflow_node_config_bool(node, &["settings", "human_confirmation"], false)
                    .to_string(),
                false,
                window,
                cx,
            );
            let routing_policy_editor = workflow_agent_field_editor(
                &workflow_id,
                &node_id,
                "routing_policy",
                &workflow_node_config_json_text(node, &["routing"], r#"{"mode":"sequential"}"#),
                true,
                window,
                cx,
            );
            let permissions_editor = workflow_agent_field_editor(
                &workflow_id,
                &node_id,
                "permissions",
                &workflow_node_config_string(node, &["settings", "permissions"], "ask"),
                false,
                window,
                cx,
            );

            let weak_name = name_editor.downgrade();
            let weak_description = description_editor.downgrade();
            let weak_category = category_editor.downgrade();
            let weak_tags = tags_editor.downgrade();
            let weak_version = version_editor.downgrade();
            let weak_model_provider = model_provider_editor.downgrade();
            let weak_model_name = model_name_editor.downgrade();
            let weak_temperature = temperature_editor.downgrade();
            let weak_max_tokens = max_tokens_editor.downgrade();
            let weak_timeout = timeout_editor.downgrade();
            let weak_system_prompt = system_prompt_editor.downgrade();
            let weak_instructions = instructions_editor.downgrade();
            let weak_output_format = output_format_editor.downgrade();
            let weak_output_schema = output_schema_editor.downgrade();
            let weak_summarize = summarize_editor.downgrade();
            let weak_skills = skills_editor.downgrade();
            let weak_mcp_tools = mcp_tools_editor.downgrade();
            let weak_system_tools = system_tools_editor.downgrade();
            let weak_coding_runtimes = coding_runtimes_editor.downgrade();
            let weak_retry = retry_editor.downgrade();
            let weak_settings_timeout = settings_timeout_editor.downgrade();
            let weak_human_confirmation = human_confirmation_editor.downgrade();
            let weak_routing_policy = routing_policy_editor.downgrade();
            let weak_permissions = permissions_editor.downgrade();
            let save_workflow_id = workflow_id.clone();
            let save_node_id = node_id.clone();

            return panel
                .child(
                    div()
                        .flex_col()
                        .gap_1()
                        .child(
                            div()
                                .text_sm()
                                .font_weight(gpui::FontWeight::BOLD)
                                .text_color(PRIMARY_TEXT())
                                .child("Agent Inspector"),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(MUTED_TEXT())
                                .child("Workflow-local node instance"),
                        ),
                )
                .child(workflow_inspector_row(
                    "Node",
                    format!("{} / {}", node.id, workflow_node_kind_label(&node.kind)),
                ))
                .child(
                    workflow_editor_section("Basic")
                        .child(workflow_editor_field("Name", name_editor, false))
                        .child(workflow_editor_field(
                            "Description",
                            description_editor,
                            true,
                        ))
                        .child(workflow_editor_field("Category", category_editor, false))
                        .child(workflow_editor_field("Tags", tags_editor, false))
                        .child(workflow_editor_field("Version", version_editor, false)),
                )
                .child(
                    workflow_editor_section("Model")
                        .child(workflow_editor_field(
                            "Provider",
                            model_provider_editor,
                            false,
                        ))
                        .child(workflow_editor_field("Model", model_name_editor, false))
                        .child(workflow_editor_field(
                            "Temperature",
                            temperature_editor,
                            false,
                        ))
                        .child(workflow_editor_field(
                            "Max tokens",
                            max_tokens_editor,
                            false,
                        ))
                        .child(workflow_editor_field(
                            "Timeout seconds",
                            timeout_editor,
                            false,
                        )),
                )
                .child(
                    workflow_editor_section("Prompt")
                        .child(workflow_editor_field("System", system_prompt_editor, true))
                        .child(workflow_editor_field(
                            "Instructions",
                            instructions_editor,
                            true,
                        )),
                )
                .child(
                    workflow_editor_section("Output")
                        .child(workflow_editor_field("Format", output_format_editor, false))
                        .child(workflow_editor_field(
                            "Schema JSON",
                            output_schema_editor,
                            true,
                        ))
                        .child(workflow_editor_field(
                            "Summarize with MainAgent",
                            summarize_editor,
                            false,
                        )),
                )
                .child(
                    workflow_editor_section("Tools")
                        .child(workflow_editor_field("Skills JSON", skills_editor, true))
                        .child(workflow_editor_field(
                            "MCP tools JSON",
                            mcp_tools_editor,
                            true,
                        ))
                        .child(workflow_editor_field(
                            "System tools JSON",
                            system_tools_editor,
                            true,
                        ))
                        .child(workflow_editor_field(
                            "Coding runtimes JSON",
                            coding_runtimes_editor,
                            true,
                        )),
                )
                .child(
                    workflow_editor_section("Settings")
                        .child(workflow_editor_field("Retry", retry_editor, false))
                        .child(workflow_editor_field(
                            "Timeout seconds",
                            settings_timeout_editor,
                            false,
                        ))
                        .child(workflow_editor_field(
                            "Human confirmation",
                            human_confirmation_editor,
                            false,
                        ))
                        .child(workflow_editor_field(
                            "Routing policy JSON",
                            routing_policy_editor,
                            true,
                        ))
                        .child(workflow_editor_field(
                            "Permissions",
                            permissions_editor,
                            false,
                        ))
                        .child(workflow_inspector_row(
                            "Routing",
                            workflow_node_routing_mode(node),
                        ))
                        .child(workflow_inspector_row(
                            "Tools",
                            workflow_node_tool_summary(node),
                        )),
                )
                .child(small_button("Save Agent", cx, move |this, cx| {
                    let read = |editor: &gpui::WeakEntity<Editor>, cx: &mut Context<AppState>| {
                        editor
                            .upgrade()
                            .map(|editor| editor.read_with(cx, |editor, cx| editor.text(cx)))
                            .unwrap_or_default()
                    };
                    let update = WorkflowAgentNodeUpdate {
                        name: read(&weak_name, cx),
                        description: read(&weak_description, cx),
                        category: read(&weak_category, cx),
                        tags: read(&weak_tags, cx),
                        version: read(&weak_version, cx),
                        model_provider: read(&weak_model_provider, cx),
                        model_name: read(&weak_model_name, cx),
                        temperature: read(&weak_temperature, cx),
                        max_tokens: read(&weak_max_tokens, cx),
                        timeout_seconds: read(&weak_timeout, cx),
                        system_prompt: read(&weak_system_prompt, cx),
                        instructions: read(&weak_instructions, cx),
                        output_format: read(&weak_output_format, cx),
                        output_schema: read(&weak_output_schema, cx),
                        summarize_with_mainagent: read(&weak_summarize, cx),
                        skills_json: read(&weak_skills, cx),
                        mcp_tools_json: read(&weak_mcp_tools, cx),
                        system_tools_json: read(&weak_system_tools, cx),
                        coding_runtimes_json: read(&weak_coding_runtimes, cx),
                        retry: read(&weak_retry, cx),
                        settings_timeout_seconds: read(&weak_settings_timeout, cx),
                        human_confirmation: read(&weak_human_confirmation, cx),
                        routing_policy_json: read(&weak_routing_policy, cx),
                        permissions: read(&weak_permissions, cx),
                    };
                    match save_workflow_agent_node_update(&save_workflow_id, &save_node_id, update)
                    {
                        Ok(()) => {
                            mark_workflow_dirty(
                                &mut this.workflow_edit_states,
                                &save_workflow_id,
                                format!("Saved Agent {}", save_node_id),
                            );
                            this.push_toast(
                                ToastLevel::Success,
                                format!("Saved Agent {}", save_node_id),
                                cx,
                            );
                        }
                        Err(err) => {
                            mark_workflow_save_failed(
                                &mut this.workflow_edit_states,
                                &save_workflow_id,
                                err.to_string(),
                            );
                            this.push_toast(
                                ToastLevel::Error,
                                format!("Failed to save Agent: {}", err),
                                cx,
                            );
                        }
                    }
                    cx.notify();
                }));
        }
        panel = panel
            .child(
                div()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(PRIMARY_TEXT())
                            .child("Workflow Inspector"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(MUTED_TEXT())
                            .child("No node selected"),
                    ),
            )
            .child(workflow_inspector_row("Name", definition.name.clone()))
            .child(workflow_inspector_row("ID", definition.id.clone()))
            .child(workflow_inspector_row(
                "Description",
                if definition.description.is_empty() {
                    "No description".to_string()
                } else {
                    definition.description.clone()
                },
            ))
            .child(workflow_inspector_row(
                "Nodes",
                definition.nodes.len().to_string(),
            ))
            .child(workflow_inspector_row(
                "Edges",
                definition.edges.len().to_string(),
            ))
            .child(workflow_inspector_row(
                "Status",
                format!("{:?}", definition.status),
            ));
    } else {
        panel = panel.child(
            div()
                .flex_col()
                .gap_2()
                .child(
                    div()
                        .text_sm()
                        .font_weight(gpui::FontWeight::BOLD)
                        .text_color(PRIMARY_TEXT())
                        .child("Workflow Inspector"),
                )
                .child(
                    div()
                        .text_xs()
                        .line_height(relative(1.45))
                        .text_color(MUTED_TEXT())
                        .child("Create a workflow from a template to inspect and edit it here."),
                ),
        );
    }

    panel
}

fn workflow_inspector_row(label: &'static str, value: String) -> impl IntoElement {
    div()
        .flex_col()
        .gap_1()
        .child(
            div()
                .text_xs()
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(MUTED_TEXT())
                .child(label),
        )
        .child(
            div()
                .text_sm()
                .line_height(relative(1.35))
                .text_color(PRIMARY_TEXT())
                .child(value),
        )
}

fn workflow_editor_section(title: &'static str) -> Div {
    div()
        .flex_col()
        .gap_2()
        .pt_3()
        .border_t_1()
        .border_color(BORDER_LIGHT())
        .child(
            div()
                .text_xs()
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(MUTED_TEXT())
                .child(title),
        )
}

fn workflow_editor_field(
    label: &'static str,
    editor: gpui::Entity<Editor>,
    multiline: bool,
) -> impl IntoElement {
    div()
        .flex_col()
        .gap_1()
        .child(
            div()
                .text_xs()
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(MUTED_TEXT())
                .child(label),
        )
        .child(
            div()
                .h(if multiline { px(78.0) } else { px(32.0) })
                .p_2()
                .rounded_lg()
                .border_1()
                .border_color(BORDER_LIGHT())
                .bg(SURFACE_PANEL())
                .text_xs()
                .text_color(PRIMARY_TEXT())
                .child(editor),
        )
}

fn workflow_agent_field_editor(
    workflow_id: &str,
    node_id: &str,
    field: &'static str,
    initial_text: &str,
    multiline: bool,
    window: &mut Window,
    cx: &mut Context<AppState>,
) -> gpui::Entity<Editor> {
    let key = format!("workflow_agent_{workflow_id}_{node_id}_{field}");
    let initial_text = initial_text.to_string();
    window.use_keyed_state(key, &mut *cx, |window, cx| {
        let mut editor = if multiline {
            Editor::multi_line(window, cx)
        } else {
            Editor::single_line(window, cx)
        };
        editor.set_text(initial_text, window, cx);
        editor
    })
}

fn workflow_copilot_brief_editor(
    window: &mut Window,
    cx: &mut Context<AppState>,
) -> gpui::Entity<Editor> {
    window.use_keyed_state("workflow_copilot_brief", &mut *cx, |window, cx| {
        let mut editor = Editor::single_line(window, cx);
        editor.set_placeholder_text("例：帮我设计一个多 Agent 会员管理系统开发流程", window, cx);
        editor
    })
}

fn import_capability_panel(
    app: &AppState,
    window: &mut Window,
    cx: &mut Context<AppState>,
) -> impl IntoElement {
    let editor = capability_import_editor(app, window, cx);
    let weak_editor = editor.downgrade();
    div()
        .w(px(380.0))
        .p_4()
        .rounded_xl()
        .border_1()
        .border_color(BORDER_LIGHT())
        .bg(SURFACE_PANEL())
        .flex_col()
        .gap_3()
        .child(
            div()
                .text_sm()
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(PRIMARY_TEXT())
                .child("Import capability"),
        )
        .child(
            div()
                .h(px(96.0))
                .p_2()
                .rounded_lg()
                .bg(GHOST_SURFACE_BG())
                .border_1()
                .border_color(BORDER_LIGHT())
                .text_xs()
                .text_color(PRIMARY_TEXT())
                .child(editor),
        )
        .child(small_button("Import JSON", cx, move |this, cx| {
            let text = weak_editor
                .upgrade()
                .map(|editor| editor.read_with(cx, |editor, cx| editor.text(cx)))
                .unwrap_or_default();
            match crate::workflows::import_capability_package_json(&text) {
                Ok(capability_id) => {
                    this.capability_import_json.clear();
                    this.push_toast(
                        ToastLevel::Success,
                        format!("Imported capability {}", capability_id),
                        cx,
                    );
                }
                Err(err) => this.push_toast(
                    ToastLevel::Error,
                    format!("Failed to import capability: {}", err),
                    cx,
                ),
            }
            cx.notify();
        }))
}

fn capability_card(
    app: &AppState,
    window: &mut Window,
    capability: crate::workflows::capability::CapabilityManifest,
    cx: &mut Context<AppState>,
) -> impl IntoElement {
    let capability_id = capability.id.clone();
    let workflow_id = capability.workflow_id.clone();
    let active_version = capability.workflow_version;
    let versions_expanded = app.expanded_capability_versions_id.as_deref() == Some(&capability.id);
    let dependencies_expanded =
        app.expanded_capability_dependencies_id.as_deref() == Some(&capability.id);
    let input_editor = capability_input_editor(app, window, cx, &capability_id);

    let mut card = div()
        .w(px(320.0))
        .min_h(px(250.0))
        .p_5()
        .rounded_xl()
        .border_1()
        .border_color(BORDER_LIGHT())
        .bg(SURFACE_PANEL())
        .flex_col()
        .gap_4()
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_base()
                        .font_weight(gpui::FontWeight::BOLD)
                        .text_color(PRIMARY_TEXT())
                        .child(capability.name),
                )
                .child(
                    div()
                        .px_2()
                        .py_0p5()
                        .rounded_md()
                        .bg(GHOST_SURFACE_BG())
                        .text_xs()
                        .text_color(BRAND_BLUE())
                        .child(format!("v{}", capability.workflow_version)),
                ),
        )
        .child(
            div()
                .text_sm()
                .line_height(relative(1.4))
                .text_color(SECONDARY_TEXT())
                .child(capability.description),
        )
        .child(
            div()
                .text_xs()
                .text_color(MUTED_TEXT())
                .child(format!("workflow: {}", capability.workflow_id)),
        )
        .child(
            div()
                .mt_auto()
                .flex_col()
                .gap_2()
                .child(div().text_xs().text_color(MUTED_TEXT()).child("Input JSON"))
                .child(
                    div()
                        .px_3()
                        .py_2()
                        .rounded_lg()
                        .bg(GHOST_SURFACE_BG())
                        .border_1()
                        .border_color(BORDER_LIGHT())
                        .text_xs()
                        .text_color(PRIMARY_TEXT())
                        .child(input_editor.clone()),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(run_button(capability_id.clone(), input_editor, cx))
                        .child(export_capability_button(capability_id.clone(), cx))
                        .child(capability_versions_button(
                            capability_id.clone(),
                            versions_expanded,
                            cx,
                        ))
                        .child(capability_dependencies_button(
                            capability_id.clone(),
                            dependencies_expanded,
                            cx,
                        )),
                ),
        );

    if versions_expanded {
        card = card.child(capability_versions_panel(
            capability_id,
            workflow_id.clone(),
            active_version,
            cx,
        ));
    }
    if dependencies_expanded {
        card = card.child(capability_dependencies_panel(workflow_id, active_version));
    }

    card
}

fn capability_import_editor(
    app: &AppState,
    window: &mut Window,
    cx: &mut Context<AppState>,
) -> gpui::Entity<Editor> {
    let initial_text = app.capability_import_json.clone();
    window.use_keyed_state("capability_import_editor", &mut *cx, |window, cx| {
        let mut editor = Editor::multi_line(window, cx);
        editor.set_text(initial_text, window, cx);
        editor
    })
}

fn export_capability_button(capability_id: String, cx: &mut Context<AppState>) -> impl IntoElement {
    small_button("Export JSON", cx, move |this, cx| {
        match crate::workflows::export_capability_package_json(&capability_id) {
            Ok(json) => {
                cx.write_to_clipboard(ClipboardItem::new_string(json));
                this.push_toast(
                    ToastLevel::Success,
                    format!("Exported capability {} to clipboard", capability_id),
                    cx,
                );
            }
            Err(err) => this.push_toast(
                ToastLevel::Error,
                format!("Failed to export capability: {}", err),
                cx,
            ),
        }
        cx.notify();
    })
}

fn capability_versions_button(
    capability_id: String,
    expanded: bool,
    cx: &mut Context<AppState>,
) -> impl IntoElement {
    small_button(
        if expanded {
            "Hide versions"
        } else {
            "Versions"
        },
        cx,
        move |this, cx| {
            this.expanded_capability_versions_id = if expanded {
                None
            } else {
                Some(capability_id.clone())
            };
            cx.notify();
        },
    )
}

fn capability_dependencies_button(
    capability_id: String,
    expanded: bool,
    cx: &mut Context<AppState>,
) -> impl IntoElement {
    small_button(
        if expanded { "Hide deps" } else { "Deps" },
        cx,
        move |this, cx| {
            this.expanded_capability_dependencies_id = if expanded {
                None
            } else {
                Some(capability_id.clone())
            };
            cx.notify();
        },
    )
}

fn load_workflow_versions(
    workflow_id: &str,
) -> anyhow::Result<Vec<crate::workflows::WorkflowVersionSummary>> {
    let db = crate::task_db::Database::new()?;
    let store = crate::workflows::WorkflowStore::new(&db.conn)?;
    store.list_versions(workflow_id)
}

fn activate_capability_version(
    capability_id: &str,
    workflow_id: &str,
    version: i64,
) -> anyhow::Result<()> {
    let db = crate::task_db::Database::new()?;
    let store = crate::workflows::WorkflowStore::new(&db.conn)?;
    if store.load_version(workflow_id, version)?.is_none() {
        anyhow::bail!("workflow version {} v{} not found", workflow_id, version);
    }
    crate::task_db::update_capability_workflow_version(&db.conn, capability_id, version)
}

fn capability_versions_panel(
    capability_id: String,
    workflow_id: String,
    active_version: i64,
    cx: &mut Context<AppState>,
) -> impl IntoElement {
    let mut panel = div()
        .pt_3()
        .border_t_1()
        .border_color(BORDER_LIGHT())
        .flex_col()
        .gap_2()
        .child(
            div()
                .text_xs()
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(PRIMARY_TEXT())
                .child("Published versions"),
        );

    match load_workflow_versions(&workflow_id) {
        Ok(versions) if versions.is_empty() => {
            panel = panel.child(
                div()
                    .text_xs()
                    .text_color(MUTED_TEXT())
                    .child("No immutable versions found."),
            );
        }
        Ok(versions) => {
            for version in versions {
                panel = panel.child(capability_version_row(
                    capability_id.clone(),
                    version,
                    active_version,
                    cx,
                ));
            }
        }
        Err(err) => {
            panel = panel.child(
                div()
                    .text_xs()
                    .text_color(crate::ui_theme::ERROR_TEXT())
                    .child(format!("Failed to load versions: {}", err)),
            );
        }
    }

    panel
}

fn load_workflow_dependency_report(
    workflow_id: &str,
    workflow_version: i64,
) -> anyhow::Result<WorkflowDependencyReport> {
    let db = crate::task_db::Database::new()?;
    let store = crate::workflows::WorkflowStore::new(&db.conn)?;
    let Some(definition) = store.load_active_or_version(workflow_id, workflow_version)? else {
        anyhow::bail!("workflow {} v{} not found", workflow_id, workflow_version);
    };
    Ok(WorkflowDependencyReport::from_definition(&definition))
}

struct WorkflowDependencyReport {
    summary: crate::workflows::WorkflowDependencySummary,
    missing_skills: Vec<String>,
    custom_agents: Vec<String>,
    deferred_mcp_tools: Vec<String>,
}

impl WorkflowDependencyReport {
    fn from_definition(definition: &WorkflowDefinition) -> Self {
        let summary = definition.dependency_summary();
        let missing_skills = summary
            .skills
            .iter()
            .filter(|skill_id| crate::skills::find_skill(skill_id).is_none())
            .cloned()
            .collect();
        let custom_agents = summary
            .agents
            .iter()
            .filter(|agent_id| !is_builtin_main_agent(agent_id))
            .cloned()
            .collect();
        let deferred_mcp_tools = summary.mcp_tools.clone();
        Self {
            summary,
            missing_skills,
            custom_agents,
            deferred_mcp_tools,
        }
    }

    fn is_ready(&self) -> bool {
        self.missing_skills.is_empty()
            && self.custom_agents.is_empty()
            && self.deferred_mcp_tools.is_empty()
    }
}

fn is_builtin_main_agent(agent_id: &str) -> bool {
    matches!(
        agent_id.trim().to_ascii_lowercase().as_str(),
        "main" | "mainagent" | "main_agent"
    )
}

fn capability_dependencies_panel(workflow_id: String, workflow_version: i64) -> impl IntoElement {
    let mut panel = div()
        .pt_3()
        .border_t_1()
        .border_color(BORDER_LIGHT())
        .flex_col()
        .gap_2()
        .child(
            div()
                .text_xs()
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(PRIMARY_TEXT())
                .child("Dependencies"),
        );

    match load_workflow_dependency_report(&workflow_id, workflow_version) {
        Ok(report) => {
            let summary = &report.summary;
            panel = panel
                .child(dependency_group("Agents", &summary.agents))
                .child(dependency_group("Skills", &summary.skills))
                .child(dependency_group("MCP tools", &summary.mcp_tools))
                .child(div().text_xs().text_color(MUTED_TEXT()).child(format!(
                    "nodes: condition={} approval={} output={}",
                    summary.condition_nodes, summary.human_approval_nodes, summary.output_nodes
                )));
            if !summary.has_external_dependencies() {
                panel = panel.child(
                    div()
                        .text_xs()
                        .text_color(MUTED_TEXT())
                        .child("No external agent, skill, or MCP dependencies."),
                );
            }
            panel = panel.child(dependency_validation_panel(&report));
        }
        Err(err) => {
            panel = panel.child(
                div()
                    .text_xs()
                    .text_color(crate::ui_theme::ERROR_TEXT())
                    .child(format!("Failed to load dependencies: {}", err)),
            );
        }
    }

    panel
}

fn dependency_validation_panel(report: &WorkflowDependencyReport) -> impl IntoElement {
    let mut validation = div()
        .mt_2()
        .pt_2()
        .border_t_1()
        .border_color(BORDER_LIGHT())
        .flex_col()
        .gap_2()
        .child(
            div()
                .text_size(px(10.0))
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(MUTED_TEXT())
                .child("Availability"),
        );

    if report.is_ready() {
        return validation.child(
            div()
                .text_xs()
                .text_color(crate::ui_theme::SUCCESS_TEXT())
                .child("Ready: all dependencies are locally resolvable."),
        );
    }

    if !report.missing_skills.is_empty() {
        validation = validation.child(dependency_warning_group(
            "Missing skills",
            &report.missing_skills,
        ));
    }
    if !report.custom_agents.is_empty() {
        validation = validation.child(dependency_warning_group(
            "Custom agents need inline definitions",
            &report.custom_agents,
        ));
    }
    if !report.deferred_mcp_tools.is_empty() {
        validation = validation.child(dependency_warning_group(
            "MCP tools deferred",
            &report.deferred_mcp_tools,
        ));
    }

    validation
}

fn dependency_warning_group(label: &'static str, values: &[String]) -> impl IntoElement {
    let mut group = div()
        .rounded_lg()
        .bg(GHOST_SURFACE_BG())
        .border_1()
        .border_color(BORDER_LIGHT())
        .p_2()
        .flex_col()
        .gap_1()
        .child(
            div()
                .text_xs()
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(crate::ui_theme::ERROR_TEXT())
                .child(label),
        );
    for value in values {
        group = group.child(
            div()
                .text_xs()
                .text_color(SECONDARY_TEXT())
                .child(value.clone()),
        );
    }
    group
}

fn dependency_group(label: &'static str, values: &[String]) -> impl IntoElement {
    let mut group = div().flex_col().gap_1().child(
        div()
            .text_size(px(10.0))
            .font_weight(gpui::FontWeight::BOLD)
            .text_color(MUTED_TEXT())
            .child(label),
    );

    if values.is_empty() {
        group = group.child(div().text_xs().text_color(SECONDARY_TEXT()).child("None"));
    } else {
        let mut row = div().flex().flex_wrap().gap_1();
        for value in values {
            row = row.child(
                div()
                    .px_2()
                    .py_0p5()
                    .rounded_md()
                    .bg(GHOST_SURFACE_BG())
                    .border_1()
                    .border_color(BORDER_LIGHT())
                    .text_xs()
                    .text_color(PRIMARY_TEXT())
                    .child(value.clone()),
            );
        }
        group = group.child(row);
    }

    group
}

fn capability_version_row(
    capability_id: String,
    version: crate::workflows::WorkflowVersionSummary,
    active_version: i64,
    cx: &mut Context<AppState>,
) -> impl IntoElement {
    let is_active = version.version == active_version;
    let version_number = version.version;
    let workflow_id = version.workflow_id;
    let workflow_id_for_activate = workflow_id.clone();
    div()
        .p_2()
        .rounded_lg()
        .bg(GHOST_SURFACE_BG())
        .border_1()
        .border_color(BORDER_LIGHT())
        .flex()
        .items_center()
        .justify_between()
        .gap_2()
        .child(
            div()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .text_xs()
                        .font_weight(gpui::FontWeight::BOLD)
                        .text_color(if is_active {
                            BRAND_BLUE()
                        } else {
                            PRIMARY_TEXT()
                        })
                        .child(format!("v{}", version_number)),
                )
                .child(
                    div()
                        .text_size(px(10.0))
                        .text_color(MUTED_TEXT())
                        .child(workflow_id),
                ),
        )
        .child(if is_active {
            div()
                .px_2()
                .py_0p5()
                .rounded_md()
                .bg(SURFACE_PANEL())
                .text_xs()
                .text_color(BRAND_BLUE())
                .child("Active")
                .into_any_element()
        } else {
            small_button("Activate", cx, move |this, cx| {
                match activate_capability_version(
                    &capability_id,
                    &workflow_id_for_activate,
                    version_number,
                ) {
                    Ok(()) => this.push_toast(
                        ToastLevel::Success,
                        format!("Activated {} v{}", capability_id, version_number),
                        cx,
                    ),
                    Err(err) => this.push_toast(
                        ToastLevel::Error,
                        format!("Failed to activate version: {}", err),
                        cx,
                    ),
                }
                cx.notify();
            })
            .into_any_element()
        })
}

fn load_workflow_drafts() -> anyhow::Result<Vec<crate::workflows::WorkflowSummary>> {
    let db = crate::task_db::Database::new()?;
    let store = crate::workflows::WorkflowStore::new(&db.conn)?;
    store.list_drafts()
}

fn load_recent_runs() -> anyhow::Result<Vec<crate::task_db::WorkflowRunRow>> {
    let db = crate::task_db::Database::new()?;
    crate::task_db::load_recent_workflow_runs(&db.conn, 8)
}

fn recent_runs_panel(
    app: &AppState,
    window: &mut Window,
    cx: &mut Context<AppState>,
) -> impl IntoElement {
    let mut panel = div().mt_4().flex_col().gap_3().child(
        div()
            .text_lg()
            .font_weight(gpui::FontWeight::BOLD)
            .text_color(PRIMARY_TEXT())
            .child("Recent runs"),
    );

    match load_recent_runs() {
        Ok(runs) if runs.is_empty() => {
            panel = panel.child(
                div()
                    .p_5()
                    .rounded_xl()
                    .border_1()
                    .border_color(BORDER_LIGHT())
                    .bg(SURFACE_PANEL())
                    .text_sm()
                    .text_color(SECONDARY_TEXT())
                    .child("No capability runs yet."),
            );
        }
        Ok(runs) => {
            for run in runs {
                let expanded = app
                    .expanded_workflow_run_id
                    .map(|id| id == run.id)
                    .unwrap_or(false);
                panel = panel.child(run_row(app, window, run, expanded, cx));
            }
        }
        Err(err) => {
            panel = panel.child(
                div()
                    .p_5()
                    .rounded_xl()
                    .border_1()
                    .border_color(BORDER_LIGHT())
                    .bg(SURFACE_PANEL())
                    .text_sm()
                    .text_color(crate::ui_theme::ERROR_TEXT())
                    .child(format!("Failed to load recent runs: {}", err)),
            );
        }
    }

    panel
}

fn load_run_events(run_id: usize) -> anyhow::Result<Vec<crate::task_db::WorkflowRunEventRow>> {
    let db = crate::task_db::Database::new()?;
    crate::task_db::load_workflow_run_events(&db.conn, run_id)
}

fn run_row(
    app: &AppState,
    window: &mut Window,
    run: crate::task_db::WorkflowRunRow,
    expanded: bool,
    cx: &mut Context<AppState>,
) -> impl IntoElement {
    let is_running = run.status == "running";
    let run_id = run.id;
    let status = run.status.clone();
    let approval_note_editor = if is_running {
        Some(approval_note_editor(app, window, cx, run_id))
    } else {
        None
    };
    let mut actions = div().flex().items_center().gap_2().child(
        div()
            .px_2()
            .py_0p5()
            .rounded_md()
            .bg(GHOST_SURFACE_BG())
            .text_xs()
            .text_color(run_status_color(&status))
            .font_weight(gpui::FontWeight::BOLD)
            .child(status),
    );
    if is_running {
        let note_for_approve = approval_note_editor.clone();
        let note_for_reject = approval_note_editor.clone();
        actions = actions
            .child(approval_button(
                "Approve",
                run_id,
                true,
                note_for_approve,
                cx,
            ))
            .child(approval_button(
                "Reject",
                run_id,
                false,
                note_for_reject,
                cx,
            ));
    }
    actions = actions.child(run_details_button(run_id, expanded, cx));

    let mut row = div()
        .p_4()
        .rounded_xl()
        .border_1()
        .border_color(BORDER_LIGHT())
        .bg(SURFACE_PANEL())
        .flex_col()
        .gap_3()
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap_4()
                .child(
                    div()
                        .flex_col()
                        .gap_1()
                        .child(
                            div()
                                .text_sm()
                                .font_weight(gpui::FontWeight::BOLD)
                                .text_color(PRIMARY_TEXT())
                                .child(format!("#{} {}", run.id, run.workflow_id)),
                        )
                        .child(div().text_xs().text_color(MUTED_TEXT()).child(
                            if run.error.is_empty() {
                                format!("workflow version {}", run.workflow_version)
                            } else {
                                format!("workflow version {} · {}", run.workflow_version, run.error)
                            },
                        )),
                )
                .child(actions),
        );

    if let Some(editor) = approval_note_editor {
        row = row.child(
            div()
                .pt_3()
                .border_t_1()
                .border_color(BORDER_LIGHT())
                .flex_col()
                .gap_2()
                .child(
                    div()
                        .text_xs()
                        .text_color(MUTED_TEXT())
                        .child("Approval note"),
                )
                .child(
                    div()
                        .px_3()
                        .py_2()
                        .rounded_lg()
                        .bg(GHOST_SURFACE_BG())
                        .border_1()
                        .border_color(BORDER_LIGHT())
                        .text_xs()
                        .text_color(PRIMARY_TEXT())
                        .child(editor),
                ),
        );
    }

    if expanded {
        row = row.child(run_events_panel(run_id));
    }

    row
}

fn approval_note_editor(
    _app: &AppState,
    window: &mut Window,
    cx: &mut Context<AppState>,
    run_id: usize,
) -> gpui::Entity<Editor> {
    let key = format!("approval_note_editor_{}", run_id);
    window.use_keyed_state(key, &mut *cx, |window, cx| {
        let mut editor = Editor::single_line(window, cx);
        editor.set_placeholder_text("Optional approval note", window, cx);
        editor
    })
}

fn run_status_color(status: &str) -> gpui::Hsla {
    match status {
        "succeeded" => crate::ui_theme::SUCCESS_TEXT(),
        "failed" => crate::ui_theme::ERROR_TEXT(),
        _ => BRAND_BLUE(),
    }
}

fn workflow_template_gallery(cx: &mut Context<AppState>) -> impl IntoElement {
    div()
        .w(px(420.0))
        .p_4()
        .rounded_xl()
        .border_1()
        .border_color(BORDER_LIGHT())
        .bg(SURFACE_PANEL())
        .flex_col()
        .gap_3()
        .child(
            div()
                .text_sm()
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(PRIMARY_TEXT())
                .child("Template gallery"),
        )
        .child(
            div()
                .text_xs()
                .line_height(relative(1.4))
                .text_color(SECONDARY_TEXT())
                .child("Create editable draft workflows from common capability patterns."),
        )
        .child(template_button(
            "Echo output",
            WorkflowTemplateKind::EchoOutput,
            cx,
        ))
        .child(template_button(
            "MainAgent task",
            WorkflowTemplateKind::MainAgentTask,
            cx,
        ))
        .child(template_button(
            "Human approval",
            WorkflowTemplateKind::HumanApproval,
            cx,
        ))
}

fn template_button(
    label: &'static str,
    template: WorkflowTemplateKind,
    cx: &mut Context<AppState>,
) -> impl IntoElement {
    small_button(label, cx, move |this, cx| {
        let result = create_workflow_from_template(template);
        match result {
            Ok(id) => {
                this.editing_workflow_id = Some(id.clone());
                mark_workflow_dirty(&mut this.workflow_edit_states, &id, "Created from template");
                this.push_toast(ToastLevel::Success, format!("Created workflow {}", id), cx);
            }
            Err(err) => this.push_toast(
                ToastLevel::Error,
                format!("Failed to create workflow: {}", err),
                cx,
            ),
        }
        cx.notify();
    })
}

#[derive(Debug, Clone, Copy)]
enum WorkflowTemplateKind {
    EchoOutput,
    MainAgentTask,
    HumanApproval,
}

fn run_details_button(
    run_id: usize,
    expanded: bool,
    cx: &mut Context<AppState>,
) -> impl IntoElement {
    small_button(
        if expanded { "Hide" } else { "Details" },
        cx,
        move |this, cx| {
            this.expanded_workflow_run_id = if expanded { None } else { Some(run_id) };
            cx.notify();
        },
    )
}

fn run_events_panel(run_id: usize) -> impl IntoElement {
    let mut panel = div()
        .pt_3()
        .border_t_1()
        .border_color(BORDER_LIGHT())
        .flex_col()
        .gap_2();

    match load_run_events(run_id) {
        Ok(events) if events.is_empty() => {
            panel = panel.child(
                div()
                    .text_xs()
                    .text_color(MUTED_TEXT())
                    .child("No events recorded."),
            );
        }
        Ok(events) => {
            for event in events {
                panel = panel.child(run_event_row(event));
            }
        }
        Err(err) => {
            panel = panel.child(
                div()
                    .text_xs()
                    .text_color(crate::ui_theme::ERROR_TEXT())
                    .child(format!("Failed to load run events: {}", err)),
            );
        }
    }

    panel
}

fn run_event_row(event: crate::task_db::WorkflowRunEventRow) -> impl IntoElement {
    let payload = pretty_event_payload(&event.payload);
    div()
        .p_3()
        .rounded_lg()
        .bg(GHOST_SURFACE_BG())
        .border_1()
        .border_color(BORDER_LIGHT())
        .flex_col()
        .gap_1()
        .child(
            div()
                .text_xs()
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(BRAND_BLUE())
                .child(format!("#{} {}", event.id, event.kind)),
        )
        .child(
            div()
                .text_xs()
                .line_height(relative(1.35))
                .text_color(SECONDARY_TEXT())
                .child(payload),
        )
}

fn pretty_event_payload(payload: &str) -> String {
    serde_json::from_str::<serde_json::Value>(payload)
        .ok()
        .and_then(|value| serde_json::to_string_pretty(&value).ok())
        .unwrap_or_else(|| payload.to_string())
}

fn approval_button(
    label: &'static str,
    run_id: usize,
    approved: bool,
    note_editor: Option<gpui::Entity<Editor>>,
    cx: &mut Context<AppState>,
) -> impl IntoElement {
    let weak_note_editor = note_editor.map(|editor| editor.downgrade());
    small_button(label, cx, move |this, cx| {
        let note = weak_note_editor
            .as_ref()
            .and_then(|editor| editor.upgrade())
            .map(|editor| editor.read_with(cx, |editor, cx| editor.text(cx)))
            .unwrap_or_default();
        this.push_toast(
            ToastLevel::Info,
            format!(
                "{} workflow run #{}...",
                if approved { "Approving" } else { "Rejecting" },
                run_id
            ),
            cx,
        );
        cx.spawn(async move |this, cx| {
            let result = crate::workflows::resume_capability_run_with_note(
                run_id,
                approved,
                Some(note.trim().to_string()),
            )
            .await;
            let _ = this.update(cx, |this, cx| match result {
                Ok(value) => this.push_toast(
                    ToastLevel::Success,
                    format!("Workflow run #{} resumed: {}", run_id, value["status"]),
                    cx,
                ),
                Err(err) => this.push_toast(
                    ToastLevel::Error,
                    format!("Failed to resume workflow run #{}: {}", run_id, err),
                    cx,
                ),
            });
        })
        .detach();
        cx.notify();
    })
}

fn workflow_card(
    app: &AppState,
    window: &mut Window,
    workflow: crate::workflows::WorkflowSummary,
    cx: &mut Context<AppState>,
) -> impl IntoElement {
    let workflow_id = workflow.id.clone();
    let workflow_id_for_edit = workflow.id.clone();
    let workflow_name = workflow.name.clone();
    let workflow_description = workflow.description.clone();
    let is_editing = app.editing_workflow_id.as_deref() == Some(workflow.id.as_str());
    let active_workflow_id = app.editing_workflow_id.clone();
    let switching_from_dirty_workflow = active_workflow_id
        .as_deref()
        .filter(|active_id| *active_id != workflow_id_for_edit.as_str())
        .map(|active_id| workflow_has_dirty_state(app, active_id))
        .unwrap_or(false);

    let mut card = div()
        .p_5()
        .rounded_xl()
        .border_1()
        .border_color(BORDER_LIGHT())
        .bg(SURFACE_PANEL())
        .flex_col()
        .gap_4()
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap_6()
                .child(
                    div()
                        .flex_col()
                        .gap_2()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_3()
                                .child(
                                    div()
                                        .text_base()
                                        .font_weight(gpui::FontWeight::BOLD)
                                        .text_color(PRIMARY_TEXT())
                                        .child(workflow.name),
                                )
                                .child(
                                    div()
                                        .px_2()
                                        .py_0p5()
                                        .rounded_md()
                                        .bg(GHOST_SURFACE_BG())
                                        .text_xs()
                                        .text_color(BRAND_BLUE())
                                        .child(format!("draft v{}", workflow.version)),
                                ),
                        )
                        .child(
                            div()
                                .text_sm()
                                .line_height(relative(1.4))
                                .text_color(SECONDARY_TEXT())
                                .child(if workflow.description.is_empty() {
                                    "No description".to_string()
                                } else {
                                    workflow.description
                                }),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(MUTED_TEXT())
                                .child(format!("workflow: {}", workflow.id)),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(small_button("Edit", cx, move |this, cx| {
                            if switching_from_dirty_workflow {
                                if let Some(active_id) = active_workflow_id.as_ref() {
                                    this.push_toast(
                                        ToastLevel::Warning,
                                        format!("Workflow {} has unsaved changes.", active_id),
                                        cx,
                                    );
                                }
                            }
                            match load_workflow_definition_json(&workflow_id_for_edit) {
                                Ok(json) => {
                                    this.editing_workflow_id = Some(workflow_id_for_edit.clone());
                                    this.workflow_edit_json = json;
                                    this.push_toast(
                                        ToastLevel::Info,
                                        format!("Editing workflow {}", workflow_id_for_edit),
                                        cx,
                                    );
                                }
                                Err(err) => this.push_toast(
                                    ToastLevel::Error,
                                    format!("Failed to load workflow: {}", err),
                                    cx,
                                ),
                            }
                            cx.notify();
                        }))
                        .child(small_button("Publish", cx, move |this, cx| {
                            let result = publish_workflow_as_capability(
                                &workflow_id,
                                &workflow_name,
                                &workflow_description,
                            );
                            match result {
                                Ok(capability_id) => this.push_toast(
                                    ToastLevel::Success,
                                    format!("Published capability {}", capability_id),
                                    cx,
                                ),
                                Err(err) => this.push_toast(
                                    ToastLevel::Error,
                                    format!("Failed to publish workflow: {}", err),
                                    cx,
                                ),
                            }
                            cx.notify();
                        })),
                ),
        );

    if is_editing {
        let editor = workflow_json_editor(app, window, cx, &workflow.id);
        card = card.child(workflow_editor_panel(workflow.id, editor, cx));
    } else {
        card = card.child(workflow_graph_preview(&workflow.id, window, cx));
    }

    card
}

fn workflow_graph_preview(
    workflow_id: &str,
    window: &mut Window,
    cx: &mut Context<AppState>,
) -> impl IntoElement {
    let workflow_id_for_output = workflow_id.to_string();
    let workflow_id_for_approval = workflow_id.to_string();
    let workflow_id_for_agent = workflow_id.to_string();
    let mut panel = div()
        .pt_4()
        .border_t_1()
        .border_color(BORDER_LIGHT())
        .flex_col()
        .gap_3()
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap_3()
                .child(
                    div()
                        .text_sm()
                        .font_weight(gpui::FontWeight::BOLD)
                        .text_color(PRIMARY_TEXT())
                        .child("Graph preview"),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(small_button("Add output", cx, move |this, cx| {
                            match append_workflow_node(
                                &workflow_id_for_output,
                                WorkflowQuickNodeKind::Output,
                            ) {
                                Ok(node_id) => this.push_toast(
                                    ToastLevel::Success,
                                    format!("Added node {}", node_id),
                                    cx,
                                ),
                                Err(err) => this.push_toast(
                                    ToastLevel::Error,
                                    format!("Failed to add node: {}", err),
                                    cx,
                                ),
                            }
                            cx.notify();
                        }))
                        .child(small_button("Add approval", cx, move |this, cx| {
                            match append_workflow_node(
                                &workflow_id_for_approval,
                                WorkflowQuickNodeKind::HumanApproval,
                            ) {
                                Ok(node_id) => this.push_toast(
                                    ToastLevel::Success,
                                    format!("Added node {}", node_id),
                                    cx,
                                ),
                                Err(err) => this.push_toast(
                                    ToastLevel::Error,
                                    format!("Failed to add node: {}", err),
                                    cx,
                                ),
                            }
                            cx.notify();
                        }))
                        .child(small_button("Add MainAgent", cx, move |this, cx| {
                            match append_workflow_node(
                                &workflow_id_for_agent,
                                WorkflowQuickNodeKind::MainAgent,
                            ) {
                                Ok(node_id) => this.push_toast(
                                    ToastLevel::Success,
                                    format!("Added node {}", node_id),
                                    cx,
                                ),
                                Err(err) => this.push_toast(
                                    ToastLevel::Error,
                                    format!("Failed to add node: {}", err),
                                    cx,
                                ),
                            }
                            cx.notify();
                        })),
                ),
        );

    match load_workflow_definition(workflow_id) {
        Ok(definition) if definition.nodes.is_empty() => {
            panel = panel.child(
                div()
                    .text_xs()
                    .text_color(MUTED_TEXT())
                    .child("No nodes defined."),
            );
        }
        Ok(definition) => {
            panel = panel.child(workflow_node_chain(&definition));
            panel = panel.child(workflow_edge_list(&definition, window, cx));
        }
        Err(err) => {
            panel = panel.child(
                div()
                    .text_xs()
                    .text_color(crate::ui_theme::ERROR_TEXT())
                    .child(format!("Failed to load graph: {}", err)),
            );
        }
    }

    panel
}

fn workflow_node_chain(definition: &WorkflowDefinition) -> impl IntoElement {
    let mut row = div().flex().flex_wrap().items_center().gap_2();
    for (index, node) in definition.nodes.iter().enumerate() {
        row = row.child(workflow_node_chip(node));
        if index + 1 < definition.nodes.len() {
            row = row.child(
                div()
                    .text_xs()
                    .font_weight(gpui::FontWeight::BOLD)
                    .text_color(MUTED_TEXT())
                    .child("->"),
            );
        }
    }
    row
}

fn workflow_node_chip(node: &WorkflowNode) -> impl IntoElement {
    div()
        .min_w(px(150.0))
        .p_3()
        .rounded_lg()
        .bg(GHOST_SURFACE_BG())
        .border_1()
        .border_color(BORDER_LIGHT())
        .flex_col()
        .gap_1()
        .child(
            div()
                .text_xs()
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(PRIMARY_TEXT())
                .child(if node.name.is_empty() {
                    node.id.clone()
                } else {
                    node.name.clone()
                }),
        )
        .child(
            div()
                .text_size(px(10.0))
                .text_color(BRAND_BLUE())
                .child(workflow_node_kind_label(&node.kind)),
        )
        .child(
            div()
                .text_size(px(10.0))
                .text_color(MUTED_TEXT())
                .child(node.id.clone()),
        )
}

fn workflow_edge_list(
    definition: &WorkflowDefinition,
    window: &mut Window,
    cx: &mut Context<AppState>,
) -> impl IntoElement {
    let mut panel = div().flex_col().gap_2().child(
        div()
            .text_size(px(10.0))
            .font_weight(gpui::FontWeight::BOLD)
            .text_color(MUTED_TEXT())
            .child("Edges"),
    );

    if definition.edges.is_empty() {
        return panel.child(
            div()
                .text_xs()
                .text_color(SECONDARY_TEXT())
                .child("Linear execution order; no explicit edges."),
        );
    }

    for edge in &definition.edges {
        let condition_editor = workflow_edge_condition_editor(definition, edge, window, cx);
        let weak_condition_editor = condition_editor.downgrade();
        let workflow_id = definition.id.clone();
        let edge_id = edge.id.clone();
        panel = panel.child(
            div()
                .px_3()
                .py_2()
                .rounded_lg()
                .bg(GHOST_SURFACE_BG())
                .border_1()
                .border_color(BORDER_LIGHT())
                .flex()
                .items_center()
                .justify_between()
                .gap_3()
                .child(
                    div()
                        .min_w(px(180.0))
                        .text_xs()
                        .text_color(PRIMARY_TEXT())
                        .child(format!("{} -> {}", edge.from_node_id, edge.to_node_id)),
                )
                .child(
                    div()
                        .flex_1()
                        .px_2()
                        .py_1()
                        .rounded_md()
                        .bg(SURFACE_PANEL())
                        .border_1()
                        .border_color(BORDER_LIGHT())
                        .text_xs()
                        .text_color(PRIMARY_TEXT())
                        .child(condition_editor),
                )
                .child(small_button("Save", cx, move |this, cx| {
                    let condition = weak_condition_editor
                        .upgrade()
                        .map(|editor| editor.read_with(cx, |editor, cx| editor.text(cx)))
                        .unwrap_or_default();
                    match update_workflow_edge_condition(&workflow_id, &edge_id, &condition) {
                        Ok(()) => this.push_toast(
                            ToastLevel::Success,
                            format!("Saved edge {}", edge_id),
                            cx,
                        ),
                        Err(err) => this.push_toast(
                            ToastLevel::Error,
                            format!("Failed to save edge condition: {}", err),
                            cx,
                        ),
                    }
                    cx.notify();
                })),
        );
    }

    panel
}

fn workflow_edge_condition_editor(
    definition: &WorkflowDefinition,
    edge: &WorkflowEdge,
    window: &mut Window,
    cx: &mut Context<AppState>,
) -> gpui::Entity<Editor> {
    let key = format!("workflow_edge_condition_{}_{}", definition.id, edge.id);
    let initial_text = edge.condition.clone();
    window.use_keyed_state(key, &mut *cx, |window, cx| {
        let mut editor = Editor::single_line(window, cx);
        editor.set_placeholder_text("default | always | approved == true", window, cx);
        editor.set_text(initial_text, window, cx);
        editor
    })
}

fn workflow_node_kind_label(kind: &WorkflowNodeKind) -> String {
    match kind {
        WorkflowNodeKind::Agent { agent_id } => format!("agent: {}", agent_id),
        WorkflowNodeKind::Skill { skill_id } => format!("skill: {}", skill_id),
        WorkflowNodeKind::McpTool {
            server_name,
            tool_name,
        } => format!("mcp: {}:{}", server_name, tool_name),
        WorkflowNodeKind::Condition => "condition".to_string(),
        WorkflowNodeKind::HumanApproval => "human_approval".to_string(),
        WorkflowNodeKind::Output => "output".to_string(),
    }
}

fn workflow_node_config_string(node: &WorkflowNode, path: &[&str], default: &str) -> String {
    workflow_node_config_value(node, path)
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(default)
        .to_string()
}

fn workflow_node_config_number(node: &WorkflowNode, path: &[&str], default: &str) -> String {
    workflow_node_config_value(node, path)
        .and_then(|value| {
            value
                .as_i64()
                .map(|number| number.to_string())
                .or_else(|| value.as_f64().map(|number| number.to_string()))
        })
        .unwrap_or_else(|| default.to_string())
}

fn workflow_node_config_tags(node: &WorkflowNode) -> String {
    workflow_node_config_value(node, &["metadata", "tags"])
        .and_then(|value| value.as_array())
        .map(|tags| {
            tags.iter()
                .filter_map(|tag| tag.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default()
}

fn workflow_node_config_json_text(node: &WorkflowNode, path: &[&str], default: &str) -> String {
    workflow_node_config_value(node, path)
        .map(|value| serde_json::to_string_pretty(value).unwrap_or_else(|_| default.to_string()))
        .unwrap_or_else(|| default.to_string())
}

fn workflow_node_config_bool(node: &WorkflowNode, path: &[&str], default: bool) -> bool {
    workflow_node_config_value(node, path)
        .and_then(|value| value.as_bool())
        .unwrap_or(default)
}

fn workflow_node_config_value<'a>(
    node: &'a WorkflowNode,
    path: &[&str],
) -> Option<&'a serde_json::Value> {
    let mut current = &node.config;
    for key in path {
        current = current.get(*key)?;
    }
    Some(current)
}

fn workflow_node_routing_mode(node: &WorkflowNode) -> String {
    node.config
        .get("routing")
        .and_then(|routing| routing.get("mode"))
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("sequential")
        .to_string()
}

fn workflow_node_tool_summary(node: &WorkflowNode) -> String {
    let Some(tools) = node.config.get("tools") else {
        return "No tools configured".to_string();
    };

    let count_array = |key: &str| {
        tools
            .get(key)
            .and_then(|value| value.as_array())
            .map(|items| items.len())
            .unwrap_or(0)
    };
    let skills = count_array("skills");
    let mcp_tools = count_array("mcp_tools");
    let system_tools = count_array("system_tools");
    let coding_runtimes = count_array("coding_runtimes");

    if skills + mcp_tools + system_tools + coding_runtimes == 0 {
        "No tools configured".to_string()
    } else {
        format!(
            "{skills} skills, {mcp_tools} MCP tools, {system_tools} system tools, {coding_runtimes} coding runtimes"
        )
    }
}

#[derive(Debug, Clone, Copy)]
enum WorkflowQuickNodeKind {
    Output,
    HumanApproval,
    MainAgent,
}

#[derive(Debug, Clone)]
struct WorkflowAgentNodeUpdate {
    name: String,
    description: String,
    category: String,
    tags: String,
    version: String,
    model_provider: String,
    model_name: String,
    temperature: String,
    max_tokens: String,
    timeout_seconds: String,
    system_prompt: String,
    instructions: String,
    output_format: String,
    output_schema: String,
    summarize_with_mainagent: String,
    skills_json: String,
    mcp_tools_json: String,
    system_tools_json: String,
    coding_runtimes_json: String,
    retry: String,
    settings_timeout_seconds: String,
    human_confirmation: String,
    routing_policy_json: String,
    permissions: String,
}

fn append_empty_agent_node(workflow_id: &str) -> anyhow::Result<String> {
    let mut definition = load_workflow_definition(workflow_id)?;
    let node_id = next_workflow_node_id(&definition, "local_agent");
    definition
        .nodes
        .push(empty_local_agent_node(&node_id, "Local Agent"));

    let db = crate::task_db::Database::new()?;
    let store = crate::workflows::WorkflowStore::new(&db.conn)?;
    store.save_draft(&definition)?;
    Ok(node_id)
}

fn save_workflow_agent_node_update(
    workflow_id: &str,
    node_id: &str,
    update: WorkflowAgentNodeUpdate,
) -> anyhow::Result<()> {
    let mut definition = load_workflow_definition(workflow_id)?;
    apply_workflow_agent_node_update(&mut definition, node_id, update)?;

    let db = crate::task_db::Database::new()?;
    let store = crate::workflows::WorkflowStore::new(&db.conn)?;
    store.save_draft(&definition)
}

fn apply_workflow_agent_node_update(
    definition: &mut WorkflowDefinition,
    node_id: &str,
    update: WorkflowAgentNodeUpdate,
) -> anyhow::Result<()> {
    let routing_policy = parse_optional_json("routing policy", &update.routing_policy_json)?;
    crate::workflows::RoutingPolicy::from_value(&routing_policy)?.validate(Some(definition))?;

    let Some(node) = definition
        .nodes
        .iter_mut()
        .find(|node| node.id.as_str() == node_id)
    else {
        anyhow::bail!("workflow node '{}' not found", node_id);
    };
    if !matches!(node.kind, WorkflowNodeKind::Agent { .. }) {
        anyhow::bail!("workflow node '{}' is not an Agent node", node_id);
    }

    let name = update.name.trim();
    if name.is_empty() {
        anyhow::bail!("Agent name is required");
    }
    let temperature = parse_workflow_f64("temperature", &update.temperature)?;
    let max_tokens = parse_workflow_u64("max tokens", &update.max_tokens)?;
    let timeout_seconds = parse_workflow_u64("timeout seconds", &update.timeout_seconds)?;
    let output_schema = parse_optional_json("output schema", &update.output_schema)?;
    let summarize_with_mainagent =
        parse_workflow_bool("summarize with MainAgent", &update.summarize_with_mainagent)?;
    let skills = parse_json_array("skills", &update.skills_json)?;
    let mcp_tools = parse_json_array("MCP tools", &update.mcp_tools_json)?;
    let system_tools = parse_json_array("system tools", &update.system_tools_json)?;
    let coding_runtimes = parse_json_array("coding runtimes", &update.coding_runtimes_json)?;
    let retry = parse_workflow_u64("retry", &update.retry)?;
    let settings_timeout_seconds =
        parse_workflow_u64("settings timeout seconds", &update.settings_timeout_seconds)?;
    let human_confirmation = parse_workflow_bool("human confirmation", &update.human_confirmation)?;

    node.name = name.to_string();
    set_config_string(
        &mut node.config,
        &["description"],
        update.description.trim(),
    );
    set_config_string(
        &mut node.config,
        &["metadata", "category"],
        update.category.trim(),
    );
    set_config_value(
        &mut node.config,
        &["metadata", "tags"],
        serde_json::Value::Array(
            update
                .tags
                .split(',')
                .map(str::trim)
                .filter(|tag| !tag.is_empty())
                .map(|tag| serde_json::Value::String(tag.to_string()))
                .collect(),
        ),
    );
    set_config_string(
        &mut node.config,
        &["metadata", "version"],
        update.version.trim(),
    );
    set_config_string(
        &mut node.config,
        &["model", "provider"],
        update.model_provider.trim(),
    );
    set_config_string(
        &mut node.config,
        &["model", "model"],
        update.model_name.trim(),
    );
    set_config_value(
        &mut node.config,
        &["model", "temperature"],
        serde_json::json!(temperature),
    );
    set_config_value(
        &mut node.config,
        &["model", "max_tokens"],
        serde_json::json!(max_tokens),
    );
    set_config_value(
        &mut node.config,
        &["model", "timeout_seconds"],
        serde_json::json!(timeout_seconds),
    );
    set_config_string(
        &mut node.config,
        &["prompt", "system"],
        &update.system_prompt,
    );
    set_config_string(
        &mut node.config,
        &["prompt", "instructions"],
        &update.instructions,
    );
    set_config_string(
        &mut node.config,
        &["output", "format"],
        update.output_format.trim(),
    );
    set_config_value(&mut node.config, &["output", "schema"], output_schema);
    set_config_value(
        &mut node.config,
        &["output", "summarize_with_mainagent"],
        serde_json::json!(summarize_with_mainagent),
    );
    set_config_value(&mut node.config, &["tools", "skills"], skills);
    set_config_value(&mut node.config, &["tools", "mcp_tools"], mcp_tools);
    set_config_value(&mut node.config, &["tools", "system_tools"], system_tools);
    set_config_value(
        &mut node.config,
        &["tools", "coding_runtimes"],
        coding_runtimes,
    );
    set_config_value(
        &mut node.config,
        &["settings", "retry"],
        serde_json::json!(retry),
    );
    set_config_value(
        &mut node.config,
        &["settings", "timeout_seconds"],
        serde_json::json!(settings_timeout_seconds),
    );
    set_config_value(
        &mut node.config,
        &["settings", "human_confirmation"],
        serde_json::json!(human_confirmation),
    );
    set_config_value(&mut node.config, &["routing"], routing_policy);
    set_config_string(
        &mut node.config,
        &["settings", "permissions"],
        update.permissions.trim(),
    );
    Ok(())
}

fn parse_workflow_f64(label: &str, value: &str) -> anyhow::Result<f64> {
    value
        .trim()
        .parse::<f64>()
        .map_err(|err| anyhow::anyhow!("{label} must be a number: {err}"))
}

fn parse_workflow_u64(label: &str, value: &str) -> anyhow::Result<u64> {
    value
        .trim()
        .parse::<u64>()
        .map_err(|err| anyhow::anyhow!("{label} must be a positive integer: {err}"))
}

fn parse_workflow_bool(label: &str, value: &str) -> anyhow::Result<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" | "yes" | "1" => Ok(true),
        "false" | "no" | "0" => Ok(false),
        other => anyhow::bail!("{label} must be true or false, got '{other}'"),
    }
}

fn parse_optional_json(label: &str, value: &str) -> anyhow::Result<serde_json::Value> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Ok(serde_json::Value::Null);
    }
    serde_json::from_str(trimmed)
        .map_err(|err| anyhow::anyhow!("{label} must be valid JSON: {err}"))
}

fn parse_json_array(label: &str, value: &str) -> anyhow::Result<serde_json::Value> {
    let parsed = parse_optional_json(label, value)?;
    if parsed.is_null() {
        return Ok(serde_json::Value::Array(Vec::new()));
    }
    if !parsed.is_array() {
        anyhow::bail!("{label} must be a JSON array");
    }
    Ok(parsed)
}

fn set_config_string(config: &mut serde_json::Value, path: &[&str], value: &str) {
    set_config_value(config, path, serde_json::Value::String(value.to_string()));
}

fn set_config_value(config: &mut serde_json::Value, path: &[&str], value: serde_json::Value) {
    if path.is_empty() {
        *config = value;
        return;
    }
    if !config.is_object() {
        *config = serde_json::json!({});
    }
    let mut current = config;
    for key in &path[..path.len() - 1] {
        let object = current.as_object_mut().expect("config object");
        current = object
            .entry((*key).to_string())
            .or_insert_with(|| serde_json::json!({}));
        if !current.is_object() {
            *current = serde_json::json!({});
        }
    }
    let object = current.as_object_mut().expect("config object");
    object.insert(path[path.len() - 1].to_string(), value);
}

fn empty_local_agent_node(node_id: &str, name: &str) -> WorkflowNode {
    WorkflowNode {
        id: node_id.to_string(),
        name: name.to_string(),
        kind: WorkflowNodeKind::Agent {
            agent_id: format!("local:{node_id}"),
        },
        config: empty_local_agent_config(),
    }
}

fn empty_local_agent_config() -> serde_json::Value {
    serde_json::json!({
        "description": "",
        "model": {
            "provider": "default",
            "model": "default",
            "temperature": 0.2,
            "max_tokens": 4096,
            "timeout_seconds": 120
        },
        "prompt": {
            "system": "",
            "instructions": "",
            "context_rules": []
        },
        "tools": {
            "skills": [],
            "mcp_tools": [],
            "system_tools": [],
            "coding_runtimes": []
        },
        "output": {
            "schema": null,
            "format": "text",
            "summarize_with_mainagent": true
        },
        "settings": {
            "retry": 0,
            "timeout_seconds": 120,
            "human_confirmation": false,
            "permissions": "ask"
        },
        "routing": {
            "mode": "sequential"
        }
    })
}

fn append_workflow_node(
    workflow_id: &str,
    quick_kind: WorkflowQuickNodeKind,
) -> anyhow::Result<String> {
    let mut definition = load_workflow_definition(workflow_id)?;
    let previous_node_id = definition.nodes.last().map(|node| node.id.clone());
    let node_id = next_workflow_node_id(&definition, workflow_quick_node_base(quick_kind));
    definition
        .nodes
        .push(workflow_quick_node(&node_id, quick_kind));

    if !definition.edges.is_empty() {
        if let Some(previous_node_id) = previous_node_id {
            let edge_id = next_workflow_edge_id(&definition, &previous_node_id, &node_id);
            definition.edges.push(WorkflowEdge {
                id: edge_id,
                from_node_id: previous_node_id,
                to_node_id: node_id.clone(),
                condition: "always".to_string(),
            });
        }
    }

    let db = crate::task_db::Database::new()?;
    let store = crate::workflows::WorkflowStore::new(&db.conn)?;
    store.save_draft(&definition)?;
    Ok(node_id)
}

fn update_workflow_edge_condition(
    workflow_id: &str,
    edge_id: &str,
    condition: &str,
) -> anyhow::Result<()> {
    let mut definition = load_workflow_definition(workflow_id)?;
    let Some(edge) = definition.edges.iter_mut().find(|edge| edge.id == edge_id) else {
        anyhow::bail!("workflow edge '{}' not found", edge_id);
    };
    edge.condition = condition.trim().to_string();

    let db = crate::task_db::Database::new()?;
    let store = crate::workflows::WorkflowStore::new(&db.conn)?;
    store.save_draft(&definition)
}

fn create_workflow_edge(
    workflow_id: &str,
    from_node_id: &str,
    to_node_id: &str,
) -> anyhow::Result<String> {
    let mut definition = load_workflow_definition(workflow_id)?;
    let edge_id = apply_create_workflow_edge(&mut definition, from_node_id, to_node_id)?;

    let db = crate::task_db::Database::new()?;
    let store = crate::workflows::WorkflowStore::new(&db.conn)?;
    store.save_draft(&definition)?;
    Ok(edge_id)
}

fn delete_workflow_edge(workflow_id: &str, edge_id: &str) -> anyhow::Result<()> {
    let mut definition = load_workflow_definition(workflow_id)?;
    apply_delete_workflow_edge(&mut definition, edge_id)?;

    let db = crate::task_db::Database::new()?;
    let store = crate::workflows::WorkflowStore::new(&db.conn)?;
    store.save_draft(&definition)
}

fn apply_create_workflow_edge(
    definition: &mut WorkflowDefinition,
    from_node_id: &str,
    to_node_id: &str,
) -> anyhow::Result<String> {
    if from_node_id == to_node_id {
        anyhow::bail!("self-loop routes are not allowed");
    }
    let has_from = definition
        .nodes
        .iter()
        .any(|node| node.id.as_str() == from_node_id);
    let has_to = definition
        .nodes
        .iter()
        .any(|node| node.id.as_str() == to_node_id);
    if !has_from {
        anyhow::bail!("source node '{}' not found", from_node_id);
    }
    if !has_to {
        anyhow::bail!("target node '{}' not found", to_node_id);
    }

    let edge_id = next_workflow_edge_id(definition, from_node_id, to_node_id);
    definition.edges.push(WorkflowEdge {
        id: edge_id.clone(),
        from_node_id: from_node_id.to_string(),
        to_node_id: to_node_id.to_string(),
        condition: "always".to_string(),
    });
    Ok(edge_id)
}

fn apply_delete_workflow_edge(
    definition: &mut WorkflowDefinition,
    edge_id: &str,
) -> anyhow::Result<()> {
    let before = definition.edges.len();
    definition.edges.retain(|edge| edge.id.as_str() != edge_id);
    if definition.edges.len() == before {
        anyhow::bail!("workflow edge '{}' not found", edge_id);
    }
    Ok(())
}

fn workflow_quick_node_base(kind: WorkflowQuickNodeKind) -> &'static str {
    match kind {
        WorkflowQuickNodeKind::Output => "output",
        WorkflowQuickNodeKind::HumanApproval => "approval",
        WorkflowQuickNodeKind::MainAgent => "mainagent",
    }
}

fn workflow_quick_node(node_id: &str, kind: WorkflowQuickNodeKind) -> WorkflowNode {
    match kind {
        WorkflowQuickNodeKind::Output => WorkflowNode {
            id: node_id.to_string(),
            name: "Output".to_string(),
            kind: WorkflowNodeKind::Output,
            config: serde_json::json!({
                "value": {
                    "status": "ok"
                }
            }),
        },
        WorkflowQuickNodeKind::HumanApproval => WorkflowNode {
            id: node_id.to_string(),
            name: "Approval".to_string(),
            kind: WorkflowNodeKind::HumanApproval,
            config: serde_json::Value::Null,
        },
        WorkflowQuickNodeKind::MainAgent => WorkflowNode {
            id: node_id.to_string(),
            name: "MainAgent".to_string(),
            kind: WorkflowNodeKind::Agent {
                agent_id: "mainagent".to_string(),
            },
            config: serde_json::json!({
                "workspace": "workflow",
                "prompt": "Use the workflow input as the task brief. Return a concise result."
            }),
        },
    }
}

fn next_workflow_node_id(definition: &WorkflowDefinition, base: &str) -> String {
    if !definition.nodes.iter().any(|node| node.id == base) {
        return base.to_string();
    }
    for index in 2.. {
        let candidate = format!("{}_{}", base, index);
        if !definition.nodes.iter().any(|node| node.id == candidate) {
            return candidate;
        }
    }
    unreachable!("unbounded node id search should always find a candidate")
}

fn next_workflow_edge_id(
    definition: &WorkflowDefinition,
    from_node_id: &str,
    to_node_id: &str,
) -> String {
    let base = format!("{}_to_{}", from_node_id, to_node_id);
    if !definition.edges.iter().any(|edge| edge.id == base) {
        return base;
    }
    for index in 2.. {
        let candidate = format!("{}_{}", base, index);
        if !definition.edges.iter().any(|edge| edge.id == candidate) {
            return candidate;
        }
    }
    unreachable!("unbounded edge id search should always find a candidate")
}

fn workflow_json_editor(
    app: &AppState,
    window: &mut Window,
    cx: &mut Context<AppState>,
    workflow_id: &str,
) -> gpui::Entity<Editor> {
    let key = format!("workflow_json_editor_{}", workflow_id);
    let initial_text = app.workflow_edit_json.clone();
    window.use_keyed_state(key, &mut *cx, |window, cx| {
        let mut editor = Editor::multi_line(window, cx);
        editor.set_text(initial_text, window, cx);
        editor
    })
}

fn workflow_editor_panel(
    workflow_id: String,
    editor: gpui::Entity<Editor>,
    cx: &mut Context<AppState>,
) -> impl IntoElement {
    let weak_editor_for_save = editor.downgrade();
    let workflow_id_for_save = workflow_id.clone();
    div()
        .mt_4()
        .pt_4()
        .border_t_1()
        .border_color(BORDER_LIGHT())
        .flex_col()
        .gap_3()
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_sm()
                        .font_weight(gpui::FontWeight::BOLD)
                        .text_color(PRIMARY_TEXT())
                        .child("Workflow JSON"),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(small_button("Save JSON", cx, move |this, cx| {
                            let text = weak_editor_for_save
                                .upgrade()
                                .map(|editor| editor.read_with(cx, |editor, cx| editor.text(cx)))
                                .unwrap_or_default();
                            match save_workflow_definition_json(&text) {
                                Ok(saved_id) => {
                                    this.workflow_edit_json = text;
                                    mark_workflow_saved(&mut this.workflow_edit_states, &saved_id);
                                    this.push_toast(
                                        ToastLevel::Success,
                                        format!("Saved workflow {}", saved_id),
                                        cx,
                                    );
                                }
                                Err(err) => {
                                    mark_workflow_save_failed(
                                        &mut this.workflow_edit_states,
                                        &workflow_id_for_save,
                                        err.to_string(),
                                    );
                                    this.push_toast(
                                        ToastLevel::Error,
                                        format!("Failed to save workflow JSON: {}", err),
                                        cx,
                                    );
                                }
                            }
                            cx.notify();
                        }))
                        .child(small_button("Close", cx, move |this, cx| {
                            if this.editing_workflow_id.as_deref() == Some(workflow_id.as_str()) {
                                this.editing_workflow_id = None;
                                this.workflow_edit_json.clear();
                            }
                            cx.notify();
                        })),
                ),
        )
        .child(
            div()
                .h(px(360.0))
                .p_3()
                .rounded_lg()
                .bg(GHOST_SURFACE_BG())
                .border_1()
                .border_color(BORDER_LIGHT())
                .text_xs()
                .text_color(PRIMARY_TEXT())
                .child(editor),
        )
}

fn small_button<F>(label: &'static str, cx: &mut Context<AppState>, handler: F) -> impl IntoElement
where
    F: Fn(&mut AppState, &mut Context<AppState>) + Send + Sync + 'static,
{
    div()
        .px_3()
        .py_1()
        .rounded_lg()
        .bg(GHOST_SURFACE_BG())
        .border_1()
        .border_color(BORDER_LIGHT())
        .text_xs()
        .text_color(ACCENT_TEXT())
        .font_weight(gpui::FontWeight::BOLD)
        .cursor_pointer()
        .child(label)
        .on_mouse_down(
            gpui::MouseButton::Left,
            cx.listener(move |this, _: &gpui::MouseDownEvent, _window, cx| {
                handler(this, cx);
            }),
        )
}

fn create_workflow_from_template(template: WorkflowTemplateKind) -> anyhow::Result<String> {
    let definition = match template {
        WorkflowTemplateKind::EchoOutput => echo_output_template(),
        WorkflowTemplateKind::MainAgentTask => main_agent_task_template(),
        WorkflowTemplateKind::HumanApproval => human_approval_template(),
    };
    let id = definition.id.clone();
    let db = crate::task_db::Database::new()?;
    let store = crate::workflows::WorkflowStore::new(&db.conn)?;
    store.save_draft(&definition)?;
    Ok(id)
}

async fn create_workflow_from_copilot_brief(
    brief: &str,
    source_workflow_id: Option<String>,
) -> anyhow::Result<String> {
    let context = workflow_copilot_context();
    let mut definition = crate::workflows::design_workflow_from_brief(brief, context).await?;
    if let Some(source_workflow_id) = source_workflow_id {
        definition.metadata = merge_metadata(
            definition.metadata,
            serde_json::json!({
                "copilot": {
                    "source_workflow_id": source_workflow_id,
                    "brief": brief.trim()
                }
            }),
        );
    } else {
        definition.metadata = merge_metadata(
            definition.metadata,
            serde_json::json!({
                "copilot": {
                    "brief": brief.trim()
                }
            }),
        );
    }
    crate::workflows::validate_definition_routing(&definition)?;
    let id = definition.id.clone();
    let db = crate::task_db::Database::new()?;
    let store = crate::workflows::WorkflowStore::new(&db.conn)?;
    store.save_draft(&definition)?;
    Ok(id)
}

fn workflow_copilot_context() -> crate::workflows::WorkflowCopilotContext {
    let config = crate::services::config::load_config();
    crate::workflows::WorkflowCopilotContext {
        available_skills: crate::skills::skill_manifests()
            .into_iter()
            .map(|skill| skill.id)
            .collect(),
        available_mcp_tools: Vec::new(),
        available_system_tools: vec![
            "run_system_task".to_string(),
            "run_shell_command".to_string(),
            "run_capability".to_string(),
        ],
        available_coding_runtimes: config
            .coding_agents
            .into_iter()
            .map(|agent| agent.id)
            .collect(),
    }
}

fn merge_metadata(mut base: serde_json::Value, extra: serde_json::Value) -> serde_json::Value {
    if !base.is_object() {
        base = serde_json::json!({});
    }
    if let (Some(base), Some(extra)) = (base.as_object_mut(), extra.as_object()) {
        for (key, value) in extra {
            base.insert(key.clone(), value.clone());
        }
    }
    base
}

fn template_workflow_id(slug: &str) -> String {
    format!(
        "workflow.template.{}.{}",
        slug,
        chrono::Local::now().timestamp_millis()
    )
}

fn echo_output_template() -> WorkflowDefinition {
    let mut definition =
        WorkflowDefinition::new_draft(template_workflow_id("echo"), "Echo Output Workflow");
    definition.description = "A minimal workflow that returns a fixed JSON output.".to_string();
    definition.nodes.push(WorkflowNode {
        id: "output".to_string(),
        name: "Output".to_string(),
        kind: WorkflowNodeKind::Output,
        config: serde_json::json!({
            "value": {
                "status": "ok",
                "message": "Edit this output JSON before publishing."
            }
        }),
    });
    definition
}

fn main_agent_task_template() -> WorkflowDefinition {
    let mut definition =
        WorkflowDefinition::new_draft(template_workflow_id("mainagent"), "MainAgent Task Workflow");
    definition.description =
        "A single MainAgent workflow node followed by a stable output boundary.".to_string();
    definition.nodes.push(WorkflowNode {
        id: "mainagent".to_string(),
        name: "MainAgent".to_string(),
        kind: WorkflowNodeKind::Agent {
            agent_id: "mainagent".to_string(),
        },
        config: serde_json::json!({
            "workspace": "workflow",
            "prompt": "Use the workflow input as the task brief. Return a concise result."
        }),
    });
    definition.nodes.push(WorkflowNode {
        id: "output".to_string(),
        name: "Output".to_string(),
        kind: WorkflowNodeKind::Output,
        config: serde_json::Value::Null,
    });
    definition.edges.push(WorkflowEdge {
        id: "mainagent_to_output".to_string(),
        from_node_id: "mainagent".to_string(),
        to_node_id: "output".to_string(),
        condition: "always".to_string(),
    });
    definition
}

fn human_approval_template() -> WorkflowDefinition {
    let mut definition =
        WorkflowDefinition::new_draft(template_workflow_id("approval"), "Human Approval Workflow");
    definition.description =
        "A HumanApproval workflow with approved and rejected branches.".to_string();
    definition.nodes.push(WorkflowNode {
        id: "approval".to_string(),
        name: "Approval".to_string(),
        kind: WorkflowNodeKind::HumanApproval,
        config: serde_json::Value::Null,
    });
    definition.nodes.push(WorkflowNode {
        id: "approved".to_string(),
        name: "Approved".to_string(),
        kind: WorkflowNodeKind::Output,
        config: serde_json::json!({ "value": { "branch": "approved" } }),
    });
    definition.nodes.push(WorkflowNode {
        id: "rejected".to_string(),
        name: "Rejected".to_string(),
        kind: WorkflowNodeKind::Output,
        config: serde_json::json!({ "value": { "branch": "rejected" } }),
    });
    definition.edges.push(WorkflowEdge {
        id: "approval_to_approved".to_string(),
        from_node_id: "approval".to_string(),
        to_node_id: "approved".to_string(),
        condition: "approved == true".to_string(),
    });
    definition.edges.push(WorkflowEdge {
        id: "approval_to_rejected".to_string(),
        from_node_id: "approval".to_string(),
        to_node_id: "rejected".to_string(),
        condition: "approved == false".to_string(),
    });

    definition
}

fn publish_workflow_as_capability(
    workflow_id: &str,
    workflow_name: &str,
    workflow_description: &str,
) -> anyhow::Result<String> {
    let capability_id = capability_id_for_workflow(workflow_id);
    let definition = load_workflow_definition(workflow_id)?;
    crate::workflows::validate_publish_ready(&definition)?;
    let db = crate::task_db::Database::new()?;
    let store = crate::workflows::WorkflowStore::new(&db.conn)?;
    store.publish_as_capability(
        workflow_id,
        &capability_id,
        workflow_name,
        workflow_description,
    )?;
    Ok(capability_id)
}

fn validate_and_save_workflow_definition(workflow_id: &str) -> anyhow::Result<()> {
    let definition = load_workflow_definition(workflow_id)?;
    crate::workflows::validate_definition_routing(&definition)?;
    let db = crate::task_db::Database::new()?;
    let store = crate::workflows::WorkflowStore::new(&db.conn)?;
    store.save_draft(&definition)?;
    Ok(())
}

fn capability_id_for_workflow(workflow_id: &str) -> String {
    let suffix = workflow_id
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim_matches('_')
        .to_string();
    format!("capability.{}", suffix)
}

fn load_workflow_definition_json(workflow_id: &str) -> anyhow::Result<String> {
    let definition = load_workflow_definition(workflow_id)?;
    serde_json::to_string_pretty(&definition)
        .map_err(|err| anyhow::anyhow!("failed to format workflow JSON: {}", err))
}

fn load_workflow_definition(workflow_id: &str) -> anyhow::Result<WorkflowDefinition> {
    let db = crate::task_db::Database::new()?;
    let store = crate::workflows::WorkflowStore::new(&db.conn)?;
    let Some(definition) = store.load(workflow_id)? else {
        anyhow::bail!("workflow '{}' not found", workflow_id);
    };
    Ok(definition)
}

fn save_workflow_definition_json(raw_json: &str) -> anyhow::Result<String> {
    let definition: WorkflowDefinition = serde_json::from_str(raw_json)?;
    if definition.status != crate::workflows::WorkflowStatus::Draft {
        anyhow::bail!("only draft workflows can be edited here");
    }
    crate::workflows::validate_definition_routing(&definition)?;
    let workflow_id = definition.id.clone();
    let db = crate::task_db::Database::new()?;
    let store = crate::workflows::WorkflowStore::new(&db.conn)?;
    store.save_draft(&definition)?;
    Ok(workflow_id)
}

async fn run_workflow_draft(workflow_id: String) -> anyhow::Result<serde_json::Value> {
    let definition = load_workflow_definition(&workflow_id)?;
    crate::workflows::validate_definition_routing(&definition)?;
    let run_id = {
        let db = crate::task_db::Database::new()?;
        let run_id =
            crate::task_db::insert_workflow_run(&db.conn, &definition.id, definition.version)?;
        crate::task_db::insert_workflow_run_event(
            &db.conn,
            run_id,
            "draft_run_started",
            &serde_json::to_string(&serde_json::json!({
                "workflow_id": definition.id,
                "workflow_version": definition.version,
                "input": {},
            }))?,
        )?;
        run_id
    };

    let runtime = crate::workflows::WorkflowRuntime::new();
    let result = match runtime
        .run_definition(&definition, serde_json::json!({}))
        .await
    {
        Ok(result) => result,
        Err(err) => {
            let db = crate::task_db::Database::new()?;
            let _ = crate::task_db::insert_workflow_run_event(
                &db.conn,
                run_id,
                "draft_run_failed",
                &serde_json::to_string(&serde_json::json!({
                    "workflow_id": workflow_id,
                    "error": err.to_string(),
                }))?,
            );
            let _ = crate::task_db::finish_workflow_run(
                &db.conn,
                run_id,
                "failed",
                Some(&err.to_string()),
            );
            return Err(err);
        }
    };

    let db = crate::task_db::Database::new()?;
    crate::task_db::insert_workflow_run_event(
        &db.conn,
        run_id,
        "draft_run_finished",
        &serde_json::to_string(&serde_json::json!({
            "workflow_id": workflow_id,
            "result": result.clone(),
        }))?,
    )?;
    if is_awaiting_human_approval_result(&result) {
        Ok(serde_json::json!({
            "status": "awaiting_human_approval",
            "run_id": run_id,
            "workflow_run": result,
        }))
    } else {
        crate::task_db::finish_workflow_run(&db.conn, run_id, "succeeded", None)?;
        Ok(serde_json::json!({
            "status": result
                .get("status")
                .cloned()
                .unwrap_or_else(|| serde_json::json!("succeeded")),
            "run_id": run_id,
            "workflow_run": result,
        }))
    }
}

fn workflow_node_statuses_from_run(
    value: &serde_json::Value,
) -> std::collections::HashMap<String, String> {
    value
        .pointer("/workflow_run/node_status")
        .and_then(|value| value.as_object())
        .map(|statuses| {
            statuses
                .iter()
                .filter_map(|(node_id, status)| {
                    status
                        .as_str()
                        .map(|status| (node_id.clone(), status.to_string()))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn is_awaiting_human_approval_result(value: &serde_json::Value) -> bool {
    value
        .pointer("/result/status")
        .and_then(|status| status.as_str())
        .map(|status| status == "awaiting_human_approval")
        .unwrap_or(false)
}

fn workflow_edit_state_label(app: &AppState, workflow_id: &str) -> String {
    match app.workflow_edit_states.get(workflow_id) {
        Some(state) if state.dirty && state.last_error.is_some() => {
            format!("Save failed: {}", state.last_error.as_deref().unwrap_or(""))
        }
        Some(state) if state.dirty => format!("Unsaved changes: {}", state.reason),
        Some(state) if state.last_error.is_some() => {
            format!("Save failed: {}", state.last_error.as_deref().unwrap_or(""))
        }
        _ => "Saved".to_string(),
    }
}

fn workflow_has_dirty_state(app: &AppState, workflow_id: &str) -> bool {
    app.workflow_edit_states
        .get(workflow_id)
        .map(|state| state.dirty)
        .unwrap_or(false)
}

fn mark_workflow_dirty(
    states: &mut std::collections::HashMap<String, WorkflowEditState>,
    workflow_id: &str,
    reason: impl Into<String>,
) {
    let reason = reason.into();
    log::info!(
        target: "workflow_builder",
        "workflow dirty workflow_id={} reason={}",
        workflow_id,
        reason
    );
    states.insert(
        workflow_id.to_string(),
        WorkflowEditState {
            dirty: true,
            reason,
            last_error: None,
        },
    );
}

fn mark_workflow_saved(
    states: &mut std::collections::HashMap<String, WorkflowEditState>,
    workflow_id: &str,
) {
    log::info!(
        target: "workflow_builder",
        "workflow saved workflow_id={}",
        workflow_id
    );
    states.insert(
        workflow_id.to_string(),
        WorkflowEditState {
            dirty: false,
            reason: "saved".to_string(),
            last_error: None,
        },
    );
}

fn mark_workflow_save_failed(
    states: &mut std::collections::HashMap<String, WorkflowEditState>,
    workflow_id: &str,
    error: impl Into<String>,
) {
    let error = error.into();
    log::warn!(
        target: "workflow_builder",
        "workflow save failed workflow_id={} error={}",
        workflow_id,
        error
    );
    let reason = states
        .get(workflow_id)
        .map(|state| state.reason.clone())
        .unwrap_or_else(|| "changed".to_string());
    states.insert(
        workflow_id.to_string(),
        WorkflowEditState {
            dirty: true,
            reason,
            last_error: Some(error),
        },
    );
}

fn mark_workflow_error(
    states: &mut std::collections::HashMap<String, WorkflowEditState>,
    workflow_id: &str,
    error: impl Into<String>,
) {
    let error = error.into();
    log::warn!(
        target: "workflow_builder",
        "workflow error workflow_id={} error={}",
        workflow_id,
        error
    );
    let existing = states.get(workflow_id).cloned();
    states.insert(
        workflow_id.to_string(),
        WorkflowEditState {
            dirty: existing.as_ref().map(|state| state.dirty).unwrap_or(false),
            reason: existing
                .as_ref()
                .map(|state| state.reason.clone())
                .unwrap_or_else(|| "saved".to_string()),
            last_error: Some(error),
        },
    );
}

fn capability_input_editor(
    app: &AppState,
    window: &mut Window,
    cx: &mut Context<AppState>,
    capability_id: &str,
) -> gpui::Entity<Editor> {
    let key = format!("capability_run_input_{}", capability_id);
    let initial_text = app
        .capability_run_inputs
        .get(capability_id)
        .cloned()
        .unwrap_or_else(|| "{}".to_string());
    window.use_keyed_state(key, &mut *cx, |window, cx| {
        let mut editor = Editor::single_line(window, cx);
        editor.set_placeholder_text("{}", window, cx);
        editor.set_text(initial_text, window, cx);
        editor
    })
}

fn run_button(
    capability_id: String,
    input_editor: gpui::Entity<Editor>,
    cx: &mut Context<AppState>,
) -> impl IntoElement {
    let weak_input_editor = input_editor.downgrade();
    div()
        .px_3()
        .py_1()
        .rounded_lg()
        .bg(GHOST_SURFACE_BG())
        .border_1()
        .border_color(BORDER_LIGHT())
        .text_xs()
        .text_color(ACCENT_TEXT())
        .font_weight(gpui::FontWeight::BOLD)
        .cursor_pointer()
        .child("Run")
        .on_mouse_down(
            gpui::MouseButton::Left,
            cx.listener(move |this, _: &gpui::MouseDownEvent, _window, cx| {
                let capability_id = capability_id.clone();
                let input_text = weak_input_editor
                    .upgrade()
                    .map(|editor| editor.read_with(cx, |editor, cx| editor.text(cx)))
                    .unwrap_or_else(|| "{}".to_string());
                let input = match serde_json::from_str::<serde_json::Value>(&input_text) {
                    Ok(value) => value,
                    Err(err) => {
                        this.push_toast(
                            ToastLevel::Error,
                            format!("Invalid capability input JSON: {}", err),
                            cx,
                        );
                        cx.notify();
                        return;
                    }
                };
                this.capability_run_inputs
                    .insert(capability_id.clone(), input_text);
                this.push_toast(
                    ToastLevel::Info,
                    format!("Running capability {}...", capability_id),
                    cx,
                );
                cx.spawn(async move |this, cx| {
                    let result = crate::workflows::run_capability(&capability_id, input).await;
                    let _ = this.update(cx, |this, cx| match result {
                        Ok(value) => this.push_toast(
                            ToastLevel::Success,
                            format!("Capability finished: {}", value["status"]),
                            cx,
                        ),
                        Err(err) => this.push_toast(
                            ToastLevel::Error,
                            format!("Capability failed: {}", err),
                            cx,
                        ),
                    });
                })
                .detach();
            }),
        )
}

fn tab_button(
    app: &AppState,
    label: &'static str,
    active: bool,
    target: CapabilitiesTab,
    cx: &mut Context<AppState>,
) -> impl IntoElement {
    let active_workflow_id = app.editing_workflow_id.clone();
    let leaving_dirty_workflow = target != CapabilitiesTab::Workflows
        && active_workflow_id
            .as_deref()
            .map(|workflow_id| workflow_has_dirty_state(app, workflow_id))
            .unwrap_or(false);
    div()
        .px_4()
        .py_1()
        .rounded_lg()
        .when(active, |this| this.bg(SURFACE_PANEL()).shadow_sm())
        .text_xs()
        .text_color(if active {
            ACCENT_TEXT()
        } else {
            SECONDARY_TEXT()
        })
        .font_weight(gpui::FontWeight::BOLD)
        .cursor_pointer()
        .on_mouse_down(
            gpui::MouseButton::Left,
            cx.listener(move |this, _: &gpui::MouseDownEvent, _w, cx| {
                if leaving_dirty_workflow {
                    if let Some(workflow_id) = active_workflow_id.as_ref() {
                        this.push_toast(
                            ToastLevel::Warning,
                            format!("Workflow {} has unsaved changes.", workflow_id),
                            cx,
                        );
                    }
                }
                this.capabilities_tab = target;
                cx.notify();
            }),
        )
        .child(label)
}

fn empty_state(icon: &'static str, title: &'static str, detail: &'static str) -> impl IntoElement {
    div()
        .p_12()
        .rounded_2xl()
        .border_1()
        .border_dashed()
        .border_color(BORDER_LIGHT())
        .bg(SURFACE_PANEL())
        .flex_col()
        .items_center()
        .gap_4()
        .child(crate::render_icon_element(icon, MUTED_TEXT(), 32.0))
        .child(
            div()
                .text_base()
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(PRIMARY_TEXT())
                .child(title),
        )
        .child(
            div()
                .max_w(px(460.0))
                .text_center()
                .text_sm()
                .line_height(relative(1.5))
                .text_color(SECONDARY_TEXT())
                .child(detail),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_local_agent_default_config_is_complete() {
        let node = empty_local_agent_node("local_agent", "Local Agent");

        assert_eq!(node.id, "local_agent");
        assert_eq!(node.name, "Local Agent");
        assert!(matches!(node.kind, WorkflowNodeKind::Agent { .. }));
        assert_eq!(node.config["model"]["temperature"], 0.2);
        assert_eq!(node.config["tools"]["skills"].as_array().unwrap().len(), 0);
        assert_eq!(node.config["routing"]["mode"], "sequential");
        assert_eq!(node.config["output"]["summarize_with_mainagent"], true);
    }

    #[test]
    fn next_workflow_node_id_deduplicates_local_agents() {
        let mut definition = WorkflowDefinition::new_draft("workflow.test", "Test");
        definition
            .nodes
            .push(empty_local_agent_node("local_agent", "Local Agent"));
        definition
            .nodes
            .push(empty_local_agent_node("local_agent_2", "Local Agent 2"));

        assert_eq!(
            next_workflow_node_id(&definition, "local_agent"),
            "local_agent_3"
        );
    }

    #[test]
    fn apply_agent_node_update_saves_config() {
        let mut definition = WorkflowDefinition::new_draft("workflow.test", "Test");
        definition
            .nodes
            .push(empty_local_agent_node("local_agent", "Local Agent"));

        apply_workflow_agent_node_update(
            &mut definition,
            "local_agent",
            WorkflowAgentNodeUpdate {
                name: "Planner".to_string(),
                description: "Plans the workflow".to_string(),
                category: "planning".to_string(),
                tags: "plan, review".to_string(),
                version: "1.0.0".to_string(),
                model_provider: "openai".to_string(),
                model_name: "gpt-5".to_string(),
                temperature: "0.4".to_string(),
                max_tokens: "2048".to_string(),
                timeout_seconds: "90".to_string(),
                system_prompt: "You plan work.".to_string(),
                instructions: "Return concise plans.".to_string(),
                output_format: "json".to_string(),
                output_schema: r#"{"type":"object"}"#.to_string(),
                summarize_with_mainagent: "false".to_string(),
                skills_json: r#"["skill.a"]"#.to_string(),
                mcp_tools_json: r#"[]"#.to_string(),
                system_tools_json: r#"["read_file"]"#.to_string(),
                coding_runtimes_json: r#"["claude"]"#.to_string(),
                retry: "2".to_string(),
                settings_timeout_seconds: "180".to_string(),
                human_confirmation: "true".to_string(),
                routing_policy_json: r#"{"mode":"sequential"}"#.to_string(),
                permissions: "ask".to_string(),
            },
        )
        .unwrap();

        let node = definition
            .nodes
            .iter()
            .find(|node| node.id == "local_agent")
            .unwrap();
        assert_eq!(node.name, "Planner");
        assert_eq!(node.config["description"], "Plans the workflow");
        assert_eq!(node.config["metadata"]["category"], "planning");
        assert_eq!(node.config["metadata"]["tags"][0], "plan");
        assert_eq!(node.config["metadata"]["version"], "1.0.0");
        assert_eq!(node.config["model"]["provider"], "openai");
        assert_eq!(node.config["model"]["model"], "gpt-5");
        assert_eq!(node.config["model"]["temperature"], 0.4);
        assert_eq!(node.config["model"]["max_tokens"], 2048);
        assert_eq!(node.config["prompt"]["system"], "You plan work.");
        assert_eq!(node.config["output"]["format"], "json");
        assert_eq!(node.config["output"]["schema"]["type"], "object");
        assert_eq!(node.config["output"]["summarize_with_mainagent"], false);
        assert_eq!(node.config["tools"]["skills"][0], "skill.a");
        assert_eq!(node.config["tools"]["system_tools"][0], "read_file");
        assert_eq!(node.config["tools"]["coding_runtimes"][0], "claude");
        assert_eq!(node.config["settings"]["retry"], 2);
        assert_eq!(node.config["settings"]["timeout_seconds"], 180);
        assert_eq!(node.config["settings"]["human_confirmation"], true);
        assert_eq!(node.config["routing"]["mode"], "sequential");
    }

    #[test]
    fn apply_agent_node_update_rejects_invalid_node_id() {
        let mut definition = WorkflowDefinition::new_draft("workflow.test", "Test");
        definition
            .nodes
            .push(empty_local_agent_node("local_agent", "Local Agent"));

        let err = apply_workflow_agent_node_update(
            &mut definition,
            "missing",
            WorkflowAgentNodeUpdate {
                name: "Planner".to_string(),
                description: String::new(),
                category: String::new(),
                tags: String::new(),
                version: "0.1.0".to_string(),
                model_provider: "default".to_string(),
                model_name: "default".to_string(),
                temperature: "0.2".to_string(),
                max_tokens: "4096".to_string(),
                timeout_seconds: "120".to_string(),
                system_prompt: String::new(),
                instructions: String::new(),
                output_format: "text".to_string(),
                output_schema: "null".to_string(),
                summarize_with_mainagent: "true".to_string(),
                skills_json: "[]".to_string(),
                mcp_tools_json: "[]".to_string(),
                system_tools_json: "[]".to_string(),
                coding_runtimes_json: "[]".to_string(),
                retry: "0".to_string(),
                settings_timeout_seconds: "120".to_string(),
                human_confirmation: "false".to_string(),
                routing_policy_json: r#"{"mode":"sequential"}"#.to_string(),
                permissions: "ask".to_string(),
            },
        )
        .unwrap_err();

        assert!(err.to_string().contains("not found"));
    }

    #[test]
    fn create_workflow_edge_adds_default_edge() {
        let mut definition = WorkflowDefinition::new_draft("workflow.test", "Test");
        definition
            .nodes
            .push(empty_local_agent_node("local_agent", "Local Agent"));
        definition
            .nodes
            .push(empty_local_agent_node("local_agent_2", "Local Agent 2"));

        let edge_id =
            apply_create_workflow_edge(&mut definition, "local_agent", "local_agent_2").unwrap();

        assert_eq!(edge_id, "local_agent_to_local_agent_2");
        assert_eq!(definition.edges.len(), 1);
        assert_eq!(definition.edges[0].condition, "always");
    }

    #[test]
    fn delete_workflow_edge_removes_edge() {
        let mut definition = WorkflowDefinition::new_draft("workflow.test", "Test");
        definition
            .nodes
            .push(empty_local_agent_node("local_agent", "Local Agent"));
        definition
            .nodes
            .push(empty_local_agent_node("local_agent_2", "Local Agent 2"));
        let edge_id =
            apply_create_workflow_edge(&mut definition, "local_agent", "local_agent_2").unwrap();

        apply_delete_workflow_edge(&mut definition, &edge_id).unwrap();

        assert!(definition.edges.is_empty());
    }

    #[test]
    fn create_workflow_edge_rejects_invalid_edge() {
        let mut definition = WorkflowDefinition::new_draft("workflow.test", "Test");
        definition
            .nodes
            .push(empty_local_agent_node("local_agent", "Local Agent"));

        let missing_target =
            apply_create_workflow_edge(&mut definition, "local_agent", "missing").unwrap_err();
        assert!(missing_target.to_string().contains("target node"));

        let self_loop =
            apply_create_workflow_edge(&mut definition, "local_agent", "local_agent").unwrap_err();
        assert!(self_loop.to_string().contains("self-loop"));
    }

    #[test]
    fn workflow_edit_state_tracks_dirty_failed_and_saved() {
        let mut states = std::collections::HashMap::new();

        mark_workflow_dirty(&mut states, "workflow.test", "Added local Agent");
        assert_eq!(states["workflow.test"].dirty, true);
        assert_eq!(states["workflow.test"].reason, "Added local Agent");
        assert_eq!(states["workflow.test"].last_error, None);

        mark_workflow_save_failed(&mut states, "workflow.test", "invalid routing");
        assert_eq!(states["workflow.test"].dirty, true);
        assert_eq!(states["workflow.test"].reason, "Added local Agent");
        assert_eq!(
            states["workflow.test"].last_error.as_deref(),
            Some("invalid routing")
        );

        mark_workflow_saved(&mut states, "workflow.test");
        assert_eq!(states["workflow.test"].dirty, false);
        assert_eq!(states["workflow.test"].reason, "saved");
        assert_eq!(states["workflow.test"].last_error, None);
    }
}
