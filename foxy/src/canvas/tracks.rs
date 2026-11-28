use iced::widget::canvas::{self, Geometry, Path as CanvasPath};
use iced::{Color, Point, Rectangle, Renderer, Theme};
use crate::util::geo::merc_of;
use crate::{App, Message};

pub struct TrackProgram<'a> {
    pub app: &'a App,
}

/// One consistent set of area units for circle and ruler: m² below 0.1 km², km² above.
fn format_area(area_m2: f64) -> String {
    if area_m2 >= 100_000.0 {
        format!("{:.2} {}", area_m2 / 1_000_000.0, crate::t!("msg_0262"))
    } else {
        format!("{:.0} {}", area_m2, crate::t!("msg_0261"))
    }
}

#[cfg(test)]
mod area_format_tests {
    use super::format_area;

    #[test]
    fn area_uses_square_meters_or_square_kilometers_only() {
        assert_eq!(format_area(20_000.0), "20000 м²");
        assert_eq!(format_area(99_999.0), "99999 м²");
        assert_eq!(format_area(100_000.0), "0.10 км²");
        assert_eq!(format_area(332_956.0), "0.33 км²");
        assert_eq!(format_area(1_000_000.0), "1.00 км²");
        assert_eq!(format_area(2_500_000.0), "2.50 км²");
    }
}

// Only the two edges in this segment touch a dragged point. In particular,
// do not connect points across separate track segments.
fn adjacent_point_edges(index: usize, point_count: usize) -> [Option<(usize, usize)>; 2] {
    if index >= point_count {
        return [None, None];
    }
    [
        index.checked_sub(1).map(|prev| (prev, index)),
        (index + 1 < point_count).then_some((index, index + 1)),
    ]
}

/// From -> to follows track order, just like the ruler tool.
/// Declination is the same current value already used for the ruler readout.
fn magnetic_segment_measurement(
    from: (f64, f64), // lon, lat
    to: (f64, f64),   // lon, lat
    declination: f64,
) -> Option<(f64, f64)> { // distance in meters, magnetic azimuth in degrees
    if ![from.0, from.1, to.0, to.1, declination]
        .iter()
        .all(|value| value.is_finite())
    {
        return None;
    }
    let distance = crate::domain::haversine_m(from.1, from.0, to.1, to.0);
    let bearing = crate::util::geo::true_bearing(from.1, from.0, to.1, to.0);
    if !distance.is_finite() || distance < 0.001 || !bearing.is_finite() {
        return None;
    }
    Some((distance, (bearing - declination).rem_euclid(360.0)))
}

#[cfg(test)]
mod selected_point_measurement_tests {
    use super::{adjacent_point_edges, magnetic_segment_measurement};

    #[test]
    fn only_immediate_neighbors_within_one_segment() {
        assert_eq!(adjacent_point_edges(2, 5), [Some((1, 2)), Some((2, 3))]);
        assert_eq!(adjacent_point_edges(0, 5), [None, Some((0, 1))]);
        assert_eq!(adjacent_point_edges(4, 5), [Some((3, 4)), None]);
        assert_eq!(adjacent_point_edges(0, 1), [None, None]);
        assert_eq!(adjacent_point_edges(5, 5), [None, None]);
    }

    #[test]
    fn magnetic_azimuth_follows_track_direction_and_declination() {
        let (distance, azimuth) = magnetic_segment_measurement((0.0, 0.0), (0.01, 0.0), 10.0).unwrap();
        assert!((distance - 1113.2).abs() < 1.0);
        assert!((azimuth - 80.0).abs() < 1e-5);

        let (_, north_azimuth) = magnetic_segment_measurement((0.0, 0.0), (0.0, 0.01), 10.0).unwrap();
        assert!((north_azimuth - 350.0).abs() < 1e-5);
    }

    #[test]
    fn coincident_and_invalid_points_are_not_labeled() {
        assert!(magnetic_segment_measurement((1.0, 2.0), (1.0, 2.0), 0.0).is_none());
        assert!(magnetic_segment_measurement((f64::NAN, 2.0), (1.0, 3.0), 0.0).is_none());
    }
}

// Вспомогательная функция для создания пунктирной линии
/// Паттерн (dash/gap/dash/gap/...) для каждого стиля линии.
/// Пустой слайс = сплошная линия.
fn line_style_pattern(style: &crate::domain::LineStyle) -> &'static [f32] {
    use crate::domain::LineStyle;
    match style {
        LineStyle::Solid      => &[],
        LineStyle::Dash       => &[10.0, 8.0],
        LineStyle::Dot        => &[2.0, 6.0],
        LineStyle::DashDot    => &[10.0, 6.0, 2.0, 6.0],
        LineStyle::DashDotDot => &[10.0, 6.0, 2.0, 6.0, 2.0, 6.0],
    }
}

