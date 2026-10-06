use crate::config::Config;

/// Selected packages plus every non-installed requirement they pull in.
pub fn closure(cfg: &Config, selected: &[bool], installed: &[bool]) -> Vec<bool> {
    let mut effective = selected.to_vec();
    let mut stack: Vec<usize> = (0..selected.len()).filter(|&i| selected[i]).collect();
    while let Some(i) = stack.pop() {
        for &r in &cfg.packages[i].requires {
            if !effective[r] && !installed[r] {
                effective[r] = true;
                stack.push(r);
            }
        }
    }
    effective
}

/// Selected packages that need `target`, directly or transitively.
pub fn required_by(cfg: &Config, selected: &[bool], installed: &[bool], target: usize) -> Vec<usize> {
    (0..selected.len())
        .filter(|&p| p != target && selected[p])
        .filter(|&p| {
            let mut only = vec![false; selected.len()];
            only[p] = true;
            closure(cfg, &only, installed)[target]
        })
        .collect()
}

/// Topological order of the effective set, keeping file order where possible.
pub fn order(cfg: &Config, effective: &[bool]) -> Vec<usize> {
    let mut placed = vec![false; effective.len()];
    let mut result = Vec::new();
    let wanted = effective.iter().filter(|&&e| e).count();
    while result.len() < wanted {
        let next = (0..effective.len())
            .find(|&i| {
                effective[i] && !placed[i] && cfg.packages[i].requires.iter().all(|&r| !effective[r] || placed[r])
            })
            .expect("cycles are rejected when the config is loaded");
        placed[next] = true;
        result.push(next);
    }
    result
}
