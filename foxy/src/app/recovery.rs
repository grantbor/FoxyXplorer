use std::collections::HashSet;
use std::sync::Arc;

use iced::Task;

use crate::{App, Message, Tool, TrackInspection, InspectionExtraPass};
use crate::domain::{self, recovery};

//// Return exactly the review-only entries; suggested ranges are managed by
/// a separate selection and must not be changed by the bulk review buttons.
fn review_indices(episodes: &[domain::quality::Episode]) -> HashSet<usize> {
    episodes.iter().enumerate()
        .filter_map(|(i, ep)| (ep.verdict == domain::quality::Verdict::Review).then_some(i))
        .collect()
}

// Only explicitly checked ranges are passed to the preview and to Copy.
/// Review-only episodes are not in the automatic proposal list; checking one
/// means that the operator has manually confirmed it on the map.
fn ranges_for_selection(
    report: &recovery::Report, selected: &HashSet<usize>,
    selected_reviews: &HashSet<usize>, track: &domain::Track,
) -> Vec<recovery::SuspectRange> {
    let mut chosen: Vec<_> = selected.iter()
        .filter_map(|&i| report.ranges.get(i).cloned()).collect();
    for index in selected_reviews {
        let Some(ep) = report.quality.episodes.get(*index) else { continue; };
        if ep.verdict != domain::quality::Verdict::Review { continue; }
        let Some(seg) = track.segments.get(ep.segment) else { continue; };
        if ep.start > ep.end || ep.end >= seg.points.len() { continue; }
        let dist=|a: &domain::TrackPoint, b: &domain::TrackPoint|
            domain::haversine_m(a.lat,a.lon,b.lat,b.lon);
        // v3.4: the operator may confirm a suspect first/last fix or tail.
        // Such ranges have no second anchor. Never invent a bridge over a gap.
        let entry_m = if ep.start > 0 {
            dist(&seg.points[ep.start-1], &seg.points[ep.start])
        } else { f64::NAN };
        let exit_m = if ep.end+1 < seg.points.len() {
            dist(&seg.points[ep.end], &seg.points[ep.end+1])
        } else { f64::NAN };
        let bridge_m = if ep.start > 0 && ep.end+1 < seg.points.len() {
            dist(&seg.points[ep.start-1], &seg.points[ep.end+1])
        } else { f64::NAN };
        chosen.push(recovery::SuspectRange {
            segment: ep.segment,start:ep.start,end:ep.end,
            entry_m, exit_m, bridge_m,
        });
    }
    chosen
}

fn chosen_ranges(inspect: &TrackInspection, track: &domain::Track) -> Vec<recovery::SuspectRange> {
    ranges_for_selection(&inspect.report, &inspect.selected,
        &inspect.selected_reviews, track)
}

/// All later selections are expressed relative to the first-pass geometry.
/// Never apply their (segment, point) indexes to the original track.
// A small hard limit prevents unbounded manual iterations on irreparably
// damaged tracks. Each scan is explicit and every new checkbox starts clear.
const MAX_EXTRA_PASSES: usize = 3;

fn has_further_scan_input(inspect: &TrackInspection) -> bool {
    if inspect.extra_passes.len() >= MAX_EXTRA_PASSES { return false; }
    if let Some(last) = inspect.extra_passes.last() {
        !last.selected.is_empty() || !last.selected_reviews.is_empty()
    } else {
        !inspect.second_selected.is_empty() || !inspect.second_selected_reviews.is_empty()
    }
}

fn recompute_final_preview(inspect: &mut TrackInspection, original: &domain::Track) {
    let Some(first) = &inspect.first_pass_segments else { return; };
    let first_track = recovery::with_segments(original, first.clone());
    let mut result = first.clone();
    if let Some(report) = &inspect.second_report {
        let ranges = ranges_for_selection(report, &inspect.second_selected,
            &inspect.second_selected_reviews, &first_track);
        if !ranges.is_empty() {
            result = recovery::exclude_ranges(&first_track, &ranges);
        }
    }
    for pass in &inspect.extra_passes {
        let source = recovery::with_segments(original, pass.source_segments.clone());
        let ranges = ranges_for_selection(&pass.report, &pass.selected,
            &pass.selected_reviews, &source);
        if !ranges.is_empty() {
            result = recovery::exclude_ranges(&source, &ranges);
        }
    }
    let remaining_report = recovery::analyze_with_mode(
        &recovery::with_segments(original, result.clone()), inspect.movement_mode);
    inspect.preview_residual = Some((remaining_report.ranges.len(),
        remaining_report.quality.episodes.iter()
            .filter(|ep| ep.verdict == domain::quality::Verdict::Review).count()));
    inspect.preview_segments = Some(result);
}

/// Discard second-pass point indexes whenever the initial cuts change.
fn reset_inspection_preview(inspect: &mut TrackInspection) {
    inspect.preview_segments = None;
    inspect.preview_residual = None;
    inspect.first_pass_segments = None;
    inspect.second_report = None;
    inspect.second_selected.clear();
    inspect.second_selected_reviews.clear();
    inspect.second_focused = None;
    inspect.extra_passes.clear();
}

/// For every point of the first-pass preview, remember its index in the
/// original source. A preview reindexes segments when exclusions create gaps.
pub(crate) fn first_pass_source_indices(
    inspect: &TrackInspection, original: &domain::Track,
) -> Vec<Vec<(usize, usize)>> {
    if inspect.report.can_reorder {
        return original.segments.iter().enumerate().map(|(seg_idx, seg)| {
            let mut indexes: Vec<_> = (0..seg.points.len()).collect();
            indexes.sort_by(|&a, &b| {
                seg.points[a].time.unwrap_or(f64::NAN)
                    .total_cmp(&seg.points[b].time.unwrap_or(f64::NAN))
            });
            indexes.into_iter().map(|idx| (seg_idx, idx)).collect()
        }).collect();
    }
    let cut = chosen_ranges(inspect, original);
    let mut result: Vec<Vec<(usize,usize)>> = Vec::new();
    for (seg_idx, seg) in original.segments.iter().enumerate() {
        let mut excluded = vec![false; seg.points.len()];
        for r in cut.iter().filter(|r| r.segment == seg_idx) {
            if r.start >= excluded.len() { continue; }
            for i in r.start..=r.end.min(excluded.len()-1) { excluded[i]=true; }
        }
        let mut chunk = Vec::new();
        for index in 0..seg.points.len() {
            if excluded[index] {
                if !chunk.is_empty() { result.push(std::mem::take(&mut chunk)); }
            } else {
                chunk.push((seg_idx,index));
            }
        }
        if !chunk.is_empty() { result.push(chunk); }
    }
    result
}

