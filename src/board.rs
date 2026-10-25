use crate::sgf::{Color, Point};

/// Einfaches Brett mit Schlagregel, damit geschlagene Steine im Lösungsbild verschwinden.
pub struct Board {
    size: usize,
    cells: Vec<Option<Color>>,
}

impl Board {
    pub fn new(size: usize) -> Self {
        Self {
            size,
            cells: vec![None; size * size],
        }
    }

    fn idx(&self, p: Point) -> usize {
        p.y * self.size + p.x
    }

    pub fn get(&self, p: Point) -> Option<Color> {
        self.cells[self.idx(p)]
    }

    pub fn set(&mut self, p: Point, c: Option<Color>) {
        let i = self.idx(p);
        self.cells[i] = c;
    }

    fn neighbors(&self, p: Point) -> Vec<Point> {
        let mut out = Vec::with_capacity(4);
        if p.x > 0 {
            out.push(Point { x: p.x - 1, y: p.y });
        }
        if p.x + 1 < self.size {
            out.push(Point { x: p.x + 1, y: p.y });
        }
        if p.y > 0 {
            out.push(Point { x: p.x, y: p.y - 1 });
        }
        if p.y + 1 < self.size {
            out.push(Point { x: p.x, y: p.y + 1 });
        }
        out
    }

    /// Gruppe samt Information, ob sie noch mindestens eine Freiheit hat.
    fn group(&self, start: Point) -> (Vec<Point>, bool) {
        let color = self.get(start);
        let mut seen = vec![false; self.cells.len()];
        let mut stack = vec![start];
        let mut group = Vec::new();
        let mut liberties = false;
        seen[self.idx(start)] = true;
        while let Some(p) = stack.pop() {
            group.push(p);
            for n in self.neighbors(p) {
                match self.get(n) {
                    None => liberties = true,
                    c if c == color => {
                        let i = self.idx(n);
                        if !seen[i] {
                            seen[i] = true;
                            stack.push(n);
                        }
                    }
                    _ => {}
                }
            }
        }
        (group, liberties)
    }

    pub fn play(&mut self, color: Color, p: Point) {
        self.set(p, Some(color));
        for n in self.neighbors(p) {
            if self.get(n) == Some(color.opposite()) {
                let (group, liberties) = self.group(n);
                if !liberties {
                    for q in group {
                        self.set(q, None);
                    }
                }
            }
        }
        let (group, liberties) = self.group(p);
        if !liberties {
            for q in group {
                self.set(q, None);
            }
        }
    }

    pub fn stones(&self) -> Vec<(Point, Color)> {
        let mut out = Vec::new();
        for y in 0..self.size {
            for x in 0..self.size {
                let p = Point { x, y };
                if let Some(c) = self.get(p) {
                    out.push((p, c));
                }
            }
        }
        out
    }
}
