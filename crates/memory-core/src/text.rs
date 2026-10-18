//! Lexical normalisation (Spanish/English) and the inverted index that finds seed nodes.
//!
//! The index is derived data: it is not persisted, it is rebuilt when the `.mem` loads.

use std::collections::HashMap;

use crate::model::{MemoryNode, NodeId};

const STOPWORDS: &[&str] = &[
    // Spanish
    "que", "los", "las", "del", "con", "por", "para", "una", "uno", "unos", "unas", "como", "pero", "mas", "este",
    "esta", "esto", "estos", "estas", "ese", "esa", "eso", "esos", "esas", "aqui", "ahi", "alli", "muy", "sin",
    "sobre", "entre", "cuando", "donde", "quien", "cual", "porque", "tambien", "hay", "ser", "estar", "tener",
    "hacer", "fue", "era", "eres", "soy", "son", "estoy", "esta", "estas", "tengo", "tiene", "tienes", "algo",
    "nada", "todo", "toda", "todos", "todas", "mucho", "poco", "ya", "aun", "asi", "bien", "mal", "les", "nos",
    "mis", "tus", "sus", "mio", "tuyo", "suyo", "hola", "vale", "pues", "luego", "entonces", "desde", "hasta",
    // English
    "the", "and", "for", "you", "your", "are", "was", "were", "with", "that", "this", "these", "those", "have",
    "has", "had", "not", "but", "what", "when", "where", "who", "why", "how", "from", "about", "into", "just",
    "there", "their", "they", "them", "then", "than", "will", "would", "could", "should", "can", "did", "does",
    "her", "his", "him", "she", "its", "our", "all", "any", "some", "been", "very", "tell", "remember", "something",
    "anything",
];

/// Lowercase without accents (á→a, ñ→n…).
pub fn fold(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars().flat_map(char::to_lowercase) {
        out.push(match c {
            'á' | 'à' | 'ä' | 'â' | 'ã' => 'a',
            'é' | 'è' | 'ë' | 'ê' => 'e',
            'í' | 'ì' | 'ï' | 'î' => 'i',
            'ó' | 'ò' | 'ö' | 'ô' | 'õ' => 'o',
            'ú' | 'ù' | 'ü' | 'û' => 'u',
            'ñ' => 'n',
            'ç' => 'c',
            other => other,
        });
    }
    out
}

/// Light, symmetric stemming (index and query use the same rule): it drops the plural
/// and the final vowel, so "sueño"/"sueñas", "recuerdo"/"recuerdos", "dream"/"dreams"
/// match. It is not linguistically exact; it only has to be consistent.
fn stem(mut w: String) -> String {
    let len = w.chars().count();
    if len > 5 && w.ends_with("es") {
        w.truncate(w.len() - 2);
    } else if len > 4 && w.ends_with('s') {
        w.truncate(w.len() - 1);
    }
    if w.chars().count() > 4 && (w.ends_with('a') || w.ends_with('o') || w.ends_with('e')) {
        w.truncate(w.len() - 1);
    }
    w
}

/// Words that ask how someone looks (Spanish and English).
const VISUAL_WORDS: &[&str] = &[
    "aspecto", "apariencia", "pinta", "ropa", "vestido", "viste", "ojos", "pelo", "cara", "rostro", "manos", "orejas", "mirada",
    "peinado", "cicatriz", "mascara", "capucha", "bata", "chaqueta", "anillos", "ves", "luce", "guapa", "guapo", "lleva",
    "look", "looks", "appearance", "clothes", "wearing", "wears", "eyes", "hair", "face", "hands", "ears", "scar", "mask",
    "hood", "coat", "jacket", "rings", "outfit", "pretty", "handsome",
];

/// Does the text ask how someone looks? (to seed their appearance nodes)
pub fn visual_intent(text: &str) -> bool {
    let wanted: Vec<String> = VISUAL_WORDS.iter().flat_map(|w| terms(w).collect::<Vec<_>>()).collect();
    terms(text).any(|t| wanted.contains(&t))
}

/// Indexable terms of a text.
pub fn terms(s: &str) -> impl Iterator<Item = String> + '_ {
    s.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(fold)
        .filter(|w| (w.chars().count() >= 3 || w.chars().all(|c| c.is_ascii_digit())) && !STOPWORDS.contains(&w.as_str()))
        .map(stem)
}

/// Weight of a match in the label versus the content.
const LABEL_HIT: f32 = 1.0;
const CONTENT_HIT: f32 = 0.35;

#[derive(Default, Debug, Clone)]
pub struct LexicalIndex {
    /// term → (node, field weight)
    postings: HashMap<String, Vec<(NodeId, f32)>>,
}

impl LexicalIndex {
    fn node_terms(node: &MemoryNode) -> HashMap<String, f32> {
        let mut map: HashMap<String, f32> = HashMap::new();
        for t in terms(&node.content) {
            map.entry(t).or_insert(CONTENT_HIT);
        }
        for t in terms(&node.label) {
            map.insert(t, LABEL_HIT);
        }
        map
    }

    pub fn insert(&mut self, id: NodeId, node: &MemoryNode) {
        for (t, w) in Self::node_terms(node) {
            self.postings.entry(t).or_default().push((id, w));
        }
    }

    pub fn remove(&mut self, id: NodeId, node: &MemoryNode) {
        for t in Self::node_terms(node).into_keys() {
            if let Some(list) = self.postings.get_mut(&t) {
                list.retain(|(n, _)| *n != id);
                if list.is_empty() {
                    self.postings.remove(&t);
                }
            }
        }
    }

    /// Simplified TF-IDF score of every node for the given text, normalised to [0, 1].
    pub fn seeds(&self, query: &str, total_nodes: usize) -> Vec<(NodeId, f32)> {
        let mut scores: HashMap<NodeId, f32> = HashMap::new();
        let n = total_nodes.max(1) as f32;
        let mut seen = std::collections::HashSet::new();
        for t in terms(query) {
            if !seen.insert(t.clone()) {
                continue;
            }
            let Some(list) = self.postings.get(&t) else { continue };
            let idf = (1.0 + n / list.len() as f32).ln();
            for &(id, w) in list {
                *scores.entry(id).or_default() += w * idf;
            }
        }
        let max = scores.values().copied().fold(0.0f32, f32::max);
        if max <= 0.0 {
            return Vec::new();
        }
        let mut out: Vec<_> = scores.into_iter().map(|(id, s)| (id, s / max)).collect();
        out.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folds_and_stems_spanish_and_english() {
        let t: Vec<_> = terms("¡Mis recuerdos con la Tía Marta y las flores!").collect();
        assert_eq!(t, ["recuerd", "tia", "mart", "flor"]);
        let a: Vec<_> = terms("sueñas").collect();
        let b: Vec<_> = terms("sueño").collect();
        assert_eq!(a, b, "verb and noun match");
        let a: Vec<_> = terms("dreams").collect();
        let b: Vec<_> = terms("dream").collect();
        assert_eq!(a, b, "plural and singular match");
        assert!(visual_intent("¿Cómo ves a Kal'tsit? Fíjate en sus ojos"));
        assert!(visual_intent("How does Kal'tsit look today? Look at her eyes"));
        assert!(!visual_intent("¿Qué pasó en Chernóbog?"));
        assert!(!visual_intent("What happened in Chernobog?"));
    }
}