/// Apply exclusions to the source-index map, preserving every created split.
/// The exact same index geometry is used by recovery::exclude_ranges.
fn exclude_source_indices(
    mapping: Vec<Vec<(usize, usize)>>, ranges: &[recovery::SuspectRange],
) -> Vec<Vec<(usize, usize)>> {
    let mut result = Vec::new();
    for (seg_idx, seg) in mapping.into_iter().enumerate() {
        let mut excluded = vec![false; seg.len()];
        for range in ranges.iter().filter(|r| r.segment == seg_idx) {
            for index in range.start..=range.end.min(seg.len().saturating_sub(1)) {
                if let Some(mark) = excluded.get_mut(index) { *mark = true; }
            }
        }
        let mut chunk = Vec::new();
        for (index, point) in seg.into_iter().enumerate() {
            if excluded[index] {
                if !chunk.is_empty() { result.push(std::mem::take(&mut chunk)); }
            } else {
                chunk.push(point);
            }
        }
        if !chunk.is_empty() { result.push(chunk); }
    }
    result
}

pub(crate) fn source_indices_for_extra(
    inspect: &TrackInspection, original: &domain::Track, pass_index: usize,
) -> Option<Vec<Vec<(usize, usize)>>> {
    let first = inspect.first_pass_segments.as_ref()?;
    let mut mapping = first_pass_source_indices(inspect, original);
    let source = recovery::with_segments(original, first.clone());
    let second = inspect.second_report.as_ref()?;
    let ranges = ranges_for_selection(second, &inspect.second_selected,
        &inspect.second_selected_reviews, &source);
    mapping = exclude_source_indices(mapping, &ranges);
    for extra in inspect.extra_passes.iter().take(pass_index) {
        let source = recovery::with_segments(original, extra.source_segments.clone());
        let ranges = ranges_for_selection(&extra.report, &extra.selected,
            &extra.selected_reviews, &source);
        mapping = exclude_source_indices(mapping, &ranges);
    }
    Some(mapping)
}

/// Preserve exactly the points the operator chose to inspect when leaving the
/// diagnostic sidebar. These are ordinary editable point selections afterwards;
/// the user can extend the selection and delete with a gap (Shift+Delete).
fn points_to_select_after_inspection(
    inspect: &TrackInspection, track: &domain::Track,
) -> HashSet<(u64, usize, usize)> {
    let mut areas: Vec<(usize, usize, usize)> = Vec::new();
    if let Some(i) = inspect.focused_quality {
        if let Some(ep) = inspect.report.quality.episodes.get(i) {
            areas.push((ep.segment, ep.start, ep.end));
        }
    } else if let Some(i) = inspect.focused {
        if let Some(r) = inspect.report.ranges.get(i) {
            areas.push((r.segment, r.start, r.end));
        }
    } else if let Some(i) = inspect.focused_jump {
        if let Some(j) = inspect.report.severe_jumps.get(i) {
            areas.push((j.segment, j.to_index.saturating_sub(1), j.to_index));
        }
    } else {
        // No focused item: preserve the checked candidates for inspection.
        for &i in &inspect.selected {
            if let Some(r) = inspect.report.ranges.get(i) {
                areas.push((r.segment, r.start, r.end));
            }
        }
        for &i in &inspect.selected_reviews {
            if let Some(ep) = inspect.report.quality.episodes.get(i) {
                areas.push((ep.segment, ep.start, ep.end));
            }
        }
    }
    // The focused later pass uses indexes after ALL earlier approved cuts.
    // Never reuse these indexes against the original track.
    if let Some((pass_index, pass)) = inspect.extra_passes.iter().enumerate()
        .find(|(_, p)| p.focused.is_some())
    {
        if let Some((is_review, index)) = pass.focused {
            let region = if is_review {
                pass.report.quality.episodes.get(index).and_then(|ep| {
                    (ep.verdict == domain::quality::Verdict::Review)
                        .then_some((ep.segment, ep.start, ep.end))
                })
            } else {
                pass.report.ranges.get(index).map(|r| (r.segment, r.start, r.end))
            };
            if let (Some((seg, start, end)), Some(mapping)) =
                (region, source_indices_for_extra(inspect, track, pass_index))
            {
                if let Some(indexes) = mapping.get(seg) {
                    if start <= end && end < indexes.len() {
                        return indexes[start..=end].iter().map(|&(source_seg, source_idx)|
                            (track.id, source_seg, source_idx)).collect();
                    }
                }
            }
        }
    }
    // A second-pass focus is indexed within the first-pass preview; convert
    // through an explicit source-index map before returning to the editor.
    if let (Some((is_review, index)), Some(second)) = (
        inspect.second_focused, &inspect.second_report,
    ) {
        let region = if is_review {
            second.quality.episodes.get(index).and_then(|ep| {
                (ep.verdict == domain::quality::Verdict::Review)
                    .then_some((ep.segment, ep.start, ep.end))
            })
        } else {
            second.ranges.get(index).map(|r| (r.segment, r.start, r.end))
        };
        if let Some((seg, start, end)) = region {
            let map = first_pass_source_indices(inspect, track);
            let mut selected = HashSet::new();
            if let Some(indexes) = map.get(seg) {
                if start <= end && end < indexes.len() {
                    for &(original_seg, original_idx) in &indexes[start..=end] {
                        selected.insert((track.id, original_seg, original_idx));
                    }
                }
            }
            if !selected.is_empty() { return selected; }
        }
    }
    let mut chosen = HashSet::new();
    for (seg_idx, start, end) in areas {
        let Some(seg) = track.segments.get(seg_idx) else { continue; };
        if start > end || start >= seg.points.len() { continue; }
        for pt_idx in start..=end.min(seg.points.len() - 1) {
            chosen.insert((track.id, seg_idx, pt_idx));
        }
    }
    chosen
}


/// Select every found coordinate candidate, including manual-review episodes.
/// Unpaired impossible transitions select BOTH endpoints because their bad
/// side is unknown. Only the operator can choose which points to delete.
fn cleanup_point_selection(
    track: &domain::Track, report: &recovery::Report,
) -> HashSet<(u64, usize, usize)> {
    let mut selected = HashSet::new();
    let mut add_interval = |seg_idx: usize, start: usize, end: usize| {
        let Some(segment) = track.segments.get(seg_idx) else { return; };
        if start > end || start >= segment.points.len() { return; }
        for point in start..=end.min(segment.points.len() - 1) {
            selected.insert((track.id, seg_idx, point));
        }
    };
    for range in &report.ranges {
        add_interval(range.segment, range.start, range.end);
    }
    for episode in &report.quality.episodes {
        add_interval(episode.segment, episode.start, episode.end);
    }
    for jump in &report.severe_jumps {
        if jump.to_index > 0 {
            add_interval(jump.segment, jump.to_index - 1, jump.to_index);
        }
    }
    selected
}


