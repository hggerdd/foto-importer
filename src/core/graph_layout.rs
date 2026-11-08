//! Kräftebasiertes Layout (Fruchterman-Reingold) für den Personengraphen.
//!
//! Reine, deterministische Funktion ohne Zufall und ohne Uhr: gleiche Eingabe
//! liefert dieselben Koordinaten. Die Einheit ist der Sollabstand zweier
//! verbundener Knoten (1.0). Alle Koordinaten sind endlich.

/// Mindestabstand zweier Knotenmittelpunkte nach dem Layout.
pub const MIN_DISTANCE: f32 = 0.8;

#[derive(Clone, Copy, Debug)]
pub struct LayoutEdge {
    pub a: usize,
    pub b: usize,
    /// Stärke der Anziehung, üblicherweise 1 bis etwa 6.
    pub weight: f32,
}

/// Anzahl der Iterationen, abhängig von der Knotenzahl (O(n²) je Iteration).
fn iterations(count: usize) -> usize {
    (30_000_000 / (count * count).max(1)).clamp(80, 300)
}

/// Startpositionen auf einer Sonnenblumenspirale: deterministisch, ohne Überlagerung.
fn spiral(index: usize) -> [f32; 2] {
    let radius = 0.9 * (index as f32 + 0.5).sqrt();
    let angle = index as f32 * 2.399_963_1;
    [radius * angle.cos(), radius * angle.sin()]
}

/// Ordnet `count` Knoten an. Kanten mit ungültigen Indizes oder Schleifen
/// werden ignoriert. Die Knotenreihenfolge bestimmt die Startposition; wichtige
/// Knoten sollten vorne stehen.
pub fn layout(count: usize, edges: &[LayoutEdge]) -> Vec<[f32; 2]> {
    let mut pos: Vec<[f32; 2]> = (0..count).map(spiral).collect();
    if count < 2 {
        return pos;
    }
    let edges: Vec<LayoutEdge> = edges
        .iter()
        .copied()
        .filter(|e| e.a < count && e.b < count && e.a != e.b && e.weight.is_finite())
        .map(|e| LayoutEdge {
            weight: e.weight.clamp(0.1, 8.0),
            ..e
        })
        .collect();
    let steps = iterations(count);
    let mut temperature = 0.6 * (count as f32).sqrt().max(1.0);
    let cooling = (0.02_f32 / temperature).powf(1.0 / steps as f32);
    let mut force = vec![[0.0_f32; 2]; count];
    for _ in 0..steps {
        force.iter_mut().for_each(|f| *f = [0.0, 0.0]);
        for i in 0..count {
            for j in i + 1..count {
                let (mut dx, mut dy) = (pos[i][0] - pos[j][0], pos[i][1] - pos[j][1]);
                let mut d2 = dx * dx + dy * dy;
                if d2 < 1e-6 {
                    let angle = (i * 31 + j) as f32;
                    (dx, dy, d2) = (angle.cos() * 1e-3, angle.sin() * 1e-3, 1e-6);
                }
                // Abstoßung k²/d, als Vektor (dx,dy)·k²/d².
                let push = 1.0 / d2;
                force[i][0] += dx * push;
                force[i][1] += dy * push;
                force[j][0] -= dx * push;
                force[j][1] -= dy * push;
            }
        }
        for e in &edges {
            let (dx, dy) = (pos[e.a][0] - pos[e.b][0], pos[e.a][1] - pos[e.b][1]);
            let d = (dx * dx + dy * dy).sqrt().max(1e-3);
            // Anziehung d²·Gewicht, als Vektor (dx,dy)·d·Gewicht.
            let pull = d * e.weight;
            force[e.a][0] -= dx * pull;
            force[e.a][1] -= dy * pull;
            force[e.b][0] += dx * pull;
            force[e.b][1] += dy * pull;
        }
        for (p, f) in pos.iter_mut().zip(&force) {
            // Schwache Schwerkraft hält getrennte Gruppen beisammen.
            let (fx, fy) = (f[0] - 0.03 * p[0], f[1] - 0.03 * p[1]);
            let len = (fx * fx + fy * fy).sqrt();
            if len > 1e-9 {
                let step = len.min(temperature);
                p[0] += fx / len * step;
                p[1] += fy / len * step;
            }
        }
        temperature *= cooling;
    }
    separate(&mut pos);
    let n = count as f32;
    let (cx, cy) = pos
        .iter()
        .fold((0.0, 0.0), |a, p| (a.0 + p[0] / n, a.1 + p[1] / n));
    for (index, p) in pos.iter_mut().enumerate() {
        *p = [p[0] - cx, p[1] - cy];
        if !(p[0].is_finite() && p[1].is_finite()) {
            *p = spiral(index);
        }
    }
    pos
}

/// Schiebt zu nahe Knotenpaare auseinander, bis der Mindestabstand eingehalten ist.
fn separate(pos: &mut [[f32; 2]]) {
    for _ in 0..60 {
        let mut moved = false;
        for i in 0..pos.len() {
            for j in i + 1..pos.len() {
                let (dx, dy) = (pos[i][0] - pos[j][0], pos[i][1] - pos[j][1]);
                let d = (dx * dx + dy * dy).sqrt();
                if d >= MIN_DISTANCE {
                    continue;
                }
                moved = true;
                let (ux, uy) = if d > 1e-6 {
                    (dx / d, dy / d)
                } else {
                    let angle = (i * 31 + j) as f32;
                    (angle.cos(), angle.sin())
                };
                let shift = (MIN_DISTANCE - d) / 2.0 + 1e-3;
                pos[i] = [pos[i][0] + ux * shift, pos[i][1] + uy * shift];
                pos[j] = [pos[j][0] - ux * shift, pos[j][1] - uy * shift];
            }
        }
        if !moved {
            break;
        }
    }
}
