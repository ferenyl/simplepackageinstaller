use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use crate::source::Source;

#[derive(Debug)]
pub struct Config {
    pub sections: Vec<Section>,
    pub packages: Vec<Package>,
    pub choices: Vec<ChoiceSet>,
}

#[derive(Debug)]
pub struct Section {
    pub id: String,
    pub name: String,
    pub groups: Vec<Group>,
    pub choice: Option<usize>,
}

#[derive(Debug)]
pub struct Group {
    pub name: String,
    pub packages: Vec<usize>,
    pub choice: Option<usize>,
}

/// Packages of which at most one (exactly one when required) is selected.
#[derive(Debug)]
pub struct ChoiceSet {
    /// Name that requirements can use for whichever alternative is chosen.
    pub base: Option<String>,
    pub packages: Vec<usize>,
    pub required: bool,
}

#[derive(Debug)]
pub struct Package {
    pub name: String,
    pub source: Source,
    pub flags: Vec<String>,
    pub service: Vec<String>,
    pub user_service: Vec<String>,
    pub post: Vec<String>,
    pub requires: Vec<usize>,
    pub requires_choice: Vec<usize>,
    pub choice: Option<usize>,
    pub required: bool,
    pub selected: bool,
    pub always: bool,
    file: Option<String>,
}