/// Infer from a safe chronological ordering when a repair is possible.
/// Without such a repair, never fabricate timestamps or merge segments.
fn one_click_auto_profile(
    track: &domain::Track, chronology: &recovery::Report,
) -> recovery::MovementMode {
    if chronology.can_reorder {
        recovery::chronological_segments(track)
            .map(|ordered| domain::profile::infer_movement_mode(
                &recovery::with_segments(track, ordered)))
            .unwrap_or(recovery::MovementMode::Unknown)
    } else {
        domain::profile::infer_movement_mode(track)
    }
}

fn one_click_profile_label(mode: recovery::MovementMode) -> &'static str {
    match mode {
        recovery::MovementMode::Vehicle => crate::t!("msg_0606"),
        recovery::MovementMode::Foot => crate::t!("msg_0607"),
        recovery::MovementMode::Unknown => crate::t!("msg_0608"),
    }
}

impl App {
    pub(crate) fn update_recovery(&mut self, m: Message) -> Task<Message> {
        match m {
            Message::InspectTrack(id) => {
                if self.modal.is_some() { return Task::none(); }
                let Some(original) = self.tracks.iter().find(|t| t.id == id) else {
                    return Task::none();
                };
                // Both entry paths use exactly the existing v3.4 detector.
                // --cleanup restores the complete v3.4 diagnostic panel with
                // mode selection, editable intervals, repeated scans, preview,
                // and explicit copy creation. Normal launches never enter it.
                let vehicle_report = recovery::analyze_with_mode(
                    original, recovery::MovementMode::Vehicle);
                // The diagnostic --cleanup path retains its manual profile
                // selector. Only the ordinary one-click path infers a profile.
                // If chronology is safely repairable, infer movement from the
                // reordered geometry, never from its scrambled timestamps.
                let mode = if self.cleanup_diagnostics {
                    recovery::MovementMode::Vehicle
                } else {
                    one_click_auto_profile(original, &vehicle_report)
                };
                let report = if mode == recovery::MovementMode::Vehicle {
                    vehicle_report
                } else {
                    recovery::analyze_with_mode(original, mode)
                };
                if self.cleanup_diagnostics {
                    self.track_inspection = Some(TrackInspection {
                        track_id: id, report, movement_mode: mode,
                        focused: None, focused_jump: None, focused_quality: None,
                        selected: HashSet::new(), selected_reviews: HashSet::new(),
                        first_pass_segments: None, second_report: None,
                        second_selected: HashSet::new(), second_selected_reviews: HashSet::new(),
                        second_focused: None, extra_passes: Vec::new(),
                        preview_segments: None, preview_residual: None,
                    });
                    self.active_tool = Tool::Pan;
                    self.dragging = false;
                    self.search_focused = false;
                    self.selection.tracks.clear();
                    self.selection.tracks.insert(id);
                    self.selection.points.clear();
                    self.tracks_cache.clear();
                    return Task::none();
                }
                // In ordinary launches a single action either reorders a
                // safely repairable chronology with Undo or selects suspect
                // fixes for manual point editing; no copies and no deletion.
                if report.can_reorder {
                    let Some(ordered) = recovery::chronological_segments(original) else {
                        self.status = format!("{} · {}", crate::t!("msg_0603"),
                            one_click_profile_label(mode));
                        return Task::none();
                    };
                    self.push_undo(crate::t!("msg_0602"));
                    if let Some(track) = self.tracks.iter_mut().find(|t| t.id == id) {
                        let track = Arc::make_mut(track);
                        track.segments = ordered;
                        domain::recompute_track_stats(track);
                    }
                    self.selection.points.clear(); // indices changed by sorting
                    self.selection.tracks.clear();
                    self.selection.tracks.insert(id);
                    self.track_inspection = None;
                    self.tracks_cache.clear();
                    self.status = crate::t!("msg_0601").into();
                } else if report.reversals > 0 || report.repeated_times > 0 {
                    // Do not pretend an ambiguous chronology has been fixed.
                    self.status = crate::t!("msg_0603").into();
                } else {
                    let selected = cleanup_point_selection(original, &report);
                    if selected.is_empty() {
                        self.status = (if report.missing_times > 0 {
                            crate::t!("msg_0603")
                        } else {
                            crate::t!("msg_0600")
                        }).into();
                    } else {
                        let count = selected.len();
                        self.track_inspection = None;
                        self.selection.tracks.clear();
                        self.selection.tracks.insert(id);
                        self.selection.points = selected;
                        self.last_point_clicked = None;
                        self.active_tool = Tool::SelectPoints;
                        self.dragging = false;
                        self.search_focused = false;
                        self.tracks_cache.clear();
                        self.status = if report.extensive_damage {
                            crate::tf!("msg_0605", count = count)
                        } else {
                            crate::tf!("msg_0604", count = count)
                        };
                    }
                }
                // Include the automatically chosen profile in EVERY ordinary
                // cleanup result (healthy, restored, suspects, or warning).
                // The existing five-second status timeout is unchanged.
                self.status = format!("{} · {}", self.status,
                    one_click_profile_label(mode));
            }
            Message::InspectionClose => {
                if let Some(inspect) = self.track_inspection.take() {
                    if let Some(track) = self.tracks.iter().find(|t| t.id == inspect.track_id) {
                        let selected = points_to_select_after_inspection(&inspect, track);
                        if !selected.is_empty() {
                            self.selection.tracks.clear();
                            self.selection.tracks.insert(inspect.track_id);
                            self.selection.points = selected;
                            self.active_tool = Tool::SelectPoints;
                        }
                    }
                }
                // Selection must survive dismissing the sidebar. Otherwise
                // "Show on map" only flashes an overlay that cannot be edited.
                self.tracks_cache.clear();
            }
            Message::InspectionFocus(index) => {
                let Some(inspect) = &mut self.track_inspection else { return Task::none() };
                let Some(range) = inspect.report.ranges.get(index) else { return Task::none() };
                inspect.focused = Some(index);
                inspect.second_focused = None;
                for extra in &mut inspect.extra_passes { extra.focused = None; }
                inspect.focused_jump = None;
                inspect.focused_quality = None;
                let Some(track) = self.tracks.iter().find(|t| t.id == inspect.track_id) else {
                    return Task::none();
                };
                let Some(seg) = track.segments.get(range.segment) else { return Task::none() };
                // Focus on the suspected cluster, not on distant teleport
                // endpoints; otherwise a 130 km jump makes it microscopic.
                let start = range.start;
                let end = range.end;
                let mut bounds = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
                for pt in &seg.points[start..=end] {
                    let (mx, my) = crate::util::geo::merc_of(pt.lon, pt.lat);
                    bounds.0 = bounds.0.min(mx);
                    bounds.1 = bounds.1.min(my);
                    bounds.2 = bounds.2.max(mx);
                    bounds.3 = bounds.3.max(my);
                }
                if bounds.0.is_finite() {
                    self.cx = (bounds.0 + bounds.2) * 0.5;
                    self.cy = (bounds.1 + bounds.3) * 0.5;
                    let viewport = self.last_bounds.get();
                    let sx = (bounds.2 - bounds.0).max(20.0) / (f64::from(viewport.width) * 0.75).max(1.0);
                    let sy = (bounds.3 - bounds.1).max(20.0) / (f64::from(viewport.height) * 0.75).max(1.0);
                    self.scale = sx.max(sy).clamp(0.05, 10_000.0);
                    self.canvas_cache.clear();
                    self.grid_cache.clear();
                    self.wiki_cache.clear();
                    self.osm_canvas_cache.clear();
                }
                self.dragging = false;
                self.tracks_cache.clear();
                return self.request_osm_tiles();
            }
            Message::InspectionMovementChanged(mode) => {
                let Some(inspect) = &mut self.track_inspection else { return Task::none() };
                if inspect.movement_mode == mode { return Task::none(); }
                let Some(original) = self.tracks.iter().find(|t| t.id == inspect.track_id) else {
                    return Task::none();
                };
                inspect.movement_mode = mode;
                inspect.report = recovery::analyze_with_mode(original, mode);
                // Index-based selections would now refer to DIFFERENT intervals.
                inspect.selected.clear();
                inspect.selected_reviews.clear();
                inspect.focused = None;
                inspect.focused_jump = None;
                inspect.focused_quality = None;
                reset_inspection_preview(inspect);
                self.tracks_cache.clear();
            }
            Message::InspectionFocusJump(index) => {
                let Some(inspect) = &mut self.track_inspection else { return Task::none() };
                let Some(jump) = inspect.report.severe_jumps.get(index) else {
                    return Task::none();
                };
                let Some(track) = self.tracks.iter().find(|t| t.id == inspect.track_id) else {
                    return Task::none();
                };
                let Some(seg) = track.segments.get(jump.segment) else { return Task::none() };
                if jump.to_index == 0 || jump.to_index >= seg.points.len() { return Task::none(); }
                let a = &seg.points[jump.to_index - 1];
                let b = &seg.points[jump.to_index];
                let (ax, ay) = crate::util::geo::merc_of(a.lon, a.lat);
                let (bx, by) = crate::util::geo::merc_of(b.lon, b.lat);
                inspect.focused_jump = Some(index);
                inspect.second_focused = None;
                for extra in &mut inspect.extra_passes { extra.focused = None; }
                inspect.focused = None;
                inspect.focused_quality = None;
                self.cx = (ax + bx) * 0.5;
                self.cy = (ay + by) * 0.5;
                let viewport = self.last_bounds.get();
                self.scale = ((ax - bx).abs().max(40.0) / (f64::from(viewport.width) * 0.7).max(1.0))
                    .max((ay - by).abs().max(40.0) / (f64::from(viewport.height) * 0.7).max(1.0))
                    .clamp(0.05, 10_000.0);
                self.dragging = false;
                self.canvas_cache.clear();
                self.grid_cache.clear();
                self.wiki_cache.clear();
                self.osm_canvas_cache.clear();
                self.tracks_cache.clear();
                return self.request_osm_tiles();
            }
            Message::InspectionFocusQuality(index) => {
                let Some(inspect) = &mut self.track_inspection else { return Task::none() };
                let Some(ep) = inspect.report.quality.episodes.get(index) else { return Task::none() };
                let Some(track) = self.tracks.iter().find(|t| t.id == inspect.track_id) else {
                    return Task::none();
                };
                let Some(seg) = track.segments.get(ep.segment) else { return Task::none() };
                if ep.end >= seg.points.len() { return Task::none(); }
                let mut bounds = (f64::INFINITY,f64::INFINITY,f64::NEG_INFINITY,f64::NEG_INFINITY);
                for pt in &seg.points[ep.start..=ep.end] {
                    let (mx,my)=crate::util::geo::merc_of(pt.lon,pt.lat);
                    bounds.0=bounds.0.min(mx);
                    bounds.1=bounds.1.min(my);
                    bounds.2=bounds.2.max(mx);
                    bounds.3=bounds.3.max(my);
                }
                if !bounds.0.is_finite() { return Task::none(); }
                inspect.focused_quality=Some(index);
                inspect.second_focused=None;
                for extra in &mut inspect.extra_passes { extra.focused = None; }
                inspect.focused=None;
                inspect.focused_jump=None;
                self.cx=(bounds.0+bounds.2)*0.5;
                self.cy=(bounds.1+bounds.3)*0.5;
                let viewport=self.last_bounds.get();
                let sx=(bounds.2-bounds.0).max(25.0)/(f64::from(viewport.width)*0.75).max(1.0);
                let sy=(bounds.3-bounds.1).max(25.0)/(f64::from(viewport.height)*0.75).max(1.0);
                self.scale=sx.max(sy).clamp(0.05,10_000.0);
                self.dragging=false;
                self.canvas_cache.clear();
                self.grid_cache.clear();
                self.wiki_cache.clear();
                self.osm_canvas_cache.clear();
                self.tracks_cache.clear();
                return self.request_osm_tiles();
            }
            Message::InspectionToggle(index, checked) => {
                let Some(inspect) = &mut self.track_inspection else { return Task::none() };
                if index >= inspect.report.ranges.len() { return Task::none(); }
                if checked { inspect.selected.insert(index); }
                else { inspect.selected.remove(&index); }
                reset_inspection_preview(inspect);
                self.tracks_cache.clear();
            }
            Message::InspectionToggleReview(index, checked) => {
                let Some(inspect)=&mut self.track_inspection else { return Task::none(); };
                let Some(ep)=inspect.report.quality.episodes.get(index) else { return Task::none(); };
                if ep.verdict != domain::quality::Verdict::Review { return Task::none(); }
                if checked { inspect.selected_reviews.insert(index); }
                else { inspect.selected_reviews.remove(&index); }
                reset_inspection_preview(inspect);
                self.tracks_cache.clear();
            }
            Message::InspectionSelectAll(checked) => {
                let Some(inspect) = &mut self.track_inspection else { return Task::none() };
                inspect.selected.clear();
                if checked {
                    inspect.selected.extend(0..inspect.report.ranges.len());
                }
                reset_inspection_preview(inspect);
                self.tracks_cache.clear();
            }
            Message::InspectionSelectAllReviews(checked) => {
                let Some(inspect) = &mut self.track_inspection else { return Task::none() };
                inspect.selected_reviews.clear();
                if checked {
                    inspect.selected_reviews = review_indices(&inspect.report.quality.episodes);
                }
                // Any change to the selected cuts invalidates the old preview.
                reset_inspection_preview(inspect);
                self.tracks_cache.clear();
            }
            Message::InspectionToggleSecond(index, checked) => {
                let Some(inspect) = &mut self.track_inspection else { return Task::none(); };
                let Some(report) = &inspect.second_report else { return Task::none(); };
                if index >= report.ranges.len() { return Task::none(); }
                if checked { inspect.second_selected.insert(index); }
                else { inspect.second_selected.remove(&index); }
                inspect.extra_passes.clear();
                if let Some(original) = self.tracks.iter().find(|t| t.id == inspect.track_id) {
                    recompute_final_preview(inspect, original);
                }
                self.tracks_cache.clear();
            }
            Message::InspectionToggleSecondReview(index, checked) => {
                let Some(inspect) = &mut self.track_inspection else { return Task::none(); };
                let Some(ep) = inspect.second_report.as_ref()
                    .and_then(|r| r.quality.episodes.get(index)) else { return Task::none(); };
                if ep.verdict != domain::quality::Verdict::Review { return Task::none(); }
                if checked { inspect.second_selected_reviews.insert(index); }
                else { inspect.second_selected_reviews.remove(&index); }
                inspect.extra_passes.clear();
                if let Some(original) = self.tracks.iter().find(|t| t.id == inspect.track_id) {
                    recompute_final_preview(inspect, original);
                }
                self.tracks_cache.clear();
            }
            Message::InspectionFocusSecond(index, is_review) => {
                let Some(inspect) = &mut self.track_inspection else { return Task::none(); };
                let Some(report) = &inspect.second_report else { return Task::none(); };
                let (segment, start, end) = if is_review {
                    let Some(ep) = report.quality.episodes.get(index) else { return Task::none() };
                    if ep.verdict != domain::quality::Verdict::Review { return Task::none(); }
                    (ep.segment, ep.start, ep.end)
                } else {
                    let Some(r) = report.ranges.get(index) else { return Task::none() };
                    (r.segment, r.start, r.end)
                };
                let Some(seg) = inspect.first_pass_segments.as_ref()
                    .and_then(|s| s.get(segment)) else { return Task::none(); };
                if start > end || end >= seg.points.len() { return Task::none(); }
                let mut bounds = (f64::INFINITY, f64::INFINITY,
                    f64::NEG_INFINITY, f64::NEG_INFINITY);
                for pt in &seg.points[start..=end] {
                    let (mx,my) = crate::util::geo::merc_of(pt.lon,pt.lat);
                    bounds.0=bounds.0.min(mx); bounds.1=bounds.1.min(my);
                    bounds.2=bounds.2.max(mx); bounds.3=bounds.3.max(my);
                }
                if !bounds.0.is_finite() { return Task::none(); }
                inspect.second_focused = Some((is_review,index));
                for extra in &mut inspect.extra_passes { extra.focused = None; }
                inspect.focused = None;
                inspect.focused_quality = None;
                inspect.focused_jump = None;
                self.cx=(bounds.0+bounds.2)*0.5;
                self.cy=(bounds.1+bounds.3)*0.5;
                let viewport=self.last_bounds.get();
                let sx=(bounds.2-bounds.0).max(25.0)/(f64::from(viewport.width)*0.75).max(1.0);
                let sy=(bounds.3-bounds.1).max(25.0)/(f64::from(viewport.height)*0.75).max(1.0);
                self.scale=sx.max(sy).clamp(0.05,10_000.0);
                self.dragging=false;
                self.canvas_cache.clear(); self.grid_cache.clear();
                self.wiki_cache.clear(); self.osm_canvas_cache.clear();
                self.tracks_cache.clear();
                return self.request_osm_tiles();
            }
            Message::InspectionScanAgain => {
                let Some(inspect) = &mut self.track_inspection else { return Task::none(); };
                if inspect.preview_segments.is_none() || !has_further_scan_input(inspect) {
                    return Task::none();
                }
                let Some(original) = self.tracks.iter().find(|t| t.id == inspect.track_id) else {
                    return Task::none();
                };
                let source_segments = inspect.preview_segments.as_ref().unwrap().clone();
                let track = recovery::with_segments(original, source_segments.clone());
                let report = recovery::analyze_with_mode(&track, inspect.movement_mode);
                inspect.extra_passes.push(InspectionExtraPass {
                    source_segments, report, selected: HashSet::new(),
                    selected_reviews: HashSet::new(), focused: None,
                });
                inspect.second_focused = None;
                self.tracks_cache.clear();
            }
            Message::InspectionToggleExtra(pass_index, index, is_review, checked) => {
                let Some(inspect) = &mut self.track_inspection else { return Task::none(); };
                let Some(pass) = inspect.extra_passes.get_mut(pass_index) else { return Task::none(); };
                if is_review {
                    if pass.report.quality.episodes.get(index)
                        .is_none_or(|ep| ep.verdict != domain::quality::Verdict::Review) {
                        return Task::none();
                    }
                    if checked { pass.selected_reviews.insert(index); }
                    else { pass.selected_reviews.remove(&index); }
                } else {
                    if index >= pass.report.ranges.len() { return Task::none(); }
                    if checked { pass.selected.insert(index); }
                    else { pass.selected.remove(&index); }
                }
                // All later diagnostics were indexed into the OLD geometry.
                inspect.extra_passes.truncate(pass_index + 1);
                if let Some(original) = self.tracks.iter().find(|t| t.id == inspect.track_id) {
                    recompute_final_preview(inspect, original);
                }
                self.tracks_cache.clear();
            }
            Message::InspectionFocusExtra(pass_index, index, is_review) => {
                let Some(inspect) = &mut self.track_inspection else { return Task::none(); };
                let Some(pass) = inspect.extra_passes.get(pass_index) else { return Task::none(); };
                let (segment, start, end) = if is_review {
                    let Some(ep) = pass.report.quality.episodes.get(index) else { return Task::none(); };
                    if ep.verdict != domain::quality::Verdict::Review { return Task::none(); }
                    (ep.segment, ep.start, ep.end)
                } else {
                    let Some(range) = pass.report.ranges.get(index) else { return Task::none(); };
                    (range.segment, range.start, range.end)
                };
                let Some(seg) = pass.source_segments.get(segment) else { return Task::none(); };
                if start > end || end >= seg.points.len() { return Task::none(); }
                let mut bounds = (f64::INFINITY, f64::INFINITY,
                    f64::NEG_INFINITY, f64::NEG_INFINITY);
                for pt in &seg.points[start..=end] {
                    let (mx,my) = crate::util::geo::merc_of(pt.lon,pt.lat);
                    bounds.0=bounds.0.min(mx); bounds.1=bounds.1.min(my);
                    bounds.2=bounds.2.max(mx); bounds.3=bounds.3.max(my);
                }
                if !bounds.0.is_finite() { return Task::none(); }
                for other in &mut inspect.extra_passes { other.focused = None; }
                inspect.extra_passes[pass_index].focused = Some((is_review,index));
                inspect.second_focused = None;
                inspect.focused = None;
                inspect.focused_quality = None;
                inspect.focused_jump = None;
                self.cx=(bounds.0+bounds.2)*0.5;
                self.cy=(bounds.1+bounds.3)*0.5;
                let viewport=self.last_bounds.get();
                let sx=(bounds.2-bounds.0).max(25.0)/(f64::from(viewport.width)*0.75).max(1.0);
                let sy=(bounds.3-bounds.1).max(25.0)/(f64::from(viewport.height)*0.75).max(1.0);
                self.scale=sx.max(sy).clamp(0.05,10_000.0);
                self.dragging=false;
                self.canvas_cache.clear(); self.grid_cache.clear();
                self.wiki_cache.clear(); self.osm_canvas_cache.clear();
                self.tracks_cache.clear();
                return self.request_osm_tiles();
            }
            Message::InspectionPreview => {
                let Some(inspect) = &self.track_inspection else { return Task::none() };
                if inspect.preview_segments.is_some() {
                    let inspect = self.track_inspection.as_mut().unwrap();
                    inspect.preview_segments = None;
                    inspect.preview_residual = None;
                } else {
                    let Some(original) = self.tracks.iter().find(|t| t.id == inspect.track_id) else {
                        return Task::none();
                    };
                    let inspect = self.track_inspection.as_mut().unwrap();
                    if inspect.first_pass_segments.is_none() {
                        let first = if inspect.report.can_reorder {
                            recovery::chronological_segments(original)
                        } else {
                            let ranges = chosen_ranges(inspect, original);
                            if ranges.is_empty() { return Task::none(); }
                            Some(recovery::exclude_ranges(original, &ranges))
                        };
                        let Some(first) = first else { return Task::none(); };
                        let second_track = recovery::with_segments(original, first.clone());
                        inspect.second_report = Some(recovery::analyze_with_mode(
                            &second_track, inspect.movement_mode));
                        inspect.first_pass_segments = Some(first);
                        inspect.second_selected.clear();
                        inspect.second_selected_reviews.clear();
                        inspect.second_focused = None;
                    }
                    // Both operator-approved passes are reflected in ONE
                    // non-destructive preview and ONE derived copy.
                    recompute_final_preview(inspect, original);
                }
                self.tracks_cache.clear();
            }
            Message::InspectionCreateCopy => {
                let Some(inspect) = &self.track_inspection else { return Task::none() };
                // Enforce preview on the model path too, not just in the button.
                if inspect.preview_segments.is_none() { return Task::none(); }
                let Some(original) = self.tracks.iter().find(|t| t.id == inspect.track_id).cloned() else {
                    return Task::none();
                };
                // Commit EXACTLY the confirmed preview. Recomputing only the
                // first pass here would silently drop the user's second cuts.
                let segments = inspect.preview_segments.as_ref().unwrap().clone();
                let label = if inspect.report.can_reorder {
                    crate::t!("msg_0432")
                } else { crate::t!("msg_0433") };
                if segments.iter().all(|s| s.points.is_empty()) { return Task::none(); }
                let mut copy = recovery::with_segments(&original, segments);
                copy.id = self.next_id;
                copy.name = format!("{} ({})", original.name, label);
                copy.props.description = copy.name.clone();
                copy.source = domain::FileSource::New;
                copy.visible = true;

                self.push_undo(crate::t!("msg_0434"));
                self.next_id += 1;
                // Keep the source's geometry and metadata intact; hiding it is
                // a view-only change and is captured in the same Undo record.
                if let Some(source) = self.tracks.iter_mut().find(|t| t.id == original.id) {
                    Arc::make_mut(source).visible = false;
                }
                let copy_id = copy.id;
                self.tracks.push(Arc::new(copy));
                self.selection.tracks.clear();
                self.selection.tracks.insert(copy_id);
                self.selection.points.clear();
                self.track_inspection = None;
                self.tracks_cache.clear();
                self.status = crate::t!("msg_0435").into();
            }
            _ => {}
        }
        Task::none()
    }
}


