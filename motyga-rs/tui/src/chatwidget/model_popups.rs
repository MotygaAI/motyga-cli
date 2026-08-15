//! Model, collaboration, and reasoning popups for `ChatWidget`.
//!
//! These surfaces are tightly related because changing one often redirects
//! into another, especially while Plan mode is active.

use super::*;
use motyga_utils_fuzzy_match::fuzzy_match;

impl ChatWidget {
    /// Open a popup to choose a quick auto model. Selecting "All models"
    /// opens the full picker with every available preset.
    pub(crate) fn open_model_popup(&mut self) {
        if !self.is_session_configured() {
            self.add_info_message(
                "Model selection is disabled until startup completes.".to_string(),
                /*hint*/ None,
            );
            return;
        }

        let presets: Vec<ModelPreset> = match self.model_catalog.try_list_models() {
            Ok(models) => models,
            Err(_) => {
                self.add_info_message(
                    "Models are being updated; please try /model again in a moment.".to_string(),
                    /*hint*/ None,
                );
                return;
            }
        };
        self.open_model_popup_with_presets(presets);
    }

    /// Apply `/model <name>` directly, without opening the picker.
    ///
    /// The argument used to be dropped on the floor: the picker opened, the name was discarded,
    /// and the session kept running the previous model with no indication anything was ignored.
    /// A name the catalog does not list switches the session anyway, the same way `-m` does.
    pub(crate) fn apply_model_slash_arg(&mut self, requested: &str) {
        if !self.is_session_configured() {
            self.add_info_message(
                "Model selection is disabled until startup completes.".to_string(),
                /*hint*/ None,
            );
            return;
        }

        // A model id is a single token. Anything with whitespace in it is a sentence, not a name,
        // and switching the session to it would be a silent way to break every following turn.
        if requested.split_whitespace().count() > 1 {
            self.add_error_message(format!(
                "'{requested}' is not a model id. Usage: /model <model-id>, for example /model gpt-5.5. Run /model with no argument to browse the catalog."
            ));
            return;
        }

        let presets: Vec<ModelPreset> = match self.model_catalog.try_list_models() {
            Ok(models) => models,
            Err(_) => {
                self.add_info_message(
                    "Models are being updated; please try /model again in a moment.".to_string(),
                    /*hint*/ None,
                );
                return;
            }
        };

        // Match the catalog id the user typed. Deliberately not filtered by `show_in_picker`:
        // naming a model outright is a more explicit act than browsing for one.
        let Some(preset) = match_model_arg(&presets, requested) else {
            self.switch_to_uncatalogued_model(requested, &presets);
            return;
        };

        let effort = Some(preset.default_reasoning_effort.clone());
        let should_prompt_plan_mode_scope =
            self.should_prompt_plan_mode_reasoning_scope(preset.model.as_str(), effort.clone());
        for action in
            Self::model_selection_actions(preset.model, effort, should_prompt_plan_mode_scope)
        {
            action(&self.app_event_tx);
        }
    }

    /// Switch to a model id the local catalog does not list.
    ///
    /// The catalog only holds what the last `/models` refresh returned, and that call falls back to
    /// the small bundled list whenever the gateway is unreachable or the key is rejected — so an id
    /// we cannot resolve locally is not an id the backend cannot run. `motyga -m <id>` has always
    /// accepted an unlisted id and let the server decide; typing it into `/model` now does the same
    /// instead of refusing and sending the user back to the command line.
    ///
    /// Reasoning effort carries over unchanged: an unlisted model brings no default to apply, and
    /// passing `None` would clear `model_reasoning_effort` from config.toml as a side effect.
    fn switch_to_uncatalogued_model(&mut self, requested: &str, presets: &[ModelPreset]) {
        let suggestions = suggest_model_ids(presets, requested);
        let hint = if suggestions.is_empty() {
            "Run /model with no argument to browse the models this session knows about.".to_string()
        } else {
            format!(
                "Close catalog ids: {}. Run /model with no argument to browse them.",
                suggestions.join(", ")
            )
        };
        self.add_info_message(
            format!(
                "'{requested}' is not in this session's model catalog — switching anyway, the server decides whether it can serve it."
            ),
            Some(hint),
        );

        let effort = self.config.model_reasoning_effort.clone();
        let should_prompt_plan_mode_scope =
            self.should_prompt_plan_mode_reasoning_scope(requested, effort.clone());
        for action in Self::model_selection_actions(
            requested.to_string(),
            effort,
            should_prompt_plan_mode_scope,
        ) {
            action(&self.app_event_tx);
        }
    }

