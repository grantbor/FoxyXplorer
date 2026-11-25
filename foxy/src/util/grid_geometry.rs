//! Prepare GPX grid strokes for rendering in Web Mercator.
//!
//! Many exported grids contain two-point `trkpt` spans that are straight in
//! UTM. Drawing those endpoints as a Web Mercator chord misses the intermediate
//! `wpt` grid intersections by several metres. Insert the original waypoint
//! positions along each span; do not move or modify any source GPX points.

/// Add existing waypoint positions to the drawn grid strokes.
///
/// Both `lines` and `nodes` are in the same Web Mercator coordinate system.
/// This is called only when loading a kit, not on every canvas redraw.
pub(crate) fn align_grid_lines_to_nodes(
    lines: &mut [Vec<(f64, f64)>],
    nodes: &[(f64, f64)],
) {
    // Using a fraction of the typical spacing keeps intersections on their
    // correct lines while excluding nodes from the next parallel grid line.
    let Some(spacing) = typical_node_spacing(nodes) else {
        return;
    };
    let tolerance = (spacing * 0.2).min(200.0);
    let tolerance_sq = tolerance * tolerance;

    for line in lines {
        if line.len() < 2 {
            continue;
        }

        let mut aligned = Vec::with_capacity(line.len());
        aligned.push(line[0]);

        for pair in line.windows(2) {
            let (ax, ay) = pair[0];
            let (bx, by) = pair[1];
            let (dx, dy) = (bx - ax, by - ay);
            let length_sq = dx * dx + dy * dy;
            if length_sq < 1e-12 {
                aligned.push(pair[1]);
                continue;
            }

            let left = ax.min(bx) - tolerance;
            let right = ax.max(bx) + tolerance;
            let bottom = ay.min(by) - tolerance;
            let top = ay.max(by) + tolerance;
            let mut crossings: Vec<(f64, (f64, f64))> = Vec::new();

            for &(nx, ny) in nodes {
                // A cheap bounding-box filter before projection onto the
                // segment keeps loading fast even for large waypoint grids.
                if nx < left || nx > right || ny < bottom || ny > top {
                    continue;
                }
                let t = ((nx - ax) * dx + (ny - ay) * dy) / length_sq;
                if !(1e-6..1.0 - 1e-6).contains(&t) {
                    continue;
                }
                let ex = nx - (ax + t * dx);
                let ey = ny - (ay + t * dy);
                if ex * ex + ey * ey <= tolerance_sq {
                    crossings.push((t, (nx, ny)));
                }
            }

            // Nodes are emitted in the direction of the existing GPX line.
            crossings.sort_by(|a, b| a.0.total_cmp(&b.0));
            for (_, node) in crossings {
                if squared_distance(*aligned.last().unwrap(), node) > 1e-10 {
                    aligned.push(node);
                }
            }
            if squared_distance(*aligned.last().unwrap(), pair[1]) > 1e-10 {
                aligned.push(pair[1]);
            }
        }

        *line = aligned;
    }
}

fn squared_distance(a: (f64, f64), b: (f64, f64)) -> f64 {
    let dx = a.0 - b.0;
    let dy = a.1 - b.1;
    dx * dx + dy * dy
}

fn typical_node_spacing(nodes: &[(f64, f64)]) -> Option<f64> {
    if nodes.len() < 2 {
        return None;
    }

    // Sampling avoids a quadratic pass over all grid nodes. The median also
    // suppresses occasional duplicate/outlier waypoints in exported GPX files.
    let mut nearest = Vec::new();
    for &point in nodes.iter().take(32) {
        let min_sq = nodes.iter()
            .map(|&other| squared_distance(point, other))
            .filter(|&distance| distance > 1e-6 && distance.is_finite())
            .fold(f64::INFINITY, f64::min);
        if min_sq.is_finite() {
            nearest.push(min_sq.sqrt());
        }
    }
    nearest.sort_by(f64::total_cmp);
    nearest.get(nearest.len() / 2).copied()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::geo::merc_of;

    #[test]
    fn known_nodes_bend_a_straight_chord() {
        let nodes = vec![(0.0, 0.0), (1.0, 0.1), (2.0, 0.1), (3.0, 0.0)];
        let mut lines = vec![vec![nodes[0], nodes[3]]];
        align_grid_lines_to_nodes(&mut lines, &nodes);
        assert_eq!(lines[0], nodes);
        // Re-running the preparation does not duplicate intersections.
        align_grid_lines_to_nodes(&mut lines, &nodes);
        assert_eq!(lines[0], nodes);
    }

    #[test]
    fn unrelated_nodes_do_not_distort_a_line() {
        let mut lines = vec![vec![(0.0, 0.0), (3.0, 0.0)]];
        let nodes = vec![(0.0, 1.0), (1.0, 1.0), (2.0, 1.0), (3.0, 1.0)];
        align_grid_lines_to_nodes(&mut lines, &nodes);
        assert_eq!(lines[0], vec![(0.0, 0.0), (3.0, 0.0)]);
    }

    #[test]
    fn umreva_center_node_is_on_the_refined_stroke() {
        // Coordinates from the user-supplied original Umreva GPX. The
        // middle waypoint is ~24.5 m away from the two-endpoint Mercator chord.
        let start = merc_of(83.443039, 55.579592);
        let end = merc_of(83.791609, 55.572146);
        let middle_left = merc_of(83.609422, 55.576162);
        let middle = merc_of(83.625266, 55.575824);
        let mut lines = vec![vec![start, end]];
        let nodes = vec![start, middle_left, middle, end];
        align_grid_lines_to_nodes(&mut lines, &nodes);
        assert!(lines[0].contains(&middle));
        assert_eq!(lines[0].first(), Some(&start));
        assert_eq!(lines[0].last(), Some(&end));
    }
}
