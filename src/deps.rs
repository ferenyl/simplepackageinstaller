use crate::config::Config;

/// Selected packages plus every non-installed requirement they pull in.
pub fn closure(cfg: &Config, selected: &[bool], installed: &[bool]) -> Vec<bool> {
    let mut effective = selected.to_vec();
    let mut stack: Vec<usize> = (0..selected.len()).filter(|&i| selected[i]).collect();
    while let Some(i) = stack.pop() {
        let pkg = &cfg.packages[i];
        // A required choice is met by any chosen or installed alternative, otherwise by its first.
        let picks: Vec<usize> = pkg
            .requires_choice
            .iter()
            .map(|&c| &cfg.choices[c].packages)
            .filter(|members| !members.iter().any(|&m| effective[m] || installed[m]))
            .map(|members| members[0])
            .collect();
        for r in pkg.requires.iter().copied().chain(picks) {
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

/// A package in install order, with where it runs.
pub struct Step {
    pub pkg: usize,
    /// The package whose config position this step runs at.
    pub root: usize,
    /// Set when the package was moved ahead of its own position for this dependent.
    pub pulled_by: Option<usize>,
}

/// Config order, with each package's requirements installed right before the first package that needs them.
pub fn order(cfg: &Config, effective: &[bool]) -> Vec<Step> {
    fn place(cfg: &Config, effective: &[bool], step: Step, placed: &mut [bool], result: &mut Vec<Step>) {
        if placed[step.pkg] {
            return;
        }
        placed[step.pkg] = true;
        for r in cfg.requirements(step.pkg) {
            if effective[r] {
                let dep = Step { pkg: r, root: step.root, pulled_by: Some(step.pkg) };
                place(cfg, effective, dep, placed, result);
            }
        }
        result.push(step);
    }
    let mut placed = vec![false; effective.len()];
    let mut result = Vec::new();
    for i in (0..effective.len()).filter(|&i| effective[i]) {
        place(cfg, effective, Step { pkg: i, root: i, pulled_by: None }, &mut placed, &mut result);
    }
    result
}