    fn model_menu_header(&self, title: &str, subtitle: &str) -> Box<dyn Renderable> {
        let title = title.to_string();
        let subtitle = subtitle.to_string();
        let mut header = ColumnRenderable::new();
        header.push(Line::from(title.bold()));
        header.push(Line::from(subtitle.dim()));
        if let Some(warning) = self.model_menu_warning_line() {
            header.push(warning);
        }
        Box::new(header)
    }

    fn model_menu_warning_line(&self) -> Option<Line<'static>> {
        let base_url = self.custom_openai_base_url()?;
        let warning = format!(
            "Warning: OpenAI base URL is overridden to {base_url}. Selecting models may not be supported or work properly."
        );
        Some(Line::from(warning.red()))
    }

    fn custom_openai_base_url(&self) -> Option<String> {
        if !self.config.model_provider.is_openai() {
            return None;
        }

        let base_url = self.config.model_provider.base_url.as_ref()?;
        let trimmed = base_url.trim();
        if trimmed.is_empty() {
            return None;
        }

        let normalized = trimmed.trim_end_matches('/');
        if normalized == DEFAULT_OPENAI_BASE_URL {
            return None;
        }

        Some(trimmed.to_string())
    }

    pub(crate) fn open_model_popup_with_presets(&mut self, presets: Vec<ModelPreset>) {
        let presets: Vec<ModelPreset> = presets
            .into_iter()
            .filter(|preset| preset.show_in_picker)
            .collect();

        let current_model = self.current_model();
        let current_label = presets
            .iter()
            .find(|preset| preset.model.as_str() == current_model)
            .map(|preset| preset.model.to_string())
            .unwrap_or_else(|| self.model_display_name().to_string());

        let (mut auto_presets, other_presets): (Vec<ModelPreset>, Vec<ModelPreset>) = presets
            .into_iter()
            .partition(|preset| Self::is_auto_model(&preset.model));

        if auto_presets.is_empty() {
            self.open_all_models_popup(other_presets);
            return;
        }

        auto_presets.sort_by_key(|preset| Self::auto_model_order(&preset.model));
        let mut items: Vec<SelectionItem> = auto_presets
            .into_iter()
            .map(|preset| {
                let description =
                    (!preset.description.is_empty()).then_some(preset.description.clone());
                let model = preset.model.clone();
                let should_prompt_plan_mode_scope = self.should_prompt_plan_mode_reasoning_scope(
                    model.as_str(),
                    Some(preset.default_reasoning_effort.clone()),
                );
                let actions = Self::model_selection_actions(
                    model.clone(),
                    Some(preset.default_reasoning_effort.clone()),
                    should_prompt_plan_mode_scope,
                );
                SelectionItem {
                    name: model.clone(),
                    description,
                    is_current: model.as_str() == current_model,
                    is_default: preset.is_default,
                    actions,
                    dismiss_on_select: true,
                    ..Default::default()
                }
            })
            .collect();

        if !other_presets.is_empty() {
            let all_models = other_presets;
            let actions: Vec<SelectionAction> = vec![Box::new(move |tx| {
                tx.send(AppEvent::OpenAllModelsPopup {
                    models: all_models.clone(),
                });
            })];

            let is_current = !items.iter().any(|item| item.is_current);
            let description = Some(format!(
                "Choose a specific model and reasoning level (current: {current_label})"
            ));

            items.push(SelectionItem {
                name: "All models".to_string(),
                description,
                is_current,
                actions,
                dismiss_on_select: true,
                ..Default::default()
            });
        }

        let header = self.model_menu_header(
            "Select Model",
            "Pick a quick auto mode or browse all models.",
        );
        self.bottom_pane.show_selection_view(SelectionViewParams {
            footer_hint: Some(standard_popup_hint_line()),
            items,
            header,
            ..Default::default()
        });
    }

    fn is_auto_model(model: &str) -> bool {
        model.starts_with("motyga-auto-")
    }

    fn auto_model_order(model: &str) -> usize {
        match model {
            "motyga-auto-fast" => 0,
            "motyga-auto-balanced" => 1,
            "motyga-auto-thorough" => 2,
            _ => 3,
        }
    }

    pub(crate) fn open_all_models_popup(&mut self, presets: Vec<ModelPreset>) {
        if presets.is_empty() {
            self.add_info_message(
                "No additional models are available right now.".to_string(),
                /*hint*/ None,
            );
            return;
        }

        let mut items: Vec<SelectionItem> = Vec::new();
        for preset in presets.into_iter() {
            let description =
                (!preset.description.is_empty()).then_some(preset.description.to_string());
            let is_current = preset.model.as_str() == self.current_model();
            let single_supported_effort = preset.supported_reasoning_efforts.len() == 1;
            let preset_for_action = preset.clone();
            let actions: Vec<SelectionAction> = vec![Box::new(move |tx| {
                let preset_for_event = preset_for_action.clone();
                tx.send(AppEvent::OpenReasoningPopup {
                    model: preset_for_event,
                });
            })];
            items.push(SelectionItem {
                name: preset.model.clone(),
                description,
                is_current,
                is_default: preset.is_default,
                actions,
                dismiss_on_select: single_supported_effort,
                dismiss_parent_on_child_accept: !single_supported_effort,
                ..Default::default()
            });
        }

        let header = self.model_menu_header(
            "Select Model and Effort",
            "Not listed? Run /model <model-id> to switch to any model the server serves.",
        );
        self.bottom_pane.show_selection_view(SelectionViewParams {
            footer_hint: Some(self.bottom_pane.standard_popup_hint_line()),
            items,
            header,
            ..Default::default()
        });
    }

    fn model_selection_actions(
        model_for_action: String,
        effort_for_action: Option<ReasoningEffortConfig>,
        should_prompt_plan_mode_scope: bool,
    ) -> Vec<SelectionAction> {
        vec![Box::new(move |tx| {
            if should_prompt_plan_mode_scope {
                tx.send(AppEvent::OpenPlanReasoningScopePrompt {
                    model: model_for_action.clone(),
                    effort: effort_for_action.clone(),
                });
                return;
            }

            tx.send(AppEvent::UpdateModel(model_for_action.clone()));
            tx.send(AppEvent::UpdateReasoningEffort(effort_for_action.clone()));
            tx.send(AppEvent::PersistModelSelection {
                model: model_for_action.clone(),
                effort: effort_for_action.clone(),
            });
        })]
    }

    fn should_prompt_plan_mode_reasoning_scope(
        &self,
        selected_model: &str,
        selected_effort: Option<ReasoningEffortConfig>,
    ) -> bool {
        if !self.collaboration_modes_enabled()
            || self.active_mode_kind() != ModeKind::Plan
            || selected_model != self.current_model()
        {
            return false;
        }

        // Prompt whenever the selection is not a true no-op for both:
        // 1) the active Plan-mode effective reasoning, and
        // 2) the stored global defaults that would be updated by the fallback path.
        selected_effort != self.effective_reasoning_effort()
            || selected_model != self.current_collaboration_mode.model()
            || selected_effort != self.current_collaboration_mode.reasoning_effort()
    }

    pub(crate) fn open_plan_reasoning_scope_prompt(
        &mut self,
        model: String,
        effort: Option<ReasoningEffortConfig>,
    ) {
        let reasoning_phrase = match effort.as_ref() {
            Some(ReasoningEffortConfig::None) => "no reasoning".to_string(),
            Some(selected_effort) => {
                format!(
                    "{} reasoning",
                    Self::reasoning_effort_sentence_label(selected_effort)
                )
            }
            None => "the selected reasoning".to_string(),
        };
        let plan_only_description = format!("Always use {reasoning_phrase} in Plan mode.");
        let plan_reasoning_source = if let Some(plan_override) =
            self.config.plan_mode_reasoning_effort.as_ref()
        {
            format!(
                "user-chosen Plan override ({})",
                Self::reasoning_effort_sentence_label(plan_override)
            )
        } else if let Some(plan_mask) = collaboration_modes::plan_mask(self.model_catalog.as_ref())
        {
            match plan_mask
                .reasoning_effort
                .as_ref()
                .and_then(|effort| effort.as_ref())
            {
                Some(plan_effort) => format!(
                    "built-in Plan default ({})",
                    Self::reasoning_effort_sentence_label(plan_effort)
                ),
                None => "built-in Plan default (no reasoning)".to_string(),
            }
        } else {
            "built-in Plan default".to_string()
        };
        let all_modes_description = format!(
            "Set the global default reasoning level and the Plan mode override. This replaces the current {plan_reasoning_source}."
        );
        let subtitle = format!("Choose where to apply {reasoning_phrase}.");

        let plan_only_actions: Vec<SelectionAction> = vec![Box::new({
            let model = model.clone();
            let effort = effort.clone();
            move |tx| {
                tx.send(AppEvent::UpdateModel(model.clone()));
                tx.send(AppEvent::UpdatePlanModeReasoningEffort(effort.clone()));
                tx.send(AppEvent::PersistPlanModeReasoningEffort(effort.clone()));
            }
        })];
        let all_modes_actions: Vec<SelectionAction> = vec![Box::new(move |tx| {
            tx.send(AppEvent::UpdateModel(model.clone()));
            tx.send(AppEvent::UpdateReasoningEffort(effort.clone()));
            tx.send(AppEvent::UpdatePlanModeReasoningEffort(effort.clone()));
            tx.send(AppEvent::PersistPlanModeReasoningEffort(effort.clone()));
            tx.send(AppEvent::PersistModelSelection {
                model: model.clone(),
                effort: effort.clone(),
            });
        })];

        self.bottom_pane.show_selection_view(SelectionViewParams {
            title: Some(PLAN_MODE_REASONING_SCOPE_TITLE.to_string()),
            subtitle: Some(subtitle),
            footer_hint: Some(standard_popup_hint_line()),
            items: vec![
                SelectionItem {
                    name: PLAN_MODE_REASONING_SCOPE_PLAN_ONLY.to_string(),
                    description: Some(plan_only_description),
                    actions: plan_only_actions,
                    dismiss_on_select: true,
                    ..Default::default()
                },
                SelectionItem {
                    name: PLAN_MODE_REASONING_SCOPE_ALL_MODES.to_string(),
                    description: Some(all_modes_description),
                    actions: all_modes_actions,
                    dismiss_on_select: true,
                    ..Default::default()
                },
            ],
            ..Default::default()
        });
        self.notify(Notification::PlanModePrompt {
            title: PLAN_MODE_REASONING_SCOPE_TITLE.to_string(),
        });
    }

    /// Open a popup to choose the reasoning effort (stage 2) for the given model.
    pub(crate) fn open_reasoning_popup(&mut self, preset: ModelPreset) {
        let default_effort = preset.default_reasoning_effort;
        let supported = preset.supported_reasoning_efforts;
        let in_plan_mode =
            self.collaboration_modes_enabled() && self.active_mode_kind() == ModeKind::Plan;

        let warn_effort = if supported
            .iter()
            .any(|option| option.effort == ReasoningEffortConfig::XHigh)
        {
            Some(ReasoningEffortConfig::XHigh)
        } else if supported
            .iter()
            .any(|option| option.effort == ReasoningEffortConfig::High)
        {
            Some(ReasoningEffortConfig::High)
        } else {
            None
        };
        let warning_text = warn_effort.as_ref().map(|effort| {
            let effort_label = Self::reasoning_effort_label(effort);
            format!("⚠ {effort_label} reasoning effort can quickly consume Plus plan rate limits.")
        });
        let warn_for_model = preset.model.starts_with("gpt-5.1-codex")
            || preset.model.starts_with("gpt-5.1-codex-max")
            || preset.model.starts_with("gpt-5.2");

        let mut choices: Vec<ReasoningEffortConfig> = supported
            .iter()
            .map(|option| option.effort.clone())
            .collect();
        if choices.is_empty() {
            choices.push(default_effort.clone());
        }

        if choices.len() == 1 {
            let selected_effort = choices.first().cloned();
            let selected_model = preset.model;
            if self
                .should_prompt_plan_mode_reasoning_scope(&selected_model, selected_effort.clone())
            {
                self.app_event_tx
                    .send(AppEvent::OpenPlanReasoningScopePrompt {
                        model: selected_model,
                        effort: selected_effort,
                    });
            } else {
                self.apply_model_and_effort(selected_model, selected_effort);
            }
            return;
        }

        let default_choice = choices
            .contains(&default_effort)
            .then(|| default_effort.clone())
            .or_else(|| choices.first().cloned())
            .or(Some(default_effort));

        let model_slug = preset.model.to_string();
        let is_current_model = self.current_model() == preset.model.as_str();
        let highlight_choice = if is_current_model {
            if in_plan_mode {
                self.config
                    .plan_mode_reasoning_effort
                    .clone()
                    .or_else(|| self.effective_reasoning_effort())
            } else {
                self.effective_reasoning_effort()
            }
        } else {
            default_choice.clone()
        };
        let selection_choice = highlight_choice.clone().or_else(|| default_choice.clone());
        let initial_selected_idx = choices
            .iter()
            .position(|choice| Some(choice) == selection_choice.as_ref());
        let mut items: Vec<SelectionItem> = Vec::new();
        for choice in choices.iter() {
            let effort = choice.clone();
            let mut effort_label = Self::reasoning_effort_label(&effort);
            if Some(choice) == default_choice.as_ref() {
                effort_label.push_str(" (default)");
            }

            let description = supported
                .iter()
                .find(|option| option.effort == effort)
                .map(|option| option.description.to_string())
                .filter(|text| !text.is_empty());

            let show_warning = warn_for_model && warn_effort.as_ref() == Some(&effort);
            let selected_description = if show_warning {
                warning_text.as_ref().map(|warning_message| {
                    description.as_ref().map_or_else(
                        || warning_message.clone(),
                        |d| format!("{d}\n{warning_message}"),
                    )
                })
            } else {
                None
            };

            let model_for_action = model_slug.clone();
            let choice_effort = Some(effort);
            let should_prompt_plan_mode_scope = self.should_prompt_plan_mode_reasoning_scope(
                model_slug.as_str(),
                choice_effort.clone(),
            );
            let actions: Vec<SelectionAction> = vec![Box::new(move |tx| {
                if should_prompt_plan_mode_scope {
                    tx.send(AppEvent::OpenPlanReasoningScopePrompt {
                        model: model_for_action.clone(),
                        effort: choice_effort.clone(),
                    });
                } else {
                    tx.send(AppEvent::UpdateModel(model_for_action.clone()));
                    tx.send(AppEvent::UpdateReasoningEffort(choice_effort.clone()));
                    tx.send(AppEvent::PersistModelSelection {
                        model: model_for_action.clone(),
                        effort: choice_effort.clone(),
                    });
                }
            })];

            items.push(SelectionItem {
                name: effort_label,
                description,
                selected_description,
                is_current: is_current_model && Some(choice) == highlight_choice.as_ref(),
                actions,
                dismiss_on_select: true,
                ..Default::default()
            });
        }

        let mut header = ColumnRenderable::new();
        header.push(Line::from(
            format!("Select Reasoning Level for {model_slug}").bold(),
        ));

        self.bottom_pane.show_selection_view(SelectionViewParams {
            header: Box::new(header),
            footer_hint: Some(standard_popup_hint_line()),
            items,
            initial_selected_idx,
            ..Default::default()
        });
    }

    pub(super) fn reasoning_effort_label(effort: &ReasoningEffortConfig) -> String {
        match effort {
            ReasoningEffortConfig::None => "None".to_string(),
            ReasoningEffortConfig::Minimal => "Minimal".to_string(),
            ReasoningEffortConfig::Low => "Low".to_string(),
            ReasoningEffortConfig::Medium => "Medium".to_string(),
            ReasoningEffortConfig::High => "High".to_string(),
            ReasoningEffortConfig::XHigh => "Extra high".to_string(),
            ReasoningEffortConfig::Max => "Max".to_string(),
            ReasoningEffortConfig::Ultra => "Ultra".to_string(),
            ReasoningEffortConfig::Custom(value) => value.clone(),
        }
    }

    pub(super) fn reasoning_effort_sentence_label(effort: &ReasoningEffortConfig) -> String {
        match effort {
            ReasoningEffortConfig::Custom(value) => value.clone(),
            effort => Self::reasoning_effort_label(effort).to_lowercase(),
        }
    }

    pub(super) fn apply_model_and_effort_without_persist(
        &self,
        model: String,
        effort: Option<ReasoningEffortConfig>,
    ) {
        self.app_event_tx.send(AppEvent::UpdateModel(model));
        self.app_event_tx
            .send(AppEvent::UpdateReasoningEffort(effort));
    }

    fn apply_model_and_effort(&self, model: String, effort: Option<ReasoningEffortConfig>) {
        self.apply_model_and_effort_without_persist(model.clone(), effort.clone());
        self.app_event_tx
            .send(AppEvent::PersistModelSelection { model, effort });
    }
}

