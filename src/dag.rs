use std::collections::{BTreeMap, BTreeSet, VecDeque};

use anyhow::{Result, bail};

use crate::config::Config;

pub fn validate_config(config: &Config) -> Result<()> {
    for (name, target) in config {
        if target.cmd.is_empty() {
            bail!("target {name:?}: cmd is required");
        }

        let mut strategies = 0;
        if target.wait_for.port.is_some() {
            strategies += 1;
        }
        if target.wait_for.exit_code.is_some() {
            strategies += 1;
        }
        if target.wait_for.log_pattern.is_some() {
            strategies += 1;
        }
        if strategies > 1 {
            bail!("target {name:?}: wait_for must specify at most one strategy");
        }

        for dep in &target.depends {
            if !config.contains_key(dep) {
                bail!("target {name:?}: depends on unknown target {dep:?}");
            }
        }
    }

    let _ = topological_order(config, &config.keys().cloned().collect::<Vec<_>>())?;
    Ok(())
}

pub fn expand_targets(config: &Config, targets: &[String]) -> Result<Vec<String>> {
    let mut needed = BTreeSet::new();
    for target in targets {
        collect_target(config, target, &mut needed)?;
    }
    Ok(needed.into_iter().collect())
}

fn collect_target(config: &Config, target: &str, needed: &mut BTreeSet<String>) -> Result<()> {
    let spec = config
        .get(target)
        .ok_or_else(|| anyhow::anyhow!("unknown target {target:?}"))?;
    if !needed.insert(target.to_string()) {
        return Ok(());
    }
    for dep in &spec.depends {
        collect_target(config, dep, needed)?;
    }
    Ok(())
}

pub fn topological_order(config: &Config, targets: &[String]) -> Result<Vec<String>> {
    let wanted = expand_targets(config, targets)?;
    let wanted_set: BTreeSet<_> = wanted.iter().cloned().collect();
    let mut incoming = BTreeMap::<String, usize>::new();
    let mut outgoing = BTreeMap::<String, Vec<String>>::new();

    for name in &wanted {
        incoming.insert(name.clone(), 0);
        outgoing.insert(name.clone(), Vec::new());
    }

    for name in &wanted {
        let target = &config[name];
        for dep in &target.depends {
            if !wanted_set.contains(dep) {
                continue;
            }
            *incoming.get_mut(name).expect("target must exist") += 1;
            outgoing
                .get_mut(dep)
                .expect("dependency must exist")
                .push(name.clone());
        }
    }

    let mut queue = VecDeque::new();
    for (name, degree) in &incoming {
        if *degree == 0 {
            queue.push_back(name.clone());
        }
    }

    let mut order = Vec::new();
    while let Some(name) = queue.pop_front() {
        order.push(name.clone());
        if let Some(children) = outgoing.get(&name) {
            for child in children {
                let degree = incoming.get_mut(child).expect("child must exist");
                *degree -= 1;
                if *degree == 0 {
                    queue.push_back(child.clone());
                }
            }
        }
    }

    if order.len() != wanted.len() {
        bail!("cycle detected in dependency graph");
    }

    Ok(order)
}
