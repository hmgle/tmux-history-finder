use std::collections::{BTreeMap, HashSet};

use super::{
    rendering::{display_grapheme, grapheme_at_char_index, grapheme_width_at, true_position},
    HintPosition, HintTarget, Match, Pane, TabMode,
};

pub(super) fn assign_hints_by_distance(
    panes: &[Pane],
    matches: &[Match],
    cursor_y: usize,
    cursor_x: usize,
    hint_keys: &str,
) -> Vec<HintTarget> {
    let mut sorted = matches.to_vec();
    sorted.sort_by_key(|target| {
        let pane = &panes[target.pane_index];
        let y = pane.start_y + target.line_no;
        let x = pane.start_x + target.visual_col;
        y.abs_diff(cursor_y).pow(2) + x.abs_diff(cursor_x).pow(2)
    });
    let hints = generate_hints(hint_keys, sorted.len());
    hints
        .into_iter()
        .zip(sorted)
        .map(|(hint, target)| HintTarget { hint, target })
        .collect()
}

pub(super) fn generate_hints(keys: &str, needed_count: usize) -> Vec<String> {
    if needed_count == 0 {
        return Vec::new();
    }
    let mut seen = HashSet::new();
    let keys: Vec<char> = keys.chars().filter(|key| seen.insert(*key)).collect();
    let key_count = keys.len();
    if key_count == 0 {
        return Vec::new();
    }
    if key_count == 1 && needed_count > 1 {
        return Vec::new();
    }
    if needed_count <= key_count {
        return keys
            .iter()
            .take(needed_count)
            .map(char::to_string)
            .collect();
    }

    let mut levels = BTreeMap::<usize, Vec<String>>::new();
    levels.insert(1, keys.iter().map(char::to_string).collect());
    let mut leaf_count = key_count;
    while leaf_count < needed_count {
        let shortest = *levels.keys().next().expect("hint levels are non-empty");
        let prefix = levels
            .get_mut(&shortest)
            .and_then(Vec::pop)
            .expect("shortest hint level is non-empty");
        if levels.get(&shortest).is_some_and(Vec::is_empty) {
            levels.remove(&shortest);
        }
        let children = levels.entry(shortest + 1).or_default();
        children.extend(keys.iter().map(|key| {
            let mut hint = prefix.clone();
            hint.push(*key);
            hint
        }));
        leaf_count += key_count - 1;
    }
    let mut hints: Vec<String> = levels.into_values().flatten().collect();
    hints.truncate(needed_count);
    hints
}

pub(super) fn hint_positions(
    panes: &[Pane],
    hint_mapping: &[HintTarget],
    tab_mode: TabMode,
) -> Vec<HintPosition> {
    hint_mapping
        .iter()
        .filter_map(|entry| {
            let target = &entry.target;
            let pane = panes.get(target.pane_index)?;
            let line = pane.lines.get(target.line_no)?;
            if target.visual_col >= pane.width {
                return None;
            }
            let true_col = true_position(line, target.visual_col, tab_mode);
            let (original, _) = grapheme_at_char_index(line, true_col)?;
            let remaining_width = pane.width - target.visual_col;
            let original_width = grapheme_width_at(original, target.visual_col, tab_mode);
            let original = if original_width <= remaining_width {
                display_grapheme(original, target.visual_col, tab_mode)
            } else {
                " ".to_string()
            };
            Some(HintPosition {
                screen_y: pane.start_y + target.line_no,
                screen_x: pane.start_x + target.visual_col,
                original,
                hint: entry.hint.clone(),
            })
        })
        .collect()
}
