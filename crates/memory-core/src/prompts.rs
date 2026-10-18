//! Prompts of the dual pipeline (actor + curator) and the curator's forced output.
//! The texts come from the dictionary (`i18n`) in the requested language.

use i18n::{Lang, Prompt, tr};

use crate::model::{GraphMeta, Moment, NodeId};

/// GBNF grammar for llama-server (the `grammar` field).
pub const CURATOR_GBNF: &str = include_str!("../assets/memory_ops.gbnf");
/// The same contract as a JSON Schema (Ollama `format`, or `response_format` json_schema).
pub const CURATOR_SCHEMA: &str = include_str!("../assets/memory_ops.schema.json");

pub fn curator_system_prompt(lang: Lang, char_name: &str, root: NodeId) -> String {
    i18n::prompt(lang, Prompt::CuratorSystem).replace("{{char}}", char_name).replace("{{root}}", &root.to_string())
}

/// System prompt of the curator of a chat's memory: a neutral chronicler of that conversation.
pub fn chat_curator_system_prompt(lang: Lang, chat_title: &str, root: NodeId) -> String {
    i18n::prompt(lang, Prompt::ChatCuratorSystem).replace("{{chat}}", chat_title).replace("{{root}}", &root.to_string())
}

/// User message for the curator: active memory (with ids) + the turn to evaluate, one
/// `Speaker: text` line per utterance (scenes with several characters work too).
pub fn curator_user_message(lang: Lang, memory_block: &str, lines: &[(String, String)]) -> String {
    let mut s = format!("{}\n{memory_block}{}\n", i18n::t(lang, "curator.active_memory"), i18n::t(lang, "curator.turn"));
    for (who, text) in lines {
        s.push_str(who.trim());
        s.push_str(": ");
        s.push_str(text.trim());
        s.push('\n');
    }
    s.push_str(i18n::t(lang, "curator.output"));
    s
}

/// The actor's system prompt inside a world (possibly with more characters).
/// `context` is `SceneRecall::context` (world + scene + own memory). `user` is the name
/// the user goes by in the scene ("User" if they picked none): without it the model
/// tends to mistake them for someone present. `moment` is the point of the world's
/// timeline the scene is set at: it picks the character's scenario for then.
pub fn scene_actor_prompt(
    lang: Lang,
    character: &GraphMeta,
    world: &GraphMeta,
    context: &str,
    others: &[String],
    user: &str,
    moment: Option<&Moment>,
) -> String {
    let mut s = tr!(lang, "prompt.scene.intro", name = character.name, world = world.name);
    s.push('\n');
    s.push_str(&tr!(lang, "prompt.scene.user", user = user));
    s.push('\n');
    if !others.is_empty() {
        s.push_str(&tr!(lang, "prompt.scene.others", others = others.join(", "), name = character.name));
        s.push('\n');
    }
    if let Some(m) = moment {
        let text = if m.summary.trim().is_empty() { m.name.clone() } else { format!("{}: {}", m.name, m.summary.trim()) };
        s.push_str(&format!("\n## {}\n{}\n", i18n::t(lang, "prompt.scene.moment"), text));
    }
    let situation = character.scenario_at(moment.map(|m| m.id));
    for (title, text) in [("prompt.scene.style", world.scenario.as_str()), ("prompt.scene.situation", situation)] {
        if !text.trim().is_empty() {
            s.push_str(&format!("\n## {}\n{}\n", i18n::t(lang, title), text.trim()));
        }
    }
    s.push('\n');
    s.push_str(i18n::t(lang, "prompt.scene.memory"));
    s.push('\n');
    s.push_str(context);
    s
}

/// The actor's system prompt for a lone character: fixed sheet + the memory block
/// recalled this turn. It replaces the growing history: the actor only sees this and a
/// short window of turns.
pub fn actor_system_prompt(lang: Lang, meta: &GraphMeta, memory_block: &str) -> String {
    let mut s = tr!(lang, "prompt.solo.intro", name = meta.name);
    s.push('\n');
    if !meta.scenario.trim().is_empty() {
        s.push_str(&format!("\n## {}\n{}\n", i18n::t(lang, "prompt.solo.scenario"), meta.scenario.trim()));
    }
    s.push('\n');
    s.push_str(i18n::t(lang, "prompt.solo.memory"));
    s.push('\n');
    s.push_str(memory_block);
    s
}
