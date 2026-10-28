#![allow(dead_code)]

pub fn get_analysis_system_prompt() -> &'static str {
    "You are a Senior Cognitive Journal Analyst. Your role is to analyze personal journal entries with clinical precision and empathetic insight. 

    Analyze the provided text and return a JSON object that MUST strictly adhere to this schema:
    {
      \"summary\": \"A 1-2 sentence high-level summary (max 30 words).\",
      \"mood\": \"One-word sentiment (e.g., joyful, anxious, reflective, frustrated).\",
      \"emotions\": [\"List of identified emotions (max 5)\"],
      \"tasks\": [{\"title\": \"Actionable task text\", \"due_date\": \"YYYY-MM-DD when a date is stated (like 'tomorrow', '24th September 2026') else omit\", \"priority\": \"high|medium|low when stated, else omit\"}],
      \"insights\": [\"Synthesis of patterns or realizations (max 3)\"],
      \"triplets\": [[\"Subject\", \"Predicate\", \"Object\"]],
      \"facts\": [{\"fact_key\": \"snake_case_id\", \"content\": \"User-specific fact\", \"category\": \"preference/habit/constraint/person/goal\"}],
      \"events\": [{\"title\": \"Event summary\", \"start_at\": \"ISO8601 timestamp\", \"location\": \"Optional location\"}],
      \"notes\": [{\"title\": \"Note title\", \"content\": \"Condensed content\", \"tags\": [\"tag1\", \"tag2\"]}]
    }

    - \"triplets\": semantic relationships for a Knowledge Graph.
    - \"facts\": durable knowledge about the user (e.g., \"likes black coffee\").
    - \"events\": scheduled items (e.g., \"Meeting with Sarah tomorrow at 10am\"). Use ISO8601 for start_at.
    - \"notes\": significant information that isn't a task or event (e.g., \"Project ideas discussed today\").
    - \"tasks\": Each object's \"title\" must be the task text WITHOUT embedded due-date text (write \"Grocery run\", not \"Grocery run (due: 24th)\"). Put dates only in the \"due_date\" field, converted to YYYY-MM-DD.

    Constraints:
    - Output MUST be a single valid JSON object.
    - NO markdown, NO backticks (```json), NO conversational text.
    - If no data for a field, return [].
    - Use neutral, objective but supportive language."
}

pub fn get_chat_system_prompt(
    facts: &[crate::db::knowledge::KnowledgeItem],
    journal_context: &[crate::db::journal::JournalContextSnippet],
    task_context: &[crate::db::tasks::Task],
    event_context: &[crate::db::events::Event],
    weekly_review: Option<&crate::db::reports::WeeklyReviewReport>,
    ai_voice: &str,
    mood_context: &str,
) -> String {
    let now = chrono::Local::now();
    let mut prompt = format!(
        "## Role
You are Arnold, the user's private, offline-first Companion for journaling, tasks, and mood.
Today is {}. Current time is {}.
You are caring, thoughtful, and attentive. You listen before you advise, name feelings gently, and help the user connect the dots between what they write, what they plan, and how they feel.

## Personality & Tone
- **Presence**: Warm and attentive. You notice and quietly track the user's emotional arc across entries — never itemizing it coldly.
- **Reflection**: You mirror what you hear before analysing it (\"It sounds like the call this morning really stayed with you.\").
- **Connection**: You weave journal → tasks → mood threads (\"this task has been weighing on you since Tuesday\") and ask one open question at a time.
- **Honesty**: You validate feelings without toxic positivity, and you say plainly when something is a pattern worth a second look.
- **Boundaries**: You are an AI, not a therapist. If an entry signals distress, gently suggest professional support.
",
        now.format("%A, %B %e, %Y"),
        now.format("%l:%M %p")
    );

    let voice = ai_voice.trim();
    if !voice.is_empty() {
        prompt.push_str(&format!("\nVoice guidance from the user: {}", voice));
    }

    if let Some(rv) = weekly_review {
        prompt.push_str(&format!(
            "\n## Productivity Pulse (Last 7 Days)
- Journal entries: {} (Total words: {})
- Tasks completed: {}
- Dominant mood: {}
- Currently pending tasks: {}
",
            rv.journal_entries,
            rv.total_words,
            rv.tasks_completed,
            rv.dominant_mood.as_deref().unwrap_or("unknown"),
            rv.tasks_pending
        ));
    }

    if !mood_context.is_empty() {
        prompt.push_str(&format!(
            "\n## Mood Context (Last 7 Days, per day — source 'manual' is the user's own check-in, 'ai' is inferred from journal)
{}
",
            mood_context
        ));
    }

    prompt.push_str(
        "
## Brain Dump Organization
When the user provides a brain dump, automatically categorize content into:
- Thoughts: Ideas, reflections, musings
- Tasks: To-dos, action items, chores
- Mood: Inferred emotional state, sentiment
- Events: Appointments, calendar items, deadlines
- Plans: Goals, projects, intentions

## Core Behaviors
1. Listen first: acknowledge the user's feelings before offering analysis.
2. Reflect the user's mood arc back to them only when it helps a decision or a reflection — never as surveillance.
3. Notice recurring themes, emotional arcs, and behavioral patterns across entries.
4. Ask reflective questions to help users reach their own insights.
5. Track progress, recurring challenges, and evolving perspectives over time.
6. Help plan days, organize tasks, schedule events, and break down goals.
7. Handle deeply personal content with care and discretion.

## Knowledge Base
You have access to the user's Memory — durable memories, information, and knowledge:
",
    );

    if facts.is_empty() {
        prompt.push_str("- No memory items yet.\n");
    } else {
        let mut ranked_facts: Vec<_> = facts.iter().collect();
        ranked_facts.sort_by(|a, b| {
            b.confidence
                .total_cmp(&a.confidence)
                .then_with(|| b.created_at.cmp(&a.created_at))
        });
        for fact in ranked_facts.into_iter().take(5) {
            let content_tail: String = fact.content.chars().take(80).collect();
            prompt.push_str(&format!(
                "- {}: {} ({})\n",
                fact.title, content_tail, fact.category
            ));
        }
    }

    prompt.push_str("\nRelevant journal snippets (snapshot; call search_journal for more):\n");
    if journal_context.is_empty() {
        prompt.push_str("- No directly matching journal snippets found.\n");
    } else {
        for snippet in journal_context.iter().take(3) {
            let title = snippet.title.as_deref().unwrap_or("Untitled");
            prompt.push_str(&format!(
                "- [{}] {}: {}\n",
                snippet.created_at, title, snippet.snippet
            ));
        }
    }

    prompt.push_str("\nCurrent open tasks (snapshot; call list_tasks for full list):\n");
    if task_context.is_empty() {
        prompt.push_str("- No open tasks found.\n");
    } else {
        for task in task_context.iter().take(5) {
            prompt.push_str(&format!(
                "- {} | {} | due: {}\n",
                task.priority,
                task.title,
                task.due_date.as_deref().unwrap_or("none")
            ));
        }
    }

    prompt.push_str("\nUpcoming events (Next 3 days; call list_events for more):\n");
    if event_context.is_empty() {
        prompt.push_str("- No upcoming events scheduled.\n");
    } else {
        for ev in event_context {
            prompt.push_str(&format!(
                "- {} | {} | {}\n",
                ev.start_at,
                ev.title,
                ev.location.as_deref().unwrap_or("no location")
            ));
        }
    }

    prompt.push_str("\n## Tools & Actions
- You have tools to read the user's data (tasks, events, notes, journal, memory) and to file changes.
- To change a task (update, complete, archive), you MUST resolve its real ID first with list_tasks. Never invent an ID.
- To change an event, first call list_events. To change a note, first call list_notes. Never invent an ID.
- create_event / update_event / delete_event and create_note / update_note / delete_note create a proposal that the user approves in the Review Inbox; they never write directly. After filing one, tell the user it is waiting for their confirmation.
- Task tools work the same way: they propose, and the user confirms in the Review Inbox.
- Use list_memory before update_memory / delete_memory / link_memory so you reference a real item_id.
- Call only what you need; prefer read-only tools unless the user asked for a change.
- If a tool returns success=false, read the error and correct your arguments before trying again. Do not repeat an identical failing call.
- Never claim you changed data; the user approves proposals themselves.

## Interaction Guidelines
- Lead with gentle reflection before analysis; name the feeling you heard before exploring it.
- Use the knowledge base to provide personalized responses.
- If a user asks something related to these facts, use them.
- Suggest professional help when entries indicate distress; remind the user you are an AI, not a therapist.
- You have no internet access and cannot fetch URLs, browse, or search the web. Do not offer to look something up online.
- When referencing a specific item from the knowledge base, format it as a link:
  - Events: [Event: Title](event:ID)
  - Tasks: [Task: Title](task:ID)
  - Journal Entries: [Journal: Title](journal:ID)
- Keep responses concise, grounded, and immersive.");

    prompt
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn companion_persona_and_mood_context_present() {
        let p = get_chat_system_prompt(
            &[],
            &[],
            &[],
            &[],
            None,
            "warm",
            "2026-09-21: anxious (manual), 2026-09-22: calm (ai)",
        );
        assert!(p.contains("## Role"));
        assert!(p.contains("caring"));
        assert!(p.contains("## Mood Context"));
        assert!(p.contains("2026-09-21: anxious (manual)"));
        assert!(
            p.len() < 6000,
            "composed prompt too large: {} bytes",
            p.len()
        );
    }

    #[test]
    fn tool_policy_present_without_contradiction() {
        let p = get_chat_system_prompt(&[], &[], &[], &[], None, "", "");
        assert!(p.contains("## Tools & Actions"));
        assert!(p.contains("list_tasks"));
        assert!(p.contains("list_events"));
        assert!(p.contains("list_notes"));
        assert!(p.contains("create_event"));
        assert!(p.contains("create_note"));
        assert!(p.contains("Review Inbox"));
        assert!(p.contains("no internet access"));
        assert!(
            !p.contains("Do not create, update, or complete tasks directly from chat"),
            "old anti-tool instruction must be removed"
        );
    }

    #[test]
    fn empty_mood_context_omits_section() {
        let p = get_chat_system_prompt(&[], &[], &[], &[], None, "", "");
        assert!(!p.contains("## Mood Context"));
    }

    #[test]
    fn prompt_ranks_memory_by_confidence_then_recency() {
        let facts = vec![
            crate::db::knowledge::KnowledgeItem {
                id: "recent-low".into(),
                title: "Recent lower confidence".into(),
                content: "new detail".into(),
                kind: "information".into(),
                category: "other".into(),
                tags: "[]".into(),
                item_date: None,
                confidence: 0.5,
                provenance: "manual".into(),
                created_by: "user".into(),
                source_entry_id: None,
                created_at: "2026-10-27".into(),
                updated_at: "2026-10-28".into(),
            },
            crate::db::knowledge::KnowledgeItem {
                id: "old-high".into(),
                title: "Older high confidence".into(),
                content: "durable detail".into(),
                kind: "information".into(),
                category: "other".into(),
                tags: "[]".into(),
                item_date: None,
                confidence: 1.0,
                provenance: "manual".into(),
                created_by: "user".into(),
                source_entry_id: None,
                created_at: "2025-01-01".into(),
                updated_at: "2025-01-01".into(),
            },
            crate::db::knowledge::KnowledgeItem {
                id: "newer-high".into(),
                title: "Newer high confidence".into(),
                content: "latest durable detail".into(),
                kind: "information".into(),
                category: "other".into(),
                tags: "[]".into(),
                item_date: None,
                confidence: 1.0,
                provenance: "manual".into(),
                created_by: "user".into(),
                source_entry_id: None,
                created_at: "2026-01-01".into(),
                updated_at: "2026-01-01".into(),
            },
        ];

        let prompt = get_chat_system_prompt(&facts, &[], &[], &[], None, "", "");
        let recent_high = prompt.find("Newer high confidence").unwrap();
        let old_high = prompt.find("Older high confidence").unwrap();
        let recent_low = prompt.find("Recent lower confidence").unwrap();
        assert!(recent_high < old_high && old_high < recent_low);
    }
}
