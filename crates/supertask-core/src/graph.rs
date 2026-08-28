use std::collections::{HashMap, HashSet, VecDeque};

use crate::error::{Error, ErrorCode, Result};
use crate::spec::SuperTaskFile;

/// Kahn topological order of enabled services that will be started.
/// Disabled services are omitted but still must exist as depends_on targets.
pub fn start_order(file: &SuperTaskFile) -> Result<Vec<String>> {
    detect_cycle(file)?;
    let enabled: HashSet<&str> = file
        .services
        .iter()
        .filter(|(_, s)| s.enabled)
        .map(|(id, _)| id.as_str())
        .collect();

    let mut indeg: HashMap<&str, usize> = HashMap::new();
    let mut adj: HashMap<&str, Vec<&str>> = HashMap::new();
    for id in &enabled {
        indeg.insert(id, 0);
        adj.insert(id, Vec::new());
    }
    for (id, svc) in &file.services {
        if !enabled.contains(id.as_str()) {
            continue;
        }
        for dep in &svc.depends_on {
            if !enabled.contains(dep.as_str()) {
                return Err(Error::new(
                    ErrorCode::DepDead,
                    format!("{id} 依赖已禁用的 {dep}"),
                ));
            }
            adj.get_mut(dep.as_str()).unwrap().push(id.as_str());
            *indeg.get_mut(id.as_str()).unwrap() += 1;
        }
    }

    let mut q: VecDeque<&str> = file
        .services
        .keys()
        .filter(|id| enabled.contains(id.as_str()) && indeg.get(id.as_str()) == Some(&0))
        .map(|s| s.as_str())
        .collect();

    let mut out = Vec::new();
    while let Some(n) = q.pop_front() {
        out.push(n.to_string());
        let mut nxt: Vec<&str> = adj.get(n).cloned().unwrap_or_default();
        nxt.sort_by_key(|id| file.services.get_index_of(*id).unwrap_or(usize::MAX));
        for m in nxt {
            let d = indeg.get_mut(m).unwrap();
            *d -= 1;
            if *d == 0 {
                q.push_back(m);
            }
        }
    }

    if out.len() != enabled.len() {
        return Err(Error::new(ErrorCode::Cycle, "依赖成环"));
    }
    Ok(out)
}

pub fn stop_order(file: &SuperTaskFile) -> Result<Vec<String>> {
    let mut o = start_order(file)?;
    o.reverse();
    Ok(o)
}

pub fn detect_cycle(file: &SuperTaskFile) -> Result<()> {
    #[derive(Clone, Copy)]
    enum Color {
        White,
        Gray,
        Black,
    }
    let mut color: HashMap<&str, Color> = file
        .services
        .keys()
        .map(|k| (k.as_str(), Color::White))
        .collect();
    let mut stack = Vec::new();

    fn dfs<'a>(
        id: &'a str,
        file: &'a SuperTaskFile,
        color: &mut HashMap<&'a str, Color>,
        stack: &mut Vec<&'a str>,
    ) -> Result<()> {
        color.insert(id, Color::Gray);
        stack.push(id);
        if let Some(svc) = file.services.get(id) {
            for dep in &svc.depends_on {
                match color.get(dep.as_str()).copied().unwrap_or(Color::White) {
                    Color::Gray => {
                        let mut cycle: Vec<&str> = stack
                            .iter()
                            .copied()
                            .skip_while(|x| *x != dep.as_str())
                            .collect();
                        cycle.push(dep.as_str());
                        return Err(Error::new(
                            ErrorCode::Cycle,
                            format!("依赖成环：{}", cycle.join(" → ")),
                        ));
                    }
                    Color::White => dfs(dep, file, color, stack)?,
                    Color::Black => {}
                }
            }
        }
        stack.pop();
        color.insert(id, Color::Black);
        Ok(())
    }

    for id in file.services.keys() {
        if matches!(color.get(id.as_str()), Some(Color::White)) {
            dfs(id, file, &mut color, &mut stack)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::parse_yaml;

    fn file(y: &str) -> SuperTaskFile {
        parse_yaml(y).unwrap().0
    }

    #[test]
    fn topo_node_after_api() {
        let f = file(
            r#"
version: 1
services:
  api:
    kind: spring-boot
    module: api
    port: 8080
  web:
    kind: node
    dir: web
    port: 5173
    depends_on: [api]
"#,
        );
        assert_eq!(start_order(&f).unwrap(), vec!["api", "web"]);
        assert_eq!(stop_order(&f).unwrap(), vec!["web", "api"]);
    }

    #[test]
    fn cycle_rejected() {
        let f = file(
            r#"
version: 1
services:
  a:
    kind: node
    dir: a
    port: 1
    depends_on: [b]
  b:
    kind: node
    dir: b
    port: 2
    depends_on: [a]
"#,
        );
        let e = start_order(&f).unwrap_err();
        assert_eq!(e.code(), ErrorCode::Cycle);
    }
}
