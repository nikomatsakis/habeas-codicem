//! OpenAI model routing, independent of the once/confirm assessment policy.

use super::*;
use crate::config::BackendConfig;

pub(super) async fn assess(
    root: &Path,
    subject: &AuthoringSubject,
    config: &ResolvedAuthoringConfig,
    sources: AssessmentSources,
    usage: &mut UsageLog,
    policy: AssessmentPolicy,
) -> io::Result<Assessment> {
    let domain = sources.domain.clone();
    let first = route(root, subject, config, sources, usage).await?;
    if policy == AssessmentPolicy::Once || first.determination == Judgment::False {
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
    let chosen = select_assessment(first, second);
    usage.record.determination = Some(chosen.determination.kind().into());
    usage.save()?;
    Ok(chosen)
}

async fn route(
    root: &Path,
    subject: &AuthoringSubject,
    config: &ResolvedAuthoringConfig,
    sources: AssessmentSources,
    usage: &mut UsageLog,
) -> io::Result<Assessment> {
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
    let reason = match first.as_ref().map(|a| &a.determination) {
        Ok(Judgment::True) => return first,
        Ok(Judgment::False) => "mini_false",
        Ok(Judgment::Fixme(_)) => "mini_fixme",
        // Provider errors, cancellation, and filesystem errors cannot be fixed
        // by selecting a more expensive model. Only conversation/submission
        // failures proceed to Sol; connection/preparation/log errors return
        // through the outer Result from attempt.
        Err(error)
            if error.kind() != io::ErrorKind::Other
                || error.raw_os_error().is_some()
                || error.get_ref().is_some_and(|source| {
                    source.is::<trust_but_verify::graph::ValidationError<io::Error>>()
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
) -> io::Result<io::Result<Assessment>> {
    let mut backend = config.backend.clone();
    let BackendConfig::Openai(openai) = &mut backend else {
        unreachable!("adaptive is an OpenAI model")
    };
    openai.model = Some(model.to_owned());
    // Both tiers use the same endpoint, credential source and explicit options.
    let mut log = followup_log(state, root, subject, parent, model)?;
    log.record.model = Some(model.to_owned());
    parent.handoff_first_conversation(&mut log);
    parent.record.attempts.push(log.path.clone());
    parent.save()?;
    let result = assess_subject(
        &backend,
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
    if let Ok(assessment) = &result {
        parent.record.determination = Some(assessment.determination.kind().into());
        parent.save()?;
    }
    Ok(result)
}
