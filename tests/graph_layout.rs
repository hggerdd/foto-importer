use foto_importer::core::graph_layout::{LayoutEdge, MIN_DISTANCE, layout};
use std::time::{Duration, Instant};

fn distance(a: [f32; 2], b: [f32; 2]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt()
}

fn edge(a: usize, b: usize) -> LayoutEdge {
    LayoutEdge { a, b, weight: 1.0 }
}

#[test]
fn trivial_sizes_are_finite_and_stable() {
    assert!(layout(0, &[]).is_empty());
    let one = layout(1, &[edge(0, 0)]);
    assert_eq!(one.len(), 1);
    assert!(one[0].iter().all(|v| v.is_finite()));
    let two = layout(2, &[edge(0, 1), edge(0, 7)]);
    assert_eq!(two.len(), 2);
    let d = distance(two[0], two[1]);
    assert!((MIN_DISTANCE..3.0).contains(&d), "Abstand {d}");
}

#[test]
fn connected_people_sit_closer_than_unconnected_ones() {
    // Zwei Dreiergruppen, nur innerhalb verbunden.
    let edges = [
        edge(0, 1),
        edge(1, 2),
        edge(0, 2),
        edge(3, 4),
        edge(4, 5),
        edge(3, 5),
    ];
    let pos = layout(6, &edges);
    let inside = (distance(pos[0], pos[1]) + distance(pos[3], pos[4])) / 2.0;
    let between = distance(pos[0], pos[3]).min(distance(pos[2], pos[5]));
    assert!(inside < between, "innen {inside}, zwischen {between}");
}

#[test]
fn layout_is_deterministic_and_has_no_overlaps_for_500_people() {
    let count = 500;
    let mut edges = Vec::new();
    for i in 1..count {
        edges.push(LayoutEdge {
            a: i,
            b: i / 2,
            weight: 1.0 + (i % 5) as f32,
        });
        if i % 3 == 0 {
            edges.push(edge(i, (i * 13 + 5) % count));
        }
    }
    let start = Instant::now();
    let first = layout(count, &edges);
    let elapsed = start.elapsed();
    assert!(
        elapsed < Duration::from_secs(20),
        "Layout dauerte {elapsed:?}"
    );
    assert_eq!(first, layout(count, &edges));
    assert!(first.iter().flatten().all(|v| v.is_finite()));
    let mut closest = f32::MAX;
    for i in 0..count {
        for j in i + 1..count {
            closest = closest.min(distance(first[i], first[j]));
        }
    }
    assert!(
        closest >= MIN_DISTANCE * 0.98,
        "kleinster Abstand {closest}"
    );
}

#[test]
fn invalid_edges_and_isolated_people_do_not_break_the_layout() {
    let edges = [
        edge(0, 1),
        LayoutEdge {
            a: 0,
            b: 2,
            weight: f32::NAN,
        },
        edge(9, 1),
    ];
    let pos = layout(4, &edges);
    assert!(pos.iter().flatten().all(|v| v.is_finite()));
    assert!(distance(pos[3], pos[0]) >= MIN_DISTANCE * 0.98);
}
