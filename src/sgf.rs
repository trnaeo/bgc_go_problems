use anyhow::{bail, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Color {
    Black,
    White,
}

impl Color {
    pub fn opposite(self) -> Color {
        match self {
            Color::Black => Color::White,
            Color::White => Color::Black,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Point {
    pub x: usize,
    pub y: usize,
}

#[derive(Debug, Clone, Copy)]
pub struct Move {
    pub color: Color,
    /// `None` = Pass
    pub at: Option<Point>,
}

/// Aufbereitetes Go-Problem: Ausgangsstellung und Lösungsfolge.
#[derive(Debug)]
pub struct Problem {
    pub size: usize,
    pub black: Vec<Point>,
    pub white: Vec<Point>,
    /// Erste Lösung (bzw. Hauptvariante, falls keine markiert ist); wird nummeriert dargestellt.
    pub solution: Vec<Move>,
    /// Anzahl der als richtig markierten Lösungen im SGF (0, wenn keine markiert ist)
    pub solution_count: usize,
    /// Stelle, an der die 2. Lösung den Pfad der ersten verlässt
    pub second_solution_start: Option<Branch>,
    /// `false`, wenn keine als richtig markierte Variante gefunden wurde
    /// und stattdessen die Hauptvariante verwendet wird.
    pub solution_marked: bool,
}

#[derive(Debug)]
struct Node {
    props: Vec<(String, Vec<String>)>,
}

impl Node {
    /// Alle Werte der Property; Properties dürfen in einem Knoten mehrfach vorkommen
    /// (z.B. `AB[aa]AB[bb]`).
    fn values(&self, key: &str) -> Vec<&str> {
        self.props
            .iter()
            .filter(|(k, _)| k == key)
            .flat_map(|(_, v)| v.iter().map(String::as_str))
            .collect()
    }

    fn mv(&self, size: usize) -> Option<Move> {
        for (key, color) in [("B", Color::Black), ("W", Color::White)] {
            if let Some(v) = self.values(key).first() {
                return Some(Move {
                    color,
                    at: parse_point(v, size),
                });
            }
        }
        None
    }

    fn is_correct(&self) -> bool {
        let c = self.values("C").join(" ").to_lowercase();
        !c.contains("wrong")
            && !c.contains("incorrect")
            && (c.contains("correct") || c.contains("right"))
    }
}

#[derive(Debug)]
struct Tree {
    nodes: Vec<Node>,
    children: Vec<Tree>,
}

struct Parser {
    chars: Vec<char>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(c) if c.is_whitespace()) {
            self.pos += 1;
        }
    }

    fn parse_tree(&mut self) -> Result<Tree> {
        self.skip_ws();
        if self.peek() != Some('(') {
            bail!("SGF: '(' erwartet an Position {}", self.pos);
        }
        self.pos += 1;
        let mut nodes = Vec::new();
        let mut children = Vec::new();
        loop {
            self.skip_ws();
            match self.peek() {
                Some(';') => {
                    self.pos += 1;
                    nodes.push(self.parse_node()?);
                }
                Some('(') => children.push(self.parse_tree()?),
                Some(')') => {
                    self.pos += 1;
                    break;
                }
                _ => bail!("SGF: unerwartetes Zeichen an Position {}", self.pos),
            }
        }
        Ok(Tree { nodes, children })
    }

    fn parse_node(&mut self) -> Result<Node> {
        let mut props = Vec::new();
        loop {
            self.skip_ws();
            let mut key = String::new();
            while let Some(c) = self.peek() {
                if c.is_ascii_uppercase() {
                    key.push(c);
                    self.pos += 1;
                } else if c.is_ascii_lowercase() {
                    // FF[3] erlaubt Kleinbuchstaben im Property-Namen, sie werden ignoriert
                    self.pos += 1;
                } else {
                    break;
                }
            }
            if key.is_empty() {
                break;
            }
            let mut values = Vec::new();
            loop {
                self.skip_ws();
                if self.peek() == Some('[') {
                    values.push(self.parse_value()?);
                } else {
                    break;
                }
            }
            if values.is_empty() {
                bail!("SGF: Property {key} ohne Wert");
            }
            props.push((key, values));
        }
        Ok(Node { props })
    }

    fn parse_value(&mut self) -> Result<String> {
        self.pos += 1; // '['
        let mut s = String::new();
        loop {
            match self.peek() {
                None => bail!("SGF: nicht abgeschlossener Wert"),
                Some(']') => {
                    self.pos += 1;
                    return Ok(s);
                }
                Some('\\') => {
                    self.pos += 1;
                    if let Some(c) = self.peek() {
                        s.push(c);
                        self.pos += 1;
                    }
                }
                Some(c) => {
                    s.push(c);
                    self.pos += 1;
                }
            }
        }
    }
}

fn parse_point(s: &str, size: usize) -> Option<Point> {
    let mut it = s.chars();
    let (a, b) = (it.next()?, it.next()?);
    if it.next().is_some() || !a.is_ascii_lowercase() || !b.is_ascii_lowercase() {
        return None;
    }
    let (x, y) = ((a as u8 - b'a') as usize, (b as u8 - b'a') as usize);
    (x < size && y < size).then_some(Point { x, y })
}

/// Einzelner Punkt oder komprimiertes Rechteck ("aa:cc").
fn expand_points(v: &str, size: usize) -> Vec<Point> {
    if let Some((a, b)) = v.split_once(':') {
        let (Some(a), Some(b)) = (parse_point(a, size), parse_point(b, size)) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for y in a.y.min(b.y)..=a.y.max(b.y) {
            for x in a.x.min(b.x)..=a.x.max(b.x) {
                out.push(Point { x, y });
            }
        }
        out
    } else {
        parse_point(v, size).into_iter().collect()
    }
}

/// Sammelt alle Varianten (in SGF-Reihenfolge), die mit einem "CORRECT"-Kommentar enden.
fn collect_solutions(tree: &Tree, size: usize, path: &mut Vec<Move>, out: &mut Vec<Vec<Move>>) {
    let len = path.len();
    for node in &tree.nodes {
        if let Some(m) = node.mv(size) {
            path.push(m);
            if node.is_correct() {
                out.push(path.clone());
                path.truncate(len);
                return;
            }
        }
    }
    for child in &tree.children {
        collect_solutions(child, size, path, out);
    }
    path.truncate(len);
}

/// Erster Zug, in dem `other` vom Pfad `first` abweicht.
fn divergence_point(first: &[Move], other: &[Move]) -> Option<Branch> {
    let i = (0..first.len().min(other.len()))
        .find(|&i| first[i].color != other[i].color || first[i].at != other[i].at)?;
    Some(Branch {
        at: other[i].at?,
        // gleiche Zählung wie bei der Nummerierung der Bilder (Pass zählt nicht)
        number: other[..=i].iter().filter(|m| m.at.is_some()).count(),
    })
}

/// Abzweigung einer alternativen Lösung vom ersten Pfad.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Branch {
    /// Punkt des abweichenden Zuges
    pub at: Point,
    /// Zugnummer des abweichenden Zuges
    pub number: usize,
}

/// Folgt jeweils der ersten Variante.
fn main_line(tree: &Tree, size: usize) -> Vec<Move> {
    let mut out = Vec::new();
    let mut cur = tree;
    loop {
        out.extend(cur.nodes.iter().filter_map(|n| n.mv(size)));
        match cur.children.first() {
            Some(c) => cur = c,
            None => return out,
        }
    }
}

/// Fügt `description` als Kommentar `C[...]` in den Wurzelknoten ein.
/// Hat der Wurzelknoten bereits einen Kommentar, bleibt der SGF-Text unverändert.
pub fn with_root_comment(sgf: &str, description: &str) -> Result<String> {
    let mut parser = Parser {
        chars: sgf.chars().collect(),
        pos: 0,
    };
    let tree = parser.parse_tree()?;
    let Some(root) = tree.nodes.first() else {
        bail!("SGF enthält keinen Knoten");
    };
    let description = description.trim();
    if description.is_empty() || !root.values("C").is_empty() {
        return Ok(sgf.to_string());
    }
    let Some(semi) = sgf.find(';') else {
        bail!("SGF enthält keinen Knoten");
    };
    let escaped = description.replace('\\', "\\\\").replace(']', "\\]");
    let (head, tail) = sgf.split_at(semi + 1);
    Ok(format!("{head}C[{escaped}]{tail}"))
}

pub fn parse_problem(text: &str) -> Result<Problem> {
    let mut parser = Parser {
        chars: text.chars().collect(),
        pos: 0,
    };
    let tree = parser.parse_tree()?;
    let Some(root) = tree.nodes.first() else {
        bail!("SGF enthält keinen Knoten");
    };

    let size = match root.values("SZ").first() {
        Some(v) => v
            .split(':')
            .next()
            .and_then(|s| s.trim().parse::<usize>().ok())
            .filter(|s| (2..=25).contains(s))
            .ok_or_else(|| anyhow::anyhow!("SGF: ungültige Brettgrösse '{v}'"))?,
        None => 19,
    };

    let mut black = Vec::new();
    let mut white = Vec::new();
    for node in &tree.nodes {
        if node.mv(size).is_some() {
            break;
        }
        for v in node.values("AB") {
            black.extend(expand_points(v, size));
        }
        for v in node.values("AW") {
            white.extend(expand_points(v, size));
        }
    }

    let mut solutions = Vec::new();
    collect_solutions(&tree, size, &mut Vec::new(), &mut solutions);
    let solution_count = solutions.len();
    let second_solution_start = match solutions.as_slice() {
        [first, second, ..] => divergence_point(first, second),
        _ => None,
    };
    let (solution, solution_marked) = match solutions.into_iter().next() {
        Some(s) => (s, true),
        None => (main_line(&tree, size), false),
    };

    Ok(Problem {
        size,
        black,
        white,
        solution,
        solution_count,
        second_solution_start,
        solution_marked,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "(;GM[1]FF[4]SZ[19]AW[pa][ra][ob][qb][oc][pc][sd]AB[nb][nc][qc][nd][od][qd][re][se](;B[rb];W[rc](;B[sb]C[CORRECT])(;B[rd];W[sb])(;B[na];W[sb])(;B[pd];W[sb]))(;B[rc];W[rb])(;B[sb];W[rb])(;B[na];W[rc])(;B[sc];W[rb]))";

    #[test]
    fn adds_root_comment() {
        let out = with_root_comment(SAMPLE, "Black To Play").unwrap();
        assert!(out.starts_with("(;C[Black To Play]GM[1]"));
        assert!(parse_problem(&out).is_ok());
        // zweiter Aufruf ändert nichts mehr
        assert_eq!(with_root_comment(&out, "x").unwrap(), out);
    }

    #[test]
    fn collects_repeated_setup_properties() {
        let p = parse_problem("(;SZ[19]AB[aa]AB[bb]AW[cc]AB[dd](;B[ee]C[CORRECT]))").unwrap();
        assert_eq!(p.black.len(), 3);
        assert_eq!(p.white.len(), 1);
        assert!(p.solution_marked);
    }

    #[test]
    fn finds_all_solutions_and_divergence() {
        let p = parse_problem(
            "(;SZ[19]AB[aa]AW[bb](;W[od](;B[nd];W[ne];B[oe](;W[ne]C[CORRECT])(;W[pf]))\
             (;B[ne];W[nd]C[CORRECT])(;B[pf];W[nd]C[CORRECT]))(;W[pf];B[ne]))",
        )
        .unwrap();
        assert_eq!(p.solution_count, 3);
        assert_eq!(p.solution.len(), 5);
        assert_eq!(p.solution[0].color, Color::White);
        // 2. Lösung: W od, B ne, W nd -> weicht bei B ne vom ersten Pfad (B nd) ab
        assert_eq!(
            p.second_solution_start,
            Some(Branch {
                at: Point { x: 13, y: 4 },
                number: 2
            })
        );
    }

    #[test]
    fn single_solution_has_no_second_start() {
        let p = parse_problem(SAMPLE).unwrap();
        assert_eq!(p.solution_count, 1);
        assert_eq!(p.second_solution_start, None);
    }

    #[test]
    fn parses_sample() {
        let p = parse_problem(SAMPLE).unwrap();
        assert_eq!(p.size, 19);
        assert_eq!(p.white.len(), 7);
        assert_eq!(p.black.len(), 8);
        assert!(p.solution_marked);
        let pts: Vec<_> = p.solution.iter().map(|m| m.at.unwrap()).collect();
        assert_eq!(
            pts,
            vec![
                Point { x: 17, y: 1 },
                Point { x: 17, y: 2 },
                Point { x: 18, y: 1 }
            ]
        );
    }
}