/// Рисует линию через точки по паттерну чередования [dash, gap, dash, gap, ...].
/// Пустой паттерн = сплошная линия.
fn create_patterned_path(points: &[Point], pattern: &[f32]) -> CanvasPath {
    CanvasPath::new(|p| {
        if pattern.is_empty() {
            let mut first = true;
            for &pt in points {
                if first { p.move_to(pt); first = false; }
                else { p.line_to(pt); }
            }
            return;
        }

        for w in points.windows(2) {
            let p1 = w[0];
            let p2 = w[1];
            let dx = p2.x - p1.x;
            let dy = p2.y - p1.y;
            let dist = (dx * dx + dy * dy).sqrt();
            if dist == 0.0 { continue; }

            let dir_x = dx / dist;
            let dir_y = dy / dist;

            let mut d = 0.0;
            let mut idx = 0;
            // Чётные индексы в паттерне — рисуем, нечётные — пропускаем.
            let mut is_dash = true;

            while d < dist {
                let seg_len = pattern[idx % pattern.len()];
                let d_end = (d + seg_len).min(dist);

                if is_dash {
                    p.move_to(Point::new(p1.x + dir_x * d, p1.y + dir_y * d));
                    p.line_to(Point::new(p1.x + dir_x * d_end, p1.y + dir_y * d_end));
                }

                d = d_end;
                idx += 1;
                is_dash = !is_dash;
            }
        }
    })
}
// 
fn draw_crosshair(frame: &mut canvas::Frame, pt: Point) {
    let circle = CanvasPath::circle(pt, 16.0);
    frame.stroke(&circle, canvas::Stroke::default().with_color(Color::WHITE).with_width(3.0));
    frame.stroke(&circle, canvas::Stroke::default().with_color(Color::from_rgb(1.0, 0.2, 0.2)).with_width(2.0));
    
    let c1 = CanvasPath::line(Point::new(pt.x - 24.0, pt.y), Point::new(pt.x + 24.0, pt.y));
    let c2 = CanvasPath::line(Point::new(pt.x, pt.y - 24.0), Point::new(pt.x, pt.y + 24.0));
    
    let white_cross = canvas::Stroke::default().with_color(Color::WHITE).with_width(3.0);
    frame.stroke(&c1, white_cross);
    frame.stroke(&c2, white_cross);

    let red_cross = canvas::Stroke::default().with_color(Color::from_rgb(1.0, 0.2, 0.2)).with_width(1.5);
    frame.stroke(&c1, red_cross);
    frame.stroke(&c2, red_cross);
}