#[cfg(test)]
mod inspection_selection_tests {
    use super::*;
    use crate::domain::{FileSource, TrackPoint, TrackProps, TrackSegment};

    #[test]
    fn select_all_review_indexes_excludes_suggested_episodes() {
        use domain::quality::{Episode, Verdict};
        let ep = |verdict| Episode { segment: 0, start: 1, end: 2,
            verdict, reason: None, bad_fixes: 2, suspect_steps: 1, internal_m: 30.0, bridge_m: 10.0 };
        let episodes = vec![ep(Verdict::Review), ep(Verdict::Suggest), ep(Verdict::Review)];
        assert_eq!(review_indices(&episodes), HashSet::from([0usize, 2usize]));
    }

    #[test]
    fn confirmed_review_at_segment_edges_can_be_removed_without_a_bridge() {
        use domain::quality::{Episode, Verdict, ReviewReason};
        let points = (0..12).map(|i| TrackPoint {
            lat: 55.0, lon: 37.0 + i as f64 * 0.00001,
            ele: None, time: Some(i as f64), hdop: Some(2.0),
            speed_m_s: Some(1.0),
        }).collect();
        let track = domain::Track {
            id: 55, name: "edge".into(), color: [0,0,0], visible: true,
            segments: vec![TrackSegment { points }], source: FileSource::New,
            props: TrackProps::default(), cached_points: 0, cached_distance_m: 0.0,
        };
        let mut report = recovery::analyze_with_mode(&track, recovery::MovementMode::Vehicle);
        let ep=|start,end| Episode { segment:0, start, end,
            verdict:Verdict::Review, reason:Some(ReviewReason::SegmentEdge),
            bad_fixes:1, suspect_steps:0, internal_m:0.0, bridge_m:f64::NAN };
        report.quality.episodes = vec![ep(0,0),ep(10,11)];
        let inspect = TrackInspection { track_id: 55, report,
            movement_mode: recovery::MovementMode::Vehicle,
            focused:None, focused_jump:None, focused_quality:None,
            selected: HashSet::new(),
            selected_reviews: HashSet::from([0,1]),
            first_pass_segments:None, second_report:None,
            second_selected:HashSet::new(), second_selected_reviews:HashSet::new(),
            second_focused:None, extra_passes:Vec::new(), preview_segments:None, preview_residual:None };
        let ranges=chosen_ranges(&inspect,&track);
        assert_eq!(ranges.len(),2);
        let pieces=recovery::exclude_ranges(&track,&ranges);
        assert_eq!(pieces.len(),1);
        assert_eq!(pieces[0].points.len(),9);
        assert_eq!(pieces[0].points[0].time,Some(1.0));
        assert_eq!(pieces[0].points[8].time,Some(9.0));
        assert_eq!(track.segments[0].points.len(),12);
    }

