use itertools::Itertools;

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
    fn recurse(input_name: &str, mods: &[Module]) -> Vec<(Self, String)> {
        let mut ret = vec![];

        if let Some(n) = mods.first() {
            match n.input() {
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
        } else {
            return ret;
        }

        for i in &mods[2..] {
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