impl Package {
    /// Script path as written in the config, relative to it unless absolute; defaults to scripts/<name>.sh.
    pub fn script_file(&self) -> String {
        self.file.clone().unwrap_or_else(|| format!("scripts/{}.sh", self.name))
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawConfig {
    sections: Vec<RawSection>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSection {
    name: String,
    #[serde(default)]
    requires: Strings,
    required: Option<bool>,
    selected: Option<bool>,
    choice: Option<RawChoice>,
    groups: Vec<RawGroup>,
}

#[derive(Deserialize)]
struct RawGroup {
    name: String,
    required: Option<bool>,
    selected: Option<bool>,
    choice: Option<RawChoice>,
    #[serde(flatten)]
    packages: BTreeMap<Source, Vec<RawPackage>>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum RawChoice {
    Flag(bool),
    Base(String),
}

impl RawChoice {
    /// None when not a choice, Some(base) otherwise.
    fn into_choice(self) -> Option<Option<String>> {
        match self {
            RawChoice::Flag(false) => None,
            RawChoice::Flag(true) => Some(None),
            RawChoice::Base(base) => Some(Some(base)),
        }
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum RawPackage {
    Name(String),
    WithOptions(HashMap<String, Option<Options>>),
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields, default)]
struct Options {
    flags: Strings,
    service: Strings,
    user_service: Strings,
    post: Strings,
    requires: Strings,
    required: Option<bool>,
    selected: Option<bool>,
    file: Option<String>,
    always: Option<bool>,
}

#[derive(Default)]
struct Strings(Vec<String>);

impl<'de> Deserialize<'de> for Strings {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum OneOrMany {
            One(String),
            Many(Vec<String>),
        }
        Ok(match OneOrMany::deserialize(d)? {
            OneOrMany::One(s) => Strings(vec![s]),
            OneOrMany::Many(v) => Strings(v),
        })
    }
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()))?;
        let raw: RawConfig =
            serde_yaml_ng::from_str(&text).with_context(|| format!("invalid YAML in {}", path.display()))?;
        Self::from_raw(raw)
    }

    fn from_raw(raw: RawConfig) -> Result<Self> {
        let mut sections = Vec::new();
        let mut packages: Vec<Package> = Vec::new();
        let mut choices: Vec<ChoiceSet> = Vec::new();
        let mut by_name: HashMap<String, usize> = HashMap::new();
        let mut pending_requires: Vec<Vec<String>> = Vec::new();
        let mut configured: Vec<bool> = Vec::new();
        for rs in raw.sections {
            let section_choice = rs.choice.and_then(RawChoice::into_choice);
            let mut groups = Vec::new();
            for rg in rs.groups {
                let group_choice = rg.choice.and_then(RawChoice::into_choice);
                if section_choice.is_some() && group_choice.is_some() {
                    bail!("choice cannot be set on both section '{}' and its group '{}'", rs.name, rg.name);
                }
                let in_choice = section_choice.is_some() || group_choice.is_some();
                let mut group = Group { name: rg.name, packages: Vec::new(), choice: None };
                for (source, list) in rg.packages {
                    for rp in list {
                        let (name, opts) = match rp {
                            RawPackage::Name(n) => (n, None),
                            RawPackage::WithOptions(map) => {
                                if map.len() != 1 {
                                    bail!(
                                        "package entries with options must have exactly one name in group '{}'",
                                        group.name
                                    );
                                }
                                map.into_iter().next().unwrap()
                            }
                        };
                        if let Some(opts) = &opts
                            && (opts.file.is_some() || opts.always.is_some())
                            && source != Source::Script
                        {
                            bail!("'{name}': file and always only apply to script");
                        }
                        // The closest level that sets a flag wins: package, group, section.
                        let required = opts.as_ref().and_then(|o| o.required).or(rg.required).or(rs.required);
                        let selected = opts.as_ref().and_then(|o| o.selected).or(rg.selected).or(rs.selected);
                        let i = match by_name.get(&name) {
                            Some(&i) => {
                                if packages[i].source != source {
                                    bail!("'{name}' is listed with different sources");
                                }
                                i
                            }
                            None => {
                                by_name.insert(name.clone(), packages.len());
                                pending_requires.push(Vec::new());
                                configured.push(false);
                                packages.push(Package {
                                    name: name.clone(),
                                    source,
                                    flags: Vec::new(),
                                    service: Vec::new(),
                                    user_service: Vec::new(),
                                    post: Vec::new(),
                                    requires: Vec::new(),
                                    requires_choice: Vec::new(),
                                    choice: None,
                                    required: false,
                                    selected: false,
                                    always: false,
                                    file: None,
                                });
                                packages.len() - 1
                            }
                        };
                        if let Some(opts) = opts {
                            if std::mem::replace(&mut configured[i], true) {
                                bail!("'{name}': options may only be given on one occurrence");
                            }
                            let pkg = &mut packages[i];
                            pkg.flags = opts.flags.0;
                            pkg.service = opts.service.0;
                            pkg.user_service = opts.user_service.0;
                            pkg.post = opts.post.0;
                            pkg.always = opts.always.unwrap_or(false);
                            pkg.file = opts.file;
                            pending_requires[i].extend(opts.requires.0);
                        }
                        pending_requires[i].extend(rs.requires.0.iter().cloned());
                        // Inside a choice, required applies to the choice, not to each alternative.
                        packages[i].required |= required.unwrap_or(false) && !in_choice;
                        packages[i].selected |= selected.unwrap_or(false);
                        if !group.packages.contains(&i) {
                            group.packages.push(i);
                        }
                    }
                }
                if let Some(base) = group_choice {
                    group.choice = Some(choices.len());
                    choices.push(ChoiceSet {
                        base,
                        packages: group.packages.clone(),
                        required: rg.required.or(rs.required).unwrap_or(false),
                    });
                }
                groups.push(group);
            }
            let mut section = Section { id: slug(&rs.name), name: rs.name, groups, choice: None };
            if let Some(base) = section_choice {
                let mut members: Vec<usize> = Vec::new();
                for &p in section.groups.iter().flat_map(|g| &g.packages) {
                    if !members.contains(&p) {
                        members.push(p);
                    }
                }
                section.choice = Some(choices.len());
                choices.push(ChoiceSet { base, packages: members, required: rs.required.unwrap_or(false) });
            }
            sections.push(section);
        }

        let mut by_base = HashMap::new();
        for (c, choice) in choices.iter().enumerate() {
            if choice.packages.is_empty() {
                bail!("choice '{}' has no packages", choice.base.as_deref().unwrap_or("?"));
            }
            if let Some(base) = &choice.base
                && by_base.insert(base.as_str(), c).is_some()
            {
                bail!("choice '{base}' is defined more than once");
            }
            for &p in &choice.packages {
                if packages[p].choice.replace(c).is_some() {
                    bail!("'{}' is part of more than one choice", packages[p].name);
                }
            }
        }

        for (pi, names) in pending_requires.into_iter().enumerate() {
            let mut reqs = Vec::new();
            let mut req_choices = Vec::new();
            for name in names {
                if let Some(&c) = by_base.get(name.as_str()) {
                    req_choices.push(c);
                } else if let Some(&i) = by_name.get(&name) {
                    reqs.push(i);
                } else if let Some(si) = sections.iter().position(|s| s.id == name) {
                    // A section's alternatives count as one requirement on their choice.
                    for &p in sections[si].groups.iter().flat_map(|g| &g.packages) {
                        match packages[p].choice {
                            Some(c) => req_choices.push(c),
                            None => reqs.push(p),
                        }
                    }
                } else {
                    bail!("'{}' requires '{name}', which does not exist", packages[pi].name);
                }
            }
            if let Some(implicit) = packages[pi].source.implicit_requirement() {
                if let Some(&c) = by_base.get(implicit) {
                    req_choices.push(c);
                } else if let Some(&i) = by_name.get(implicit) {
                    reqs.push(i);
                }
            }
            reqs.retain(|&r| r != pi);
            reqs.sort_unstable();
            reqs.dedup();
            req_choices.retain(|&c| packages[pi].choice != Some(c));
            req_choices.sort_unstable();
            req_choices.dedup();
            packages[pi].requires = reqs;
            packages[pi].requires_choice = req_choices;
        }

        let cfg = Config { sections, packages, choices };
        cfg.check_cycles()?;
        Ok(cfg)
    }

    /// Direct requirements plus every alternative of each required choice.
    pub fn requirements(&self, i: usize) -> impl Iterator<Item = usize> + '_ {
        let pkg = &self.packages[i];
        pkg.requires
            .iter()
            .copied()
            .chain(pkg.requires_choice.iter().flat_map(|&c| self.choices[c].packages.iter().copied()))
    }

    fn check_cycles(&self) -> Result<()> {
        // 0 = unvisited, 1 = in progress, 2 = done
        let mut mark = vec![0u8; self.packages.len()];
        fn visit(cfg: &Config, i: usize, mark: &mut [u8]) -> Result<()> {
            match mark[i] {
                1 => bail!("dependency cycle via '{}'", cfg.packages[i].name),
                2 => return Ok(()),
                _ => {}
            }
            mark[i] = 1;
            for r in cfg.requirements(i) {
                visit(cfg, r, mark)?;
            }
            mark[i] = 2;
            Ok(())
        }
        for i in 0..self.packages.len() {
            visit(self, i, &mut mark)?;
        }
        Ok(())
    }
}

fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.to_lowercase().chars() {
        let c = match c {
            'å' | 'ä' => 'a',
            'ö' => 'o',
            c if c.is_ascii_alphanumeric() => c,
            _ => '-',
        };
        if c != '-' || !out.ends_with('-') {
            out.push(c);
        }
    }
    out.trim_matches('-').to_string()
}