    #[test]
    fn focused_suspect_points_are_editable_after_closing_inspection() {
        let points = (0..20).map(|i| TrackPoint {
            lat: 55.0 + if i == 10 { 0.004 } else { 0.0 },
            lon: 37.0 + i as f64 * 0.00001,
            ele: None, time: Some(i as f64), hdop: None, speed_m_s: None,
        }).collect();
        let track=domain::Track {
            id: 12, name: "selection".into(), color: [0,0,0], visible:true,
            segments: vec![TrackSegment { points }], source: FileSource::New,
            props: TrackProps::default(), cached_points:0, cached_distance_m:0.0,
        };
        let report=recovery::analyze_with_mode(&track,recovery::MovementMode::Foot);
        let idx=report.ranges.iter().position(|r|r.start==10 && r.end==10).unwrap();
        let inspect=TrackInspection {
            track_id: track.id, report, movement_mode: recovery::MovementMode::Foot,
            focused: Some(idx), focused_jump: None, focused_quality: None,
            selected: HashSet::new(), selected_reviews: HashSet::new(), first_pass_segments: None, second_report: None,
                    second_selected: HashSet::new(), second_selected_reviews: HashSet::new(),
                    second_focused: None, extra_passes: Vec::new(), preview_segments: None, preview_residual: None,
        };
        let points=points_to_select_after_inspection(&inspect,&track);
        assert_eq!(points,HashSet::from([(12,0,10)]));
    }