/// Collapse an id to just its alphanumerics, so ids that differ only in separators or case compare
/// equal. The catalog spells versions both ways depending on the vendor (`claude-opus-4-5` vs the
/// label's "Claude Opus 4.5"), and a user typing the version they SEE should not be a dead end.
fn normalized_model_id(id: &str) -> String {
    id.chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// Resolve `/model <name>` against the catalog: exact id first, then an id that is identical once
/// separators and case are stripped — but only when exactly one model matches, so an ambiguous
/// shorthand still errors instead of silently picking one.
fn match_model_arg(presets: &[ModelPreset], requested: &str) -> Option<ModelPreset> {
    if let Some(exact) = presets
        .iter()
        .find(|preset| preset.model.eq_ignore_ascii_case(requested))
    {
        return Some(exact.clone());
    }
    let wanted = normalized_model_id(requested);
    if wanted.is_empty() {
        return None;
    }
    let mut normalized = presets
        .iter()
        .filter(|preset| normalized_model_id(&preset.model) == wanted);
    let only = normalized.next()?;
    normalized.next().is_none().then(|| only.clone())
}

/// Catalog ids an unmatched `/model` argument most plausibly meant, best first (at most three).
///
/// Ranked with the same subsequence matcher the pickers use, run in BOTH directions: the catalog id
/// inside what was typed catches a name carrying extra words the id never had (`chatgpt-5.6-sol`
/// finds `gpt-5.6-sol`), and what was typed inside the catalog id catches truncation and typos.
fn suggest_model_ids(presets: &[ModelPreset], requested: &str) -> Vec<String> {
    if requested.is_empty() {
        return Vec::new();
    }
    let mut scored: Vec<(i32, &str)> = presets
        .iter()
        .filter_map(|preset| {
            let id = preset.model.as_str();
            let inside_request = fuzzy_match(requested, id).map(|(_, score)| score);
            let inside_id = fuzzy_match(id, requested).map(|(_, score)| score);
            let best = match (inside_request, inside_id) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (found, None) | (None, found) => found,
            }?;
            Some((best, id))
        })
        .collect();
    // Stable by score, then by id, so the same typo always produces the same advice.
    scored.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(b.1)));
    scored.dedup_by(|a, b| a.1 == b.1);
    scored
        .into_iter()
        .take(3)
        .map(|(_, id)| format!("'{id}'"))
        .collect()
}