impl<'a> canvas::Program<Message> for TrackProgram<'a> {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> Vec<Geometry> {
        let app = self.app;
        let geom = app.tracks_cache.draw(renderer, bounds.size(), |frame| {
            let hw_f = bounds.width as f64 / 2.0;
            let hh_f = bounds.height as f64 / 2.0;
            let to_screen = |mx: f64, my: f64| -> Point {
                Point::new(
                    (hw_f + (mx - app.cx) / app.scale) as f32,
                    (hh_f - (my - app.cy) / app.scale) as f32,
                )
            };

            // ==========================================
            // ПРОХОД 1: Рисуем линии треков
            // ==========================================
            for t in &app.tracks {
                let inspected = app.track_inspection.as_ref().is_some_and(|i| i.track_id == t.id);
                if !t.visible && !inspected { continue; }
                // During preview the original is NOT drawn: gaps remain gaps.
                if inspected && app.track_inspection.as_ref().is_some_and(|i| i.preview_segments.is_some()) {
                    continue;
                }
                let color = Color::from_rgb8(t.props.line_color[0], t.props.line_color[1], t.props.line_color[2]);
                let width = t.props.line_width.max(1.0);
                let is_polygon = t.props.poly_type == crate::domain::PolyType::Polygon;
                let line_pattern = line_style_pattern(&t.props.line_style);

                for seg in &t.segments {
                    if seg.points.len() < 2 { continue; }

                    let screen_pts: Vec<Point> = seg.points.iter().map(|pt| {
                        let (mx, my) = merc_of(pt.lon, pt.lat);
                        to_screen(mx, my)
                    }).collect();

                    // Заливка для полигона
                    if is_polygon && t.props.fill_type != crate::domain::FillType::None {
                        let path = CanvasPath::new(|p| {
                            let mut first = true;
                            for &pt in &screen_pts {
                                if first { p.move_to(pt); first = false; }
                                else { p.line_to(pt); }
                            }
                            p.close();
                        });
                        let alpha = if t.props.fill_type == crate::domain::FillType::Solid { 1.0 } else { 0.3 };
                        let fill_c = Color::from_rgba8(t.props.fill_color[0], t.props.fill_color[1], t.props.fill_color[2], alpha);
                        frame.fill(&path, fill_c);
                    }

                    // Путь для линии
                    let mut stroke_pts = screen_pts.clone();
                    if is_polygon && stroke_pts.len() >= 2 {
                        stroke_pts.push(stroke_pts[0]);
                    }

                    let path = if !line_pattern.is_empty() {
                        create_patterned_path(&stroke_pts, line_pattern)
                    } else {
                        CanvasPath::new(|p| {
                            let mut first = true;
                            for &pt in &stroke_pts {
                                if first { p.move_to(pt); first = false; }
                                else { p.line_to(pt); }
                            }
                        })
                    };

                    // Обводка выделенного трека
                    if app.selection.tracks.contains(&t.id) {
                        frame.stroke(&path, canvas::Stroke::default().with_color(Color::WHITE).with_width(width + 2.0));
                    }
                    // Сам трек
                    frame.stroke(&path, canvas::Stroke::default().with_color(color).with_width(width));
                }
            }

            // ------------------------------------------
            // GPX diagnostic overlay: red intervals are suspected, not proven.
            // The preview draws ONLY retained segments, never a bridge across a gap.
            // ------------------------------------------
            if let Some(inspect) = &app.track_inspection {
                if let Some(track) = app.tracks.iter().find(|t| t.id == inspect.track_id) {
                    if let Some(preview) = &inspect.preview_segments {
                        let color = Color::from_rgb8(0x4a, 0x9e, 0xbb);
                        for segment in preview {
                            let pts: Vec<_> = segment.points.iter().map(|p| {
                                let (mx, my) = merc_of(p.lon, p.lat);
                                to_screen(mx, my)
                            }).collect();
                            if pts.len() >= 2 {
                                let path = create_patterned_path(&pts, &[]);
                                frame.stroke(&path, canvas::Stroke::default().with_color(color).with_width(3.0));
                            }
                            if let Some(&a) = pts.first() {
                                frame.fill(&CanvasPath::circle(a, 4.0), color);
                            }
                            if let Some(&z) = pts.last() {
                                frame.fill(&CanvasPath::circle(z, 4.0), color);
                            }
                        }
                    } else {
                        // Highlight suspicious position jumps, including those
                        // where there is no defensible interval to propose cutting.
                        // Mark endpoints only: no misleading connection is drawn.
                        for (jump_index, jump) in inspect.report.severe_jumps.iter().enumerate() {
                            let Some(segment) = track.segments.get(jump.segment) else { continue };
                            if jump.to_index == 0 || jump.to_index >= segment.points.len() { continue; }
                            let jump_color = if inspect.focused_jump == Some(jump_index) {
                                Color::from_rgb8(0xff, 0x60, 0x20)
                            } else { Color::from_rgb8(0xf0, 0xb3, 0x37) };
                            for point in [&segment.points[jump.to_index - 1], &segment.points[jump.to_index]] {
                                let (mx, my) = merc_of(point.lon, point.lat);
                                let p = to_screen(mx, my);
                                frame.stroke(&CanvasPath::circle(p, 7.0),
                                    canvas::Stroke::default().with_color(jump_color).with_width(2.0));
                            }
                        }
                        // Review-only quality episodes are outlined, not crossed
                        // out: weak positioning alone is not a delete instruction.
                        for (i, ep) in inspect.report.quality.episodes.iter().enumerate() {
                            if ep.verdict != crate::domain::quality::Verdict::Review { continue; }
                            let Some(segment)=track.segments.get(ep.segment) else { continue; };
                            if ep.start>ep.end || ep.end>=segment.points.len() { continue; }
                            let points:Vec<_>=segment.points[ep.start..=ep.end].iter().map(|p| {
                                let (mx,my)=merc_of(p.lon,p.lat);
                                to_screen(mx,my)
                            }).collect();
                            let color=if inspect.focused_quality == Some(i) {
                                Color::from_rgb8(0xff,0xbf,0x35)
                            } else { Color::from_rgb8(0xdf,0x9b,0x44) };
                            if points.len()>=2 {
                                frame.stroke(&create_patterned_path(&points,&[]),
                                    canvas::Stroke::default().with_color(color).with_width(2.0));
                            }
                        }
                        for (index, region) in inspect.report.ranges.iter().enumerate() {
                            let Some(segment) = track.segments.get(region.segment) else { continue };
                            let points: Vec<_> = segment.points[region.start..=region.end]
                                .iter().map(|p| {
                                    let (mx, my) = merc_of(p.lon, p.lat);
                                    to_screen(mx, my)
                                }).collect();
                            let color = if inspect.focused == Some(index) {
                                Color::from_rgb8(0xff, 0xa0, 0x30)
                            } else { Color::from_rgb8(0xf0, 0x48, 0x48) };
                            if points.len() >= 2 {
                                let path = create_patterned_path(&points, &[]);
                                frame.stroke(&path, canvas::Stroke::default().with_color(Color::WHITE).with_width(6.0));
                                frame.stroke(&path, canvas::Stroke::default().with_color(color).with_width(4.0));
                            }
                            for p in [points.first(), points.last()].into_iter().flatten() {
                                frame.fill(&CanvasPath::circle(*p, 5.0), color);
                            }
                        }
                    }
                    // A second-pass finding refers to the first preview's
                    // segments, NOT to source indexes or to the final output.
                    // Outline the focused candidate even after it was checked
                    // for removal, without adding a connecting segment.
                    if let (Some((is_review, index)), Some(second), Some(first)) = (
                        inspect.second_focused, &inspect.second_report, &inspect.first_pass_segments,
                    ) {
                        let area = if is_review {
                            second.quality.episodes.get(index).and_then(|ep| {
                                (ep.verdict == crate::domain::quality::Verdict::Review)
                                    .then_some((ep.segment, ep.start, ep.end))
                            })
                        } else {
                            second.ranges.get(index)
                                .map(|r| (r.segment, r.start, r.end))
                        };
                        if let Some((segment, start, end)) = area {
                            if let Some(points) = first.get(segment).map(|s| &s.points) {
                                if start <= end && end < points.len() {
                                    let pts: Vec<_> = points[start..=end].iter().map(|pt| {
                                        let (mx, my) = merc_of(pt.lon, pt.lat);
                                        to_screen(mx, my)
                                    }).collect();
                                    let color = Color::from_rgb8(0xff, 0xb0, 0x38);
                                    if pts.len() >= 2 {
                                        frame.stroke(&create_patterned_path(&pts, &[]),
                                            canvas::Stroke::default().with_color(color).with_width(4.0));
                                    }
                                    for pt in [pts.first(), pts.last()].into_iter().flatten() {
                                        frame.fill(&CanvasPath::circle(*pt, 6.0), color);
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // ==========================================
            // ПРОХОД 2: Рисуем точки треков (ромбы)
            // ==========================================
            for t in &app.tracks {
                if !t.visible { continue; }
                let color = Color::from_rgb8(t.props.line_color[0], t.props.line_color[1], t.props.line_color[2]);
                
                // Проверяем: открыта ли таблица TrackList именно для этого трека
                let tracklist_state: Option<&crate::TrackListState> = match &app.modal {
                    Some(crate::Modal::TrackList(state)) if state.track_id == t.id => Some(state),
                    _ => None,
                };
                
                // Рисуем, если активен SelectPoints И трек выделен, ИЛИ открыта таблица этого трека
                let is_editing_tool = app.active_tool == crate::Tool::SelectPoints;
                let draw_points = (app.selection.tracks.contains(&t.id) && is_editing_tool) || tracklist_state.is_some();

                if draw_points {
                    for (seg_idx, seg) in t.segments.iter().enumerate() {
                        for (pt_idx, pt) in seg.points.iter().enumerate() {
                            let (mx, my) = merc_of(pt.lon, pt.lat);
                            let sp = to_screen(mx, my);

                            if sp.x < -10.0 || sp.x > bounds.width + 10.0 || sp.y < -10.0 || sp.y > bounds.height + 10.0 { 
                                continue; 
                            }

                            // Проверяем выделение
                            let is_pt_selected = if let Some(state) = tracklist_state {
                                // Если таблица открыта — смотрим её выделение (sect = seg_idx + 1)
                                state.selected_points.contains(&(seg_idx + 1, pt_idx))
                            } else {
                                // Иначе — обычное выделение на карте
                                app.selection.points.contains(&(t.id, seg_idx, pt_idx))
                            };

                            let fill_color = if is_pt_selected { 
                                Color::from_rgb8(0x2a, 0x7f, 0xff) 
                            } else { 
                                Color::WHITE 
                            };

                            let r = 8.0; 
                            let diamond = CanvasPath::new(|p| {
                                p.move_to(Point::new(sp.x, sp.y - r));
                                p.line_to(Point::new(sp.x + r, sp.y));
                                p.line_to(Point::new(sp.x, sp.y + r));
                                p.line_to(Point::new(sp.x - r, sp.y));
                                p.close();
                            });

                            frame.fill(&diamond, fill_color);
                            frame.stroke(&diamond, canvas::Stroke::default().with_color(color).with_width(2.0));
                        }
                    }
                }
            }

            // ==========================================
            // ИЗМЕРЕНИЯ ПРИ ПЕРЕТАСКИВАНИИ ОДНОЙ ТОЧКИ
            // ==========================================
            // A temporary overlay: no new track data or selection state.
            if app.active_tool == crate::Tool::SelectPoints
                && app.moving_points
                && app.selection.points.len() == 1
            {
                if let Some(&(track_id, seg_idx, pt_idx)) = app.selection.points.iter().next() {
                    if let Some(track) = app.tracks.iter().find(|t| t.id == track_id && t.visible) {
                        if let Some(segment) = track.segments.get(seg_idx) {
                            for (from_idx, to_idx) in adjacent_point_edges(pt_idx, segment.points.len())
                                .into_iter().flatten()
                            {
                                let from = &segment.points[from_idx];
                                let to = &segment.points[to_idx];
                                let Some((distance, azimuth)) = magnetic_segment_measurement(
                                    (from.lon, from.lat), (to.lon, to.lat), app.declination,
                                ) else { continue };

                                let (mx1, my1) = merc_of(from.lon, from.lat);
                                let (mx2, my2) = merc_of(to.lon, to.lat);
                                let a = to_screen(mx1, my1);
                                let b = to_screen(mx2, my2);
                                let dx = b.x - a.x;
                                let dy = b.y - a.y;
                                let len = dx.hypot(dy);
                                if !len.is_finite() || len < 1.0 { continue; }
                                // Avoid labels for edges wholly outside the viewport.
                                if a.x.max(b.x) < 0.0 || a.x.min(b.x) > bounds.width
                                    || a.y.max(b.y) < 0.0 || a.y.min(b.y) > bounds.height
                                {
                                    continue;
                                }

                                let text = if distance >= 1000.0 {
                                    format!("{:.2} {} · {:.0}°M", distance / 1000.0, crate::t!("msg_0259"), azimuth)
                                } else {
                                    format!("{:.0} {} · {:.0}°M", distance, crate::t!("msg_0258"), azimuth)
                                };

                                // Highlight the measured edge so that the labels
                                // remain unambiguous on busy maps.
                                let line = CanvasPath::line(a, b);
                                let accent = Color::from_rgb8(0x2a, 0x7f, 0xff);
                                frame.stroke(&line, canvas::Stroke::default().with_color(Color::WHITE).with_width(4.0));
                                frame.stroke(&line, canvas::Stroke::default().with_color(accent).with_width(2.5));

                                let box_w = text.chars().count() as f32 * 7.4 + 16.0;
                                let box_h = 23.0;
                                // Place the label just aside from the edge, not
                                // on top of the dragged point or its diamond.
                                let label_x = (a.x + b.x) / 2.0 - dy / len * 20.0;
                                let label_y = (a.y + b.y) / 2.0 + dx / len * 20.0;
                                let label_x = if bounds.width >= box_w + 8.0 {
                                    label_x.clamp(box_w / 2.0 + 4.0, bounds.width - box_w / 2.0 - 4.0)
                                } else { bounds.width / 2.0 };
                                let label_y = if bounds.height >= box_h + 8.0 {
                                    label_y.clamp(box_h / 2.0 + 4.0, bounds.height - box_h / 2.0 - 4.0)
                                } else { bounds.height / 2.0 };
                                let position = Point::new(label_x - box_w / 2.0, label_y - box_h / 2.0);
                                let size = iced::Size::new(box_w, box_h);
                                frame.fill_rectangle(position, size, Color::from_rgba8(0, 0, 0, 0.88));
                                frame.stroke(
                                    &CanvasPath::rectangle(position, size),
                                    canvas::Stroke::default().with_color(accent).with_width(1.5),
                                );
                                frame.fill_text(canvas::Text {
                                    content: text,
                                    position: Point::new(label_x, label_y),
                                    color: Color::WHITE,
                                    size: iced::Pixels(14.0),
                                    align_x: iced::widget::text::Alignment::Center,
                                    align_y: iced::alignment::Vertical::Center,
                                    ..Default::default()
                                });
                            }
                        }
                    }
                }
            }

            // ==========================================
            // РАМКА ВЫДЕЛЕНИЯ
            // ==========================================
            if let (Some(start), Some(end)) = (app.rubber_band_start, app.rubber_band_end) {
                let rect = Rectangle {
                    x: start.x.min(end.x),
                    y: start.y.min(end.y),
                    width: (end.x - start.x).abs(),
                    height: (end.y - start.y).abs(),
                };
                frame.fill_rectangle(rect.position(), rect.size(), Color::from_rgba8(0x2a, 0x7f, 0xff, 0.15));
                let rect_path = CanvasPath::rectangle(rect.position(), rect.size());
                frame.stroke(&rect_path, canvas::Stroke::default().with_color(Color::from_rgb8(0x2a, 0x7f, 0xff)).with_width(1.0));
            }          

            // ==========================================
            // WAYPOINTS
            // ==========================================
            for wp in &app.waypoints {
                if !wp.visible { continue; }
                let (mx, my) = merc_of(wp.lon, wp.lat);
                let pt = to_screen(mx, my);
                
                super::waypoint_label::draw_waypoint_label(
                    frame, pt, &wp.name, wp.bg_color, wp.text_color,
                    app.selection.waypoints.contains(&wp.id),
                );
            }

            // ==========================================
            // ПРИЦЕЛ WAYPOINT (ИЗ ТАБЛИЦЫ/КНОПКИ)
            // ==========================================
            if let Some((lon, lat)) = app.waypoint_crosshair {
                let (mx, my) = merc_of(lon, lat);
                draw_crosshair(frame, to_screen(mx, my));
            }

            // ==========================================
            // EDITABLE CIRCLE: only a preview until Enter commits a polygon.
            // ==========================================
            if let Some(draft) = &app.circle_draft {
                let ring = draft.ring();
                if ring.len() > 3 {
                    let pts: Vec<Point> = ring.iter().map(|&(lon, lat)| {
                        let (mx, my) = merc_of(lon, lat);
                        to_screen(mx, my)
                    }).collect();
                    let path = CanvasPath::new(|p| {
                        p.move_to(pts[0]);
                        for &pt in pts.iter().skip(1) { p.line_to(pt); }
                        p.close();
                    });
                    let color = Color::from_rgb8(0x2a, 0x7f, 0xff);
                    frame.fill(&path, Color::from_rgba8(42, 127, 255, 0.12));
                    frame.stroke(&path, canvas::Stroke::default().with_color(color).with_width(2.2));
                    let (mx, my) = merc_of(draft.center_lon, draft.center_lat);
                    let center = to_screen(mx, my);
                    frame.fill(&CanvasPath::circle(center, 4.0), Color::WHITE);
                    frame.stroke(&CanvasPath::circle(center, 4.0),
                        canvas::Stroke::default().with_color(color).with_width(1.5));

                    let radius_text = if draft.radius_m >= 1000.0 {
                        format!("{:.2} {}", draft.radius_m / 1000.0, crate::t!("msg_0259"))
                    } else {
                        format!("{:.0} {}", draft.radius_m, crate::t!("msg_0258"))
                    };
                    let area_text = format_area(draft.area_m2());
                    let label = format!("{}    {}",
                        crate::tf!("msg_0630", value = radius_text),
                        crate::tf!("msg_0631", value = area_text));
                    let width = (label.chars().count() as f32 * 7.3 + 18.0)
                        .min((bounds.width - 10.0).max(0.0));
                    let left = (center.x - width / 2.0)
                        .clamp(5.0, (bounds.width - width - 5.0).max(5.0));
                    let top = (center.y - 35.0)
                        .clamp(5.0, (bounds.height - 26.0).max(5.0));
                    frame.fill_rectangle(Point::new(left, top), iced::Size::new(width, 25.0),
                        Color::from_rgba8(0, 0, 0, 0.82));
                    frame.fill_text(canvas::Text {
                        content: label,
                        position: Point::new(left + width / 2.0, top + 12.5),
                        color: Color::WHITE,
                        size: iced::Pixels(13.0),
                        align_x: iced::widget::text::Alignment::Center,
                        align_y: iced::alignment::Vertical::Center,
                        ..Default::default()
                    });

                    // Once the radius is locked, show where the edge can be grabbed.
                    // The dot is snapped to the outline, not placed at the raw cursor.
                    if draft.fixed && !draft.dragging_edge && app.map_cursor_inside {
                        if let Some(cursor) = app.cursor {
                            if let Some(edge) = draft.edge_hit(cursor, bounds.size(), app.cx, app.cy, app.scale) {
                                let dot = CanvasPath::circle(edge, 5.0);
                                frame.fill(&dot, Color::WHITE);
                                frame.stroke(&dot, canvas::Stroke::default().with_color(color).with_width(2.0));

                                let hint = crate::t!("msg_0634");
                                let hint_w = (hint.chars().count() as f32 * 7.3 + 16.0)
                                    .min((bounds.width - 10.0).max(0.0));
                                let hint_x = (edge.x + 14.0)
                                    .clamp(5.0, (bounds.width - hint_w - 5.0).max(5.0));
                                let hint_y = (if edge.y + 33.0 > bounds.height {
                                    edge.y - 31.0
                                } else {
                                    edge.y + 10.0
                                }).clamp(5.0, (bounds.height - 24.0).max(5.0));
                                frame.fill_rectangle(Point::new(hint_x, hint_y),
                                    iced::Size::new(hint_w, 24.0), Color::from_rgba8(0, 0, 0, 0.85));
                                frame.fill_text(canvas::Text {
                                    content: hint.to_string(),
                                    position: Point::new(hint_x + hint_w / 2.0, hint_y + 12.0),
                                    color: Color::WHITE,
                                    size: iced::Pixels(13.0),
                                    align_x: iced::widget::text::Alignment::Center,
                                    align_y: iced::alignment::Vertical::Center,
                                    ..Default::default()
                                });
                            }
                        }
                    }
                }
            }

            // ==========================================
            // РИСУЕМЫЙ ТРЕК (DrawTrack / Freehand)
            // ==========================================
            if let Some(dt) = &app.drawing_track {
                if !dt.points.is_empty() {
                    let color = Color::from_rgb8(0x2a, 0x7f, 0xff);
                    let is_freehand = app.freehand_is_drawing;
                    
                    let screen_pts: Vec<Point> = dt.points.iter().map(|&(lon, lat)| {
                        let (mx, my) = merc_of(lon, lat);
                        to_screen(mx, my)
                    }).collect();
                    
                    if screen_pts.len() >= 2 {
                        let path = CanvasPath::new(|p| {
                            let mut first = true;
                            for pt in &screen_pts {
                                if first { p.move_to(*pt); first = false; }
                                else { p.line_to(*pt); }
                            }
                        });
                        frame.stroke(&path, canvas::Stroke::default().with_color(color).with_width(2.0));
                    }
                    
                    if !is_freehand {
                        if let Some(cursor) = app.cursor {
                            let last = *screen_pts.last().unwrap();
                            let line = CanvasPath::line(last, cursor);
                            frame.stroke(&line, canvas::Stroke::default().with_color(color).with_width(1.5));
                        }
                        for pt in &screen_pts {
                            let circle = CanvasPath::circle(*pt, 5.0);
                            frame.fill(&circle, Color::WHITE);
                            frame.stroke(&circle, canvas::Stroke::default().with_color(color).with_width(1.5));
                        }

                        // ---------- Подписи к сегментам (как в Ruler) ----------
                        let pts = &dt.points;
                        if pts.len() >= 2 {
                            for i in 0..pts.len() - 1 {
                                let (lon1, lat1) = pts[i];
                                let (lon2, lat2) = pts[i + 1];
                                let sp1 = screen_pts[i];
                                let sp2 = screen_pts[i + 1];

                                let dx_px = sp2.x - sp1.x;
                                let dy_px = sp2.y - sp1.y;
                                let screen_dist_px = (dx_px * dx_px + dy_px * dy_px).sqrt();

                                if screen_dist_px > 40.0 {
                                    let dist_m = crate::domain::haversine_m(lat1, lon1, lat2, lon2);
                                    let true_az = crate::util::geo::true_bearing(lat1, lon1, lat2, lon2);
                                    let mut mag_az = true_az - app.declination;
                                    if mag_az < 0.0 { mag_az += 360.0; }
                                    if mag_az >= 360.0 { mag_az -= 360.0; }

                                    let text = if dist_m >= 1000.0 {
                                        format!("{:.2} {}, {:.0}°", dist_m / 1000.0, crate::t!("msg_0259"), mag_az)
                                    } else {
                                        format!("{:.0} {}, {:.0}°", dist_m, crate::t!("msg_0258"), mag_az)
                                    };

                                    let mid_x = (sp1.x + sp2.x) / 2.0;
                                    let mid_y = (sp1.y + sp2.y) / 2.0;

                                    let font_size = 14.0;
                                    let box_h = 21.0;
                                    let box_w = text.chars().count() as f32 * 7.5 + 12.0;

                                    let bg_rect = Rectangle {
                                        x: mid_x - box_w / 2.0,
                                        y: mid_y - box_h - 4.0,
                                        width: box_w,
                                        height: box_h,
                                    };
                                    frame.fill_rectangle(bg_rect.position(), bg_rect.size(), Color::from_rgba8(0, 0, 0, 0.85));
                                    let border = CanvasPath::rectangle(bg_rect.position(), bg_rect.size());
                                    frame.stroke(&border, canvas::Stroke::default().with_color(color).with_width(1.5));
                                    frame.fill_text(canvas::Text {
                                        content: text,
                                        position: Point::new(mid_x, mid_y - box_h / 2.0 - 4.0),
                                        color: Color::WHITE,
                                        size: iced::Pixels(font_size),
                                        align_x: iced::widget::text::Alignment::Center,
                                        align_y: iced::alignment::Vertical::Center,
                                        ..Default::default()
                                    });
                                }
                            }

                            // ---------- Общая плашка «Длина» ----------
                            let mut total_m = 0.0;
                            for w in pts.windows(2) {
                                let (lon1, lat1) = w[0];
                                let (lon2, lat2) = w[1];
                                total_m += crate::domain::haversine_m(lat1, lon1, lat2, lon2);
                            }

                            let len_text = if total_m >= 1000.0 {
                                format!("{} {:.2} {}", crate::t!("msg_0256"), total_m / 1000.0, crate::t!("msg_0259"))
                            } else {
                                format!("{} {:.0} {}", crate::t!("msg_0256"), total_m, crate::t!("msg_0258"))
                            };

                            let font_size = 18.0;
                            let box_h = 28.0;
                            let box_w = len_text.chars().count() as f32 * 9.0 + 16.0;

                            let last = *screen_pts.last().unwrap();
                            let bg_rect = Rectangle {
                                x: last.x - box_w / 2.0,
                                y: last.y + 15.0,
                                width: box_w,
                                height: box_h,
                            };

                            frame.fill_rectangle(bg_rect.position(), bg_rect.size(), Color::from_rgba8(0x2a, 0x7f, 0xff, 0.9));
                            let border = CanvasPath::rectangle(bg_rect.position(), bg_rect.size());
                            frame.stroke(&border, canvas::Stroke::default().with_color(Color::WHITE).with_width(2.0));

                            frame.fill_text(canvas::Text {
                                content: len_text,
                                position: Point::new(last.x, last.y + 15.0 + box_h / 2.0),
                                color: Color::WHITE,
                                size: iced::Pixels(font_size),
                                align_x: iced::widget::text::Alignment::Center,
                                align_y: iced::alignment::Vertical::Center,
                                ..Default::default()
                            });
                        }
                    }
                }
            }
            
            // ==========================================
            // ЛИНЕЙКА (Ruler)
            // ==========================================
            if app.active_tool == crate::Tool::Ruler && !app.ruler_points.is_empty() {
                let color = Color::from_rgb8(255, 140, 0);

                // Используем только зафиксированные кликами точки (без курсора)
                let pts = app.ruler_points.clone();

                let mut sum_sx = 0.0;
                let mut sum_sy = 0.0;
                let mut screen_pts = Vec::new();

                // Отрисовка маркеров всех зафиксированных точек
                for &(lon, lat) in &app.ruler_points {
                    let (mx, my) = merc_of(lon, lat);
                    let sp = to_screen(mx, my);
                    
                    let circle = CanvasPath::circle(sp, 4.0);
                    frame.fill(&circle, color);
                    frame.stroke(&circle, canvas::Stroke::default().with_color(Color::WHITE).with_width(1.5));
                }
                
                for i in 0..pts.len() - 1 {
                    let (lon1, lat1) = pts[i];
                    let (lon2, lat2) = pts[i + 1];
                    let (mx1, my1) = merc_of(lon1, lat1);
                    let (mx2, my2) = merc_of(lon2, lat2);
                    let sp1 = to_screen(mx1, my1);
                    let sp2 = to_screen(mx2, my2);
                    
                    sum_sx += sp1.x;
                    sum_sy += sp1.y;
                    screen_pts.push(sp1);
                    if i == pts.len() - 2 {
                        screen_pts.push(sp2);
                    }

                    // Линия
                    let path = CanvasPath::line(sp1, sp2);
                    frame.stroke(&path, canvas::Stroke::default().with_color(color).with_width(2.0));

                    // Подпись
                    let dx_px = sp2.x - sp1.x;
                    let dy_px = sp2.y - sp1.y;
                    let screen_dist_px = (dx_px * dx_px + dy_px * dy_px).sqrt();

                    if screen_dist_px > 40.0 {
                        let dist_m = crate::domain::haversine_m(lat1, lon1, lat2, lon2);
                        let true_az = crate::util::geo::true_bearing(lat1, lon1, lat2, lon2);
                        let mut mag_az = true_az - app.declination;
                        if mag_az < 0.0 { mag_az += 360.0; }
                        if mag_az >= 360.0 { mag_az -= 360.0; }

                        let text = if dist_m >= 1000.0 {
                            format!("{:.2} {}, {:.0}°", dist_m / 1000.0, crate::t!("msg_0259"), mag_az)
                        } else {
                            format!("{:.0} {}, {:.0}°", dist_m, crate::t!("msg_0258"), mag_az)
                        };

                        let mid_x = (sp1.x + sp2.x) / 2.0;
                        let mid_y = (sp1.y + sp2.y) / 2.0;

                        let font_size = 14.0;
                        let box_h = 21.0;
                        let box_w = text.chars().count() as f32 * 7.5 + 12.0;

                        let bg_rect = Rectangle {
                            x: mid_x - box_w / 2.0,
                            y: mid_y - box_h - 4.0,
                            width: box_w,
                            height: box_h,
                        };
                        frame.fill_rectangle(bg_rect.position(), bg_rect.size(), Color::from_rgba8(0, 0, 0, 0.85));
                        let border = CanvasPath::rectangle(bg_rect.position(), bg_rect.size());
                        frame.stroke(&border, canvas::Stroke::default().with_color(color).with_width(1.5));
                        frame.fill_text(canvas::Text {
                            content: text,
                            position: Point::new(mid_x, mid_y - box_h / 2.0 - 4.0),
                            color: Color::WHITE,
                            size: iced::Pixels(font_size),
                            align_x: iced::widget::text::Alignment::Center,
                            align_y: iced::alignment::Vertical::Center,
                            ..Default::default()
                        });
                    }
                }

                // The two-line summary appears only after the third click.
                // Length is the open polyline (no closing segment); area is
                // calculated from the closed polygon as before.
                if app.ruler_points.len() >= 3 {
                    let total_m: f64 = pts.windows(2)
                        .map(|pair| {
                            let (lon1, lat1) = pair[0];
                            let (lon2, lat2) = pair[1];
                            crate::domain::haversine_m(lat1, lon1, lat2, lon2)
                        })
                        .sum();
                    let length_text = if total_m > 1000.0 {
                        format!("{} {:.1} {}", crate::t!("msg_0256"), total_m / 1000.0, crate::t!("msg_0259"))
                    } else {
                        format!("{} {:.0} {}", crate::t!("msg_0256"), total_m, crate::t!("msg_0258"))
                    };

                    let n = pts.len();
                    let lat_c = pts.iter().map(|p| p.1).sum::<f64>() / n as f64;
                    let kx = 111320.0 * lat_c.to_radians().cos();
                    let ky = 111320.0;
                    
                    let mut area_m2 = 0.0;
                    for i in 0..n {
                        let j = (i + 1) % n;
                        let x1 = pts[i].0 * kx; let y1 = pts[i].1 * ky;
                        let x2 = pts[j].0 * kx; let y2 = pts[j].1 * ky;
                        area_m2 += x1 * y2 - x2 * y1;
                    }
                    area_m2 = (area_m2 / 2.0).abs();
                    
                    let area_text = format!("{} {}", crate::t!("msg_0257"), format_area(area_m2));

                    let cx = sum_sx / (screen_pts.len() as f32 - 1.0);
                    let cy = sum_sy / (screen_pts.len() as f32 - 1.0);
                    
                    let font_size = 18.0;
                    let box_h = 56.0;
                    let box_w = length_text.chars().count().max(area_text.chars().count()) as f32 * 9.0 + 16.0;
                    
                    let bg_rect = Rectangle {
                        x: cx - box_w / 2.0,
                        y: cy - box_h / 2.0,
                        width: box_w,
                        height: box_h,
                    };
                    
                    frame.fill_rectangle(bg_rect.position(), bg_rect.size(), Color::from_rgba8(255, 140, 0, 0.9));
                    let border = CanvasPath::rectangle(bg_rect.position(), bg_rect.size());
                    frame.stroke(&border, canvas::Stroke::default().with_color(Color::WHITE).with_width(2.0));

                    for (content, y) in [(length_text, cy - 13.0), (area_text, cy + 13.0)] {
                        frame.fill_text(canvas::Text {
                            content,
                            position: Point::new(cx, y),
                            color: Color::WHITE,
                            size: iced::Pixels(font_size),
                            align_x: iced::widget::text::Alignment::Center,
                            align_y: iced::alignment::Vertical::Center,
                            ..Default::default()
                        });
                    }
                }
            }

            // ==========================================
            // ПРИЦЕЛ ИЗ ТАБЛИЦЫ ТРЕКОВ (TrackList)
            // ==========================================
            if let Some(crate::Modal::TrackList(state)) = &app.modal {
                if let Some((sect, pt_idx)) = state.last_clicked {
                    if let Some(t) = app.tracks.iter().find(|t| t.id == state.track_id) {
                        if sect > 0 && sect <= t.segments.len() {
                            let seg = &t.segments[sect - 1];
                            if pt_idx < seg.points.len() {
                                let pt = &seg.points[pt_idx];
                                let (mx, my) = merc_of(pt.lon, pt.lat);
                                draw_crosshair(frame, to_screen(mx, my));
                            }
                        }
                    }
                }
            }
        });
        vec![geom]
    }
}
