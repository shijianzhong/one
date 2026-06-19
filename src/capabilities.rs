use editor::Editor;
use gpui::{
    div, prelude::*, px, relative, AnyElement, ClipboardItem, Context, InteractiveElement,
    ParentElement, Styled, Window,
};

use crate::i18n::{t, Translations};
use crate::ui_theme::{
    ACCENT_TEXT, BORDER_LIGHT, BRAND_BLUE, CANVAS_BG, GHOST_SURFACE_BG, MUTED_TEXT, PRIMARY_TEXT,
    SECONDARY_TEXT, SURFACE_PANEL,
};
use crate::workflows::{WorkflowDefinition, WorkflowEdge, WorkflowNode, WorkflowNodeKind};
use crate::{AppState, CapabilitiesTab, ToastLevel};

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
                    t(lang, Translations::CAPABILITY_LIBRARY),
                    library_active,
                    CapabilitiesTab::Library,
                    cx,
                ))
                .child(tab_button(
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

            match load_workflow_drafts() {
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
            Ok(id) => this.push_toast(ToastLevel::Success, format!("Created workflow {}", id), cx),
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

#[derive(Debug, Clone, Copy)]
enum WorkflowQuickNodeKind {
    Output,
    HumanApproval,
    MainAgent,
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
                                    this.push_toast(
                                        ToastLevel::Success,
                                        format!("Saved workflow {}", saved_id),
                                        cx,
                                    );
                                }
                                Err(err) => this.push_toast(
                                    ToastLevel::Error,
                                    format!("Failed to save workflow JSON: {}", err),
                                    cx,
                                ),
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
    let workflow_id = definition.id.clone();
    let db = crate::task_db::Database::new()?;
    let store = crate::workflows::WorkflowStore::new(&db.conn)?;
    store.save_draft(&definition)?;
    Ok(workflow_id)
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
    label: &'static str,
    active: bool,
    target: CapabilitiesTab,
    cx: &mut Context<AppState>,
) -> impl IntoElement {
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