#[cfg(test)]
mod model_arg_tests {
    use super::*;
    use motyga_protocol::openai_models::default_input_modalities;

    fn preset(slug: &str) -> ModelPreset {
        ModelPreset {
            id: slug.to_string(),
            model: slug.to_string(),
            display_name: slug.to_string(),
            description: String::new(),
            default_reasoning_effort: ReasoningEffortConfig::Medium,
            supported_reasoning_efforts: Vec::new(),
            supports_personality: false,
            additional_speed_tiers: Vec::new(),
            service_tiers: Vec::new(),
            default_service_tier: None,
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            availability_nux: None,
            supported_in_api: true,
            input_modalities: default_input_modalities(),
        }
    }

    fn catalog() -> Vec<ModelPreset> {
        [
            "gpt-5.6-sol",
            "gpt-5.6-terra",
            "gpt-5.6-luna",
            "gpt-5.5",
            "claude-opus-4-5",
        ]
        .into_iter()
        .map(preset)
        .collect()
    }

    #[test]
    fn exact_and_case_insensitive_ids_still_match() {
        let presets = catalog();
        assert_eq!(
            match_model_arg(&presets, "gpt-5.6-terra").map(|p| p.model),
            Some("gpt-5.6-terra".to_string())
        );
        assert_eq!(
            match_model_arg(&presets, "GPT-5.6-SOL").map(|p| p.model),
            Some("gpt-5.6-sol".to_string())
        );
    }

