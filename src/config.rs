use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use crate::source::Source;

#[derive(Debug)]
pub struct Config {
    pub sections: Vec<Section>,
    pub packages: Vec<Package>,
}

#[derive(Debug)]
pub struct Section {
    pub id: String,
    pub name: String,
    pub groups: Vec<Group>,
}

#[derive(Debug)]
pub struct Group {
    pub name: String,
    pub packages: Vec<usize>,
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
    groups: Vec<RawGroup>,
}

#[derive(Deserialize)]
struct RawGroup {
    name: String,
    required: Option<bool>,
    selected: Option<bool>,
    #[serde(flatten)]
    packages: BTreeMap<Source, Vec<RawPackage>>,
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
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("kan inte läsa {}", path.display()))?;
        let raw: RawConfig = serde_yaml_ng::from_str(&text)
            .with_context(|| format!("ogiltig YAML i {}", path.display()))?;
        Self::from_raw(raw)
    }

    fn from_raw(raw: RawConfig) -> Result<Self> {
        let mut sections = Vec::new();
        let mut packages: Vec<Package> = Vec::new();
        let mut by_name = HashMap::new();
        let mut pending_requires: Vec<Vec<String>> = Vec::new();
        for rs in raw.sections {
            let mut groups = Vec::new();
            for rg in rs.groups {
                let mut group = Group { name: rg.name, packages: Vec::new() };
                for (source, list) in rg.packages {
                    for rp in list {
                        let (name, opts) = match rp {
                            RawPackage::Name(n) => (n, Options::default()),
                            RawPackage::WithOptions(map) => {
                                if map.len() != 1 {
                                    bail!("paketposter med tillval måste ha exakt ett namn i gruppen '{}'", group.name);
                                }
                                let (name, opts) = map.into_iter().next().unwrap();
                                (name, opts.unwrap_or_default())
                            }
                        };
                        if (opts.file.is_some() || opts.always.is_some()) && source != Source::Script {
                            bail!("'{name}': file och always gäller bara script");
                        }
                        if by_name.insert(name.clone(), packages.len()).is_some() {
                            bail!("paketet '{name}' finns flera gånger");
                        }
                        let mut requires = opts.requires.0;
                        requires.extend(rs.requires.0.iter().cloned());
                        pending_requires.push(requires);
                        group.packages.push(packages.len());
                        packages.push(Package {
                            name,
                            source,
                            flags: opts.flags.0,
                            service: opts.service.0,
                            user_service: opts.user_service.0,
                            post: opts.post.0,
                            requires: Vec::new(),
                            // The closest level that sets a flag wins: package, group, section.
                            required: opts.required.or(rg.required).or(rs.required).unwrap_or(false),
                            selected: opts.selected.or(rg.selected).or(rs.selected).unwrap_or(false),
                            always: opts.always.unwrap_or(false),
                            file: opts.file,
                        });
                    }
                }
                groups.push(group);
            }
            sections.push(Section { id: slug(&rs.name), name: rs.name, groups });
        }

        for (pi, names) in pending_requires.into_iter().enumerate() {
            let mut reqs = Vec::new();
            for name in names {
                if let Some(&i) = by_name.get(&name) {
                    reqs.push(i);
                } else if let Some(si) = sections.iter().position(|s| s.id == name) {
                    reqs.extend(sections[si].groups.iter().flat_map(|g| g.packages.iter().copied()));
                } else {
                    bail!("'{}' kräver '{name}', som inte finns", packages[pi].name);
                }
            }
            if let Some(implicit) = packages[pi].source.implicit_requirement()
                && let Some(&i) = by_name.get(implicit)
            {
                reqs.push(i);
            }
            reqs.retain(|&r| r != pi);
            reqs.sort_unstable();
            reqs.dedup();
            packages[pi].requires = reqs;
        }

        let cfg = Config { sections, packages };
        cfg.check_cycles()?;
        Ok(cfg)
    }

    fn check_cycles(&self) -> Result<()> {
        // 0 = unvisited, 1 = in progress, 2 = done
        let mut mark = vec![0u8; self.packages.len()];
        fn visit(cfg: &Config, i: usize, mark: &mut [u8]) -> Result<()> {
            match mark[i] {
                1 => bail!("beroendecykel via '{}'", cfg.packages[i].name),
                2 => return Ok(()),
                _ => {}
            }
            mark[i] = 1;
            for &r in &cfg.packages[i].requires {
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
