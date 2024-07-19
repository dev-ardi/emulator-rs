use itertools::Itertools;
use tracing::instrument;

use crate::playbook::Module;

type Data = ();

#[derive(Debug, Clone)]
pub struct PbTree {
    pub module: Module,
    pub children: Vec<(PbTree, String)>,
}

impl PbTree {
    pub fn new(mods: &[Module]) -> Self {
        let data = mods[0].clone();
        let mods = &mods[1..];
        let name = data.name().to_owned();

        Self {
            module: data,
            children: Self::recurse(&name, mods),
        }
    }
    // TODO: This is feverish code that I wrote sleep deprived at 4AM
    fn recurse(input_name: &str, mods: &[Module]) -> Vec<(Self, String)> {
        let Some(first) = mods.first() else {
            return vec![];
        };

        let mut ret = vec![];
        match first.input() {
            Some(s) => {
                if let Some((a, b)) = s.split_once('.') {
                    if a == input_name {
                        let first = mods[0].clone();
                        let nodes = Self::recurse(first.name(), &mods[1..]);
                        ret.push((
                            Self {
                                module: first,
                                children: nodes,
                            },
                            b.to_owned(),
                        ));
                    }
                }
            }
            None => {
                let first = mods[0].clone();
                let nodes = Self::recurse(first.name(), &mods[1..]);
                ret.push((
                    Self {
                        module: first,
                        children: nodes,
                    },
                    "output".to_owned(),
                ));
            }
        }

        for i in &mods[1..] {
            if let Some((a, b)) = i.name().split_once('.') {
                if a == input_name {
                    let first = mods[0].clone();
                    let nodes = Self::recurse(first.name(), &mods[1..]);
                    ret.push((
                        Self {
                            module: first,
                            children: nodes,
                        },
                        b.to_owned(),
                    ));
                }
            }
        }
        ret
    }
}

fn new(mods: &[Box<Module>]) -> () {
    let things = mods.iter().enumerate().map(|(n, i)| {
        let modname = i.name();

        let mut cur = mods[n + 2..]
            .iter()
            .filter_map(|x| {
                if let Some((a, b)) = x.name().split_once('.') {
                    if a == modname {
                        return Some(b.to_owned());
                    }
                }
                None
            })
            .collect_vec();

        if let Some(n) = mods.get(n + 1) {
            match n.input() {
                Some(s) => {
                    if let Some((a, b)) = s.split_once('.') {
                        if a == modname {
                            cur.push(b.to_owned())
                        }
                    }
                }
                None => {
                    cur.push("output".to_owned());
                }
            }
        }
        cur
    });
}