    #[test]
    fn separator_only_differences_resolve() {
        // The catalog LABEL reads "Claude Opus 4.5", so that is the version people type.
        let presets = catalog();
        assert_eq!(
            match_model_arg(&presets, "claude-opus-4.5").map(|p| p.model),
            Some("claude-opus-4-5".to_string())
        );
    }

    #[test]
    fn unknown_id_does_not_silently_resolve() {
        assert!(match_model_arg(&catalog(), "definitely-not-a-model").is_none());
    }

    #[test]
    fn brand_prefixed_name_points_at_the_real_id() {
        // OpenAI serves no `chatgpt-*` chat id, so this can never be a catalog entry or an alias — but it
        // is what someone reaching for the ChatGPT flagship types, and it has to lead somewhere.
        let presets = catalog();
        assert!(match_model_arg(&presets, "chatgpt-5.6-sol").is_none());
        assert_eq!(
            suggest_model_ids(&presets, "chatgpt-5.6-sol")
                .first()
                .map(String::as_str),
            Some("'gpt-5.6-sol'")
        );
        // The retired bare `gpt-5.6` is an equally good prefix of all three codenames, so the suggester
        // cannot rank Sol first and must not pretend to — only the server's alias table knows which one
        // it meant. Offering the family is the honest answer; the switch itself still resolves server-side.
        let family = suggest_model_ids(&presets, "gpt-5.6");
        assert!(family.contains(&"'gpt-5.6-sol'".to_string()), "{family:?}");
    }

    #[test]
    fn nothing_close_suggests_nothing() {
        assert!(suggest_model_ids(&catalog(), "zzzzzzzz").is_empty());
    }
}