    fn iter_test_track() -> domain::Track {
        let points = (0..12).map(|i| TrackPoint {
            lat: 55.0, lon: 37.0 + i as f64 * 0.00001,
            ele: None, time: Some(i as f64), hdop: None, speed_m_s: None,
        }).collect();
        domain::Track { id: 77, name: "two-pass".into(), visible: true,
            color: [0,0,0], segments: vec![TrackSegment { points }],
            source: FileSource::New, props: TrackProps::default(),
            cached_points: 0, cached_distance_m: 0.0 }
    }

    #[test]
    fn third_pass_finds_a_new_edge_after_second_cut_without_index_drift() {
        // Original points 0..11. First approved pass removes 3..4.
        // Second approved pass removes original 7. A residual boundary
        // then becomes visible as original 8 in segment 2 of the NEW track.
        let original = iter_test_track();
        let mut report = recovery::analyze_with_mode(&original, recovery::MovementMode::Vehicle);
        report.ranges = vec![recovery::SuspectRange { segment: 0, start: 3, end: 4,
            entry_m:0.0, exit_m:0.0, bridge_m:0.0 }];
        let first = recovery::exclude_ranges(&original, &report.ranges);
        let mut second = recovery::analyze_with_mode(
            &recovery::with_segments(&original, first.clone()), recovery::MovementMode::Vehicle);
        second.ranges = vec![recovery::SuspectRange { segment: 1, start: 2, end: 2,
            entry_m:0.0, exit_m:0.0, bridge_m:0.0 }];
        let mut inspect = TrackInspection {
            track_id: original.id, report, movement_mode: recovery::MovementMode::Vehicle,
            focused:None, focused_jump:None, focused_quality:None,
            selected:HashSet::from([0]), selected_reviews:HashSet::new(),
            first_pass_segments:Some(first), second_report:Some(second),
            second_selected:HashSet::from([0]), second_selected_reviews:HashSet::new(),
            second_focused:None, extra_passes:Vec::new(),
            preview_segments:None, preview_residual:None,
        };
        recompute_final_preview(&mut inspect, &original);
        let second_geometry = inspect.preview_segments.clone().unwrap();
        assert_eq!(second_geometry.iter().map(|s|s.points.len()).sum::<usize>(), 9);
        assert_eq!(second_geometry.len(), 3);
        assert!(has_further_scan_input(&inspect));
        let mut third = recovery::analyze_with_mode(
            &recovery::with_segments(&original, second_geometry.clone()),
            recovery::MovementMode::Vehicle);
        third.ranges = vec![recovery::SuspectRange { segment:2, start:0, end:0,
            entry_m:f64::NAN, exit_m:0.0, bridge_m:f64::NAN }];
        inspect.extra_passes.push(InspectionExtraPass {
            source_segments:second_geometry, report:third,
            selected:HashSet::new(), selected_reviews:HashSet::new(), focused:Some((false,0)),
        });
        // No implicit deletion when a new diagnosis is produced.
        recompute_final_preview(&mut inspect, &original);
        assert_eq!(inspect.preview_segments.as_ref().unwrap().iter()
            .map(|s|s.points.len()).sum::<usize>(), 9);
        assert_eq!(points_to_select_after_inspection(&inspect, &original),
            HashSet::from([(original.id,0,8)]));
        inspect.extra_passes[0].selected.insert(0);
        recompute_final_preview(&mut inspect, &original);
        let times: Vec<_> = inspect.preview_segments.as_ref().unwrap().iter()
            .flat_map(|s|s.points.iter()).map(|p|p.time.unwrap() as usize).collect();
        assert_eq!(times, vec![0,1,2,5,6,9,10,11]);
        assert_eq!(original.segments[0].points.len(), 12);
        reset_inspection_preview(&mut inspect);
        assert!(inspect.extra_passes.is_empty());
    }

