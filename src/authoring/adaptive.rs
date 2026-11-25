//! OpenAI model routing, independent of the once/confirm assessment policy.

use super::*;
use crate::config::BackendConfig;

// {impl}`requirements.authoring.adaptive`
pub(super) async fn assess(
    root: &Path,
    subject: &AuthoringSubject,
    config: &ResolvedAuthoringConfig,
    sources: AssessmentSources,
    usage: &mut UsageLog,
    policy: AssessmentPolicy,
) -> io::Result<(Judgment, Value)> {
    let domain = sources.domain.clone();
    let first = route(root, subject, config, sources, usage).await?;
    if policy == AssessmentPolicy::Once || first.0 == Judgment::False {
        return Ok(first);
    }
    let state = usage_state_root(root)?;
    let mut log = followup_log(&state, root, subject, usage, "adaptive confirmation")?;
    usage.record.attempts.push(log.path.clone());
    usage.save()?;
    let result = match subject.domain_sources(domain.clone()) {
        Ok(sources) => route(root, subject, config, sources, &mut log).await,
        Err(error) => Err(error),
    };
    let second = finish_followup(log, "adaptive confirmation", result)?;
    if first.0.kind() == second.0.kind() && first.1["applies"] == second.1["applies"] {
        return Ok(first);
    }
    let describe = |record: &(Judgment, Value)| match record.1["applies"].as_bool() {
        Some(applies) => format!("{}` with applies `{applies}", record.0.kind()),
        None => record.0.kind().to_owned(),
    };
    let first_kind = describe(&first);
    let second_kind = describe(&second);
    let (less, more) = if favorability(&second) < favorability(&first) {
        (second, first)
    } else {
        (first, second)
    };
    let less_kind = describe(&less);
    let mut backend = config.backend.clone();
    let BackendConfig::Openai(openai) = &mut backend else {
        unreachable!()
    };
    openai.model = Some("gpt-6.1-sol".into());
    let provider = RigProvider::connect(&backend).await?;
    let mut log = followup_log(&state, root, subject, usage, "adaptive verification")?;
    log.record.model = Some("gpt-6.1-sol".into());
    usage.record.attempts.push(log.path.clone());
    usage.save()?;
    let result = match followup_sources(&domain, subject, &mut log) {
        Ok(mut sources) => {
            verify(
                &provider,
                &mut sources,
                &mut log,
                &less.0,
                less.1["rationale"].as_str().unwrap_or_default(),
            )
            .await
        }
        Err(error) => Err(error),
    };
    let verification = finish_followup(log, "adaptive verification", result)?;
    let chosen = confirmed_record(
        less,
        more,
        &first_kind,
        &second_kind,
        &less_kind,
        verification,
    );
    usage.record.determination = Some(chosen.0.kind().into());
    usage.save()?;
    Ok(chosen)
}

// {impl}`requirements.authoring.adaptive`
async fn route(
    root: &Path,
    subject: &AuthoringSubject,
    config: &ResolvedAuthoringConfig,
    sources: AssessmentSources,
    usage: &mut UsageLog,
) -> io::Result<(Judgment, Value)> {
    let state = usage_state_root(root)?;
    if sources.has_applicable_fixme {
        usage.record.routing_reason = Some("existing_fixme");
        usage.save()?;
        return attempt(root, subject, config, sources, usage, &state, "gpt-6.1-sol").await?;
    }
    let domain = sources.domain.clone();
    let first = attempt(
        root,
        subject,
        config,
        sources,
        usage,
        &state,
        "gpt-5.4-mini",
    )
    .await?;
    let reason = match &first {
        Ok((Judgment::True, _)) => return first,
        Ok((Judgment::False, _)) => "mini_false",
        Ok((Judgment::Fixme(_), _)) => "mini_fixme",
        // Provider errors, cancellation, and filesystem errors cannot be fixed
        // by selecting a more expensive model. Only conversation/submission
        // failures proceed to Sol; connection/preparation/log errors return
        // through the outer Result from attempt.
        Err(error)
            if error.kind() != io::ErrorKind::Other
                || error.raw_os_error().is_some()
                || error.get_ref().is_some_and(|source| {
                    source.is::<rig_core::error::ProviderError>()
                        || source.is::<serde_json::Error>()
                }) =>
        {
            return first;
        }
        Err(_) => "mini_assessment_failed",
    };
    usage.record.routing_reason = Some(reason);
    usage.save()?;
    let sources = subject.domain_sources(domain.clone())?;
    attempt(root, subject, config, sources, usage, &state, "gpt-6.1-sol").await?
}

// {cite}`habeas:util`
async fn attempt(
    root: &Path,
    subject: &AuthoringSubject,
    config: &ResolvedAuthoringConfig,
    sources: AssessmentSources,
    parent: &mut UsageLog,
    state: &Path,
    model: &str,
) -> io::Result<io::Result<(Judgment, Value)>> {
    let mut backend = config.backend.clone();
    let BackendConfig::Openai(openai) = &mut backend else {
        unreachable!("adaptive is an OpenAI model")
    };
    openai.model = Some(model.to_owned());
    // Both tiers use the same endpoint, credential source and explicit options.
    let provider = RigProvider::connect(&backend).await?;
    let mut log = followup_log(state, root, subject, parent, model)?;
    log.record.model = Some(model.to_owned());
    parent.handoff_first_request(&mut log);
    parent.record.attempts.push(log.path.clone());
    parent.save()?;
    let result = assess_subject(
        &provider,
        root,
        state,
        subject,
        sources,
        &mut log,
        AssessmentPolicy::Once,
    )
    .await;
    log.finish(if result.is_ok() {
        "completed"
    } else {
        "failed"
    })?;
    if let Ok((judgment, _)) = &result {
        parent.record.determination = Some(judgment.kind().into());
        parent.save()?;
    }
    Ok(result)
}