    #[test]
    fn second_pass_cut_uses_intermediate_indices_and_preserves_all_gaps() {
        // The first cut removes original points 3..4. The second pass
        // removes index 2 from the NEW segment containing original points
        // 5,6,7,..., i.e. original 7 (not original point 2).
        let track=iter_test_track();
        let mut report=recovery::analyze_with_mode(&track, recovery::MovementMode::Vehicle);
        report.ranges = vec![recovery::SuspectRange { segment:0,start:3,end:4,
            entry_m:0.0,exit_m:0.0,bridge_m:0.0 }];
        let first=recovery::exclude_ranges(&track,&report.ranges);
        assert_eq!(first.len(),2);
        let mut inspect=TrackInspection {
            track_id:77, report, movement_mode:recovery::MovementMode::Vehicle,
            focused:None, focused_jump:None, focused_quality:None,
            selected:HashSet::from([0]), selected_reviews:HashSet::new(),
            first_pass_segments:Some(first.clone()), second_report:None,
            second_selected:HashSet::new(), second_selected_reviews:HashSet::new(),
            second_focused:None, extra_passes:Vec::new(), preview_segments:None, preview_residual:None,
        };
        let mut second_report=recovery::analyze_with_mode(
            &recovery::with_segments(&track,first),recovery::MovementMode::Vehicle);
        second_report.ranges=vec![recovery::SuspectRange { segment:1,start:2,end:2,
            entry_m:0.0,exit_m:0.0,bridge_m:0.0 }];
        inspect.second_report=Some(second_report);
        // Second pass must never cut anything before a user checks it.
        recompute_final_preview(&mut inspect,&track);
        let preview=inspect.preview_segments.as_ref().unwrap();
        assert_eq!(preview.iter().map(|s|s.points.len()).sum::<usize>(),10);
        inspect.second_selected.insert(0);
        recompute_final_preview(&mut inspect,&track);
        let preview=inspect.preview_segments.as_ref().unwrap();
        assert_eq!(preview.len(),3);
        assert_eq!(preview.iter().map(|s|s.points.len()).sum::<usize>(),9);
        let remaining_times: Vec<_>=preview.iter().flat_map(|s|s.points.iter())
            .map(|p|p.time.unwrap() as usize).collect();
        assert_eq!(remaining_times,vec![0,1,2,5,6,8,9,10,11]);
        assert_eq!(track.segments[0].points.len(),12);
        // Focused second-pass point 7 maps to original index 7, not 2.
        inspect.second_focused=Some((false,0));
        assert_eq!(points_to_select_after_inspection(&inspect,&track),
            HashSet::from([(77,0,7)]));
        reset_inspection_preview(&mut inspect);
        assert!(inspect.second_report.is_none());
        assert!(inspect.second_selected.is_empty());
        assert!(inspect.preview_segments.is_none());
    }
}

#[cfg(test)]
mod one_click_cleanup_tests {
    use super::*;
    use crate::domain::{FileSource, TrackPoint, TrackProps, TrackSegment};
    use crate::domain::quality::{Episode, Verdict};

    fn track() -> domain::Track {
        let points = (0..12).map(|i| TrackPoint {
            lat: 55.0, lon: 37.0 + i as f64 * 0.00001,
            ele: None, time: Some(i as f64), hdop: None, speed_m_s: None,
        }).collect();
        domain::Track {
            id: 12, name: "one-click".into(), color: [0, 0, 0], visible: true,
            segments: vec![TrackSegment { points }], source: FileSource::New,
            props: TrackProps::default(), cached_points: 0, cached_distance_m: 0.0,
        }
    }

    #[test]
    fn healthy_track_selects_nothing_and_does_not_change_data() {
        let track = track();
        let report = recovery::analyze_with_mode(&track, recovery::MovementMode::Vehicle);
        assert!(cleanup_point_selection(&track, &report).is_empty());
        assert_eq!(track.segments[0].points.len(), 12);
    }

    #[test]
    fn suggestions_reviews_and_unresolved_jump_endpoints_are_all_selected() {
        let track = track();
        let mut report = recovery::analyze_with_mode(&track, recovery::MovementMode::Vehicle);
        report.ranges = vec![recovery::SuspectRange {
            segment: 0, start: 2, end: 4,
            entry_m: 1.0, exit_m: 1.0, bridge_m: 1.0,
        }];
        report.quality.episodes = vec![Episode {
            segment: 0, start: 8, end: 9, verdict: Verdict::Review,
            reason: None, bad_fixes: 2, suspect_steps: 0,
            internal_m: 0.0, bridge_m: 0.0,
        }];
        report.severe_jumps = vec![recovery::SevereJump {
            segment: 0, to_index: 6, distance_m: 400.0, speed_m_s: 100.0,
        }];
        let found = cleanup_point_selection(&track, &report);
        let expected: HashSet<_> = [2, 3, 4, 5, 6, 8, 9].into_iter()
            .map(|i| (track.id, 0usize, i)).collect();
        assert_eq!(found, expected);
        // Only highlighting; no edit and no derived track.
        assert_eq!(track.segments[0].points.len(), 12);
    }

    #[test]
    fn auto_profile_uses_safely_restored_chronology() {
        let mut track = track();
        track.segments[0].points = (0..301).map(|i| TrackPoint {
            lat: 55.0, lon: 37.0 + i as f64 * 14.0 / 63_770.0,
            ele: None, time: Some(i as f64), hdop: Some(5.0),
            speed_m_s: Some(14.0),
        }).collect();
        track.segments[0].points.swap(50, 250);
        let report = recovery::analyze_with_mode(&track, recovery::MovementMode::Vehicle);
        assert!(report.can_reorder);
        assert_eq!(one_click_auto_profile(&track, &report),
            recovery::MovementMode::Vehicle);
    }

    #[test]
    fn safe_ordering_restores_observations_without_creating_more_points() {
        let mut track = track();
        track.segments[0].points.swap(4, 5);
        let ordered = recovery::chronological_segments(&track).unwrap();
        assert!(ordered[0].points.windows(2).all(|p| p[0].time < p[1].time));
        assert_eq!(ordered[0].points.len(), track.segments[0].points.len());
        assert_ne!(ordered[0].points[4].time, track.segments[0].points[4].time);
    }
}
